//! Reading and changing the messages of one mailbox (`docs/mail.md`, 「Reading」 and 「Where each piece of
//! state lives」). Nothing here keeps a copy of a message: every list and every thread is read from the
//! server when it is asked for.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};

use async_imap::imap_proto::SectionPath;
use async_imap::types::Fetch;
use futures::TryStreamExt;
use icu_collator::options::{CollatorOptions, Strength};
use icu_collator::preferences::CollationNumericOrdering;
use icu_collator::{Collator, CollatorBorrowed, CollatorPreferences};
use mail_parser::{Address, HeaderName, MessageParser};

use crate::imap::charset::{decode_transfer, part_text};
use crate::imap::conversation::{conversation_key, id_key};
use crate::imap::folders::{ensure_role_folder, folders_of, list_folders};
use crate::imap::moved_in::{begin_move_in, moved_in};
use crate::imap::search::Search;
use crate::imap::session::{Connection, MailSession};
use crate::imap::structure::{message_parts, snippet_part, snippet_text};
use crate::imap::{
  Landed, expunge, flag_name, part_number, quoted, run_landing, store_flags, uid_fetch, uid_search,
  uid_set, wire_name,
};
use crate::labels::label_id_of;
use crate::routes::requests::{Asked, MailFault};
use crate::wire::{
  FolderRole, MailAddress, MailConversation, MailFolder, MailFolderState, MailListMessage, MailMessage,
  MailPage, MailPart, MailRef, MailSort, MailSortBy, MailSummaryMessage,
};

/// How many messages a page of a list holds.
pub const PAGE_SIZE: usize = 50;

/// The part of a body a snippet is cut from, before its encoding is undone.
const SNIPPET_BYTES: usize = 800;

/// The most of one body part a message's view reads. A body past it is shown cut.
const BODY_BYTES: usize = 2 * 1024 * 1024;

/// The system flags and keywords a request may set or clear. A label's keyword is the other thing a request
/// may set, and is checked by its own form.
const SETTABLE: [&str; 7] =
  ["\\Seen", "\\Flagged", "\\Answered", "$Forwarded", "$Junk", "$NotJunk", "$MDNSent"];

/// The headers a row and a message are read from.
const HEADER_FIELDS: &str = "BODY.PEEK[HEADER.FIELDS (DATE SUBJECT FROM TO CC BCC REPLY-TO MESSAGE-ID IN-REPLY-TO REFERENCES DISPOSITION-NOTIFICATION-TO)]";

/// The receipt request's header, read as an address.
const RECEIPT_HEADER: &str = "Disposition-Notification-To";

/// What a fetch of rows asks for.
fn row_items() -> String {
  format!("(UID FLAGS BODYSTRUCTURE INTERNALDATE {HEADER_FIELDS})")
}

/// A message's headers, read.
#[derive(Debug, Default)]
pub struct Heads {
  pub subject: String,
  pub from: Vec<MailAddress>,
  pub to: Vec<MailAddress>,
  pub cc: Vec<MailAddress>,
  pub bcc: Vec<MailAddress>,
  pub reply_to: Vec<MailAddress>,
  /// The date the message says it was written, in Unix milliseconds.
  pub date: Option<i64>,
  /// Its own message ID, as written without its angle brackets.
  pub message_id: Option<String>,
  pub in_reply_to: Vec<String>,
  pub references: Vec<String>,
  /// Where a read receipt is asked to go.
  pub receipt_to: Option<String>,
}

fn addresses(address: Option<&Address<'_>>) -> Vec<MailAddress> {
  address
    .map(|address| {
      address
        .iter()
        .filter_map(|one| {
          Some(MailAddress {
            name: one.name().unwrap_or("").to_owned(),
            address: one.address()?.to_lowercase(),
          })
        })
        .collect()
    })
    .unwrap_or_default()
}

/// The headers of a header block, decoded.
pub fn heads(raw: Option<&[u8]>) -> Heads {
  let Some(raw) = raw else {
    return Heads::default();
  };
  let parser = MessageParser::new()
    .with_address_headers()
    .with_date_headers()
    .with_message_ids()
    .header_text(HeaderName::Subject)
    .header_address(HeaderName::Other(RECEIPT_HEADER.into()));
  let Some(message) = parser.parse_headers(raw) else {
    return Heads::default();
  };
  let ids = |value: &mail_parser::HeaderValue<'_>| {
    value
      .as_text_list()
      .map(|ids| ids.iter().map(|id| id.to_string()).collect())
      .unwrap_or_default()
  };

  Heads {
    subject: message.subject().unwrap_or("").to_owned(),
    from: addresses(message.from()),
    to: addresses(message.to()),
    cc: addresses(message.cc()),
    bcc: addresses(message.bcc()),
    reply_to: addresses(message.reply_to()),
    date: message.date().map(|date| date.to_timestamp() * 1000),
    message_id: message.message_id().map(str::to_owned),
    in_reply_to: ids(message.in_reply_to()),
    references: ids(message.references()),
    receipt_to: message
      .header(HeaderName::Other(RECEIPT_HEADER.into()))
      .and_then(|value| value.as_address())
      .and_then(|address| address.first())
      .and_then(|one| one.address())
      .map(|address| address.trim().to_lowercase()),
  }
}

/// A row of a list, its headers and its parts, from what a fetch answered for one message.
fn read_row(folder: &str, fetch: &Fetch) -> Option<(MailListMessage, Heads, crate::wire::MailParts)> {
  let uid = fetch.uid?;
  let flags: Vec<String> = fetch.flags().map(|flag| flag_name(&flag)).collect();
  let heads = heads(fetch.header());
  let parts = message_parts(fetch.bodystructure());
  let key = conversation_key(heads.message_id.as_deref(), &heads.in_reply_to, &heads.references)
    .unwrap_or_else(|| format!("{folder}\u{0}{uid}"));
  let date = fetch
    .internal_date()
    .map(|date| date.timestamp_millis())
    .filter(|date| *date != 0)
    .or(heads.date)
    .unwrap_or(0);
  let has = |name: &str| flags.iter().any(|flag| flag == name);
  let row = MailListMessage {
    folder: folder.to_owned(),
    uid,
    key,
    message_id: heads.message_id.as_deref().map(id_key),
    subject: heads.subject.clone(),
    from: heads.from.clone(),
    to: heads.to.clone(),
    date,
    seen: has("\\Seen"),
    flagged: has("\\Flagged"),
    answered: has("\\Answered"),
    draft: has("\\Draft"),
    receipt_answered: has("$MDNSent"),
    labels: flags.iter().filter_map(|flag| label_id_of(flag).map(str::to_owned)).collect(),
    has_attachments: !parts.attachments.is_empty(),
    snippet: String::new(),
  };

  Some((row, heads, parts))
}

/// The section a part number names in a fetch's answer.
fn section(part: &str) -> SectionPath {
  SectionPath::Part(part.split('.').filter_map(|number| number.parse().ok()).collect(), None)
}

/// The list rows of the given messages of the selected folder, in the order given, their snippets read in
/// one fetch per part number rather than one per message.
async fn list_rows(
  connection: &mut Connection,
  folder: &str,
  uids: &[u32],
) -> Result<Vec<MailListMessage>, MailFault> {
  let fetched = uid_fetch(connection.session(), uids, &row_items()).await?;
  let mut rows: HashMap<u32, MailListMessage> = HashMap::new();
  let mut by_part: BTreeMap<String, Vec<(u32, MailPart)>> = BTreeMap::new();

  for fetch in &fetched {
    let Some((row, _, _)) = read_row(folder, fetch) else {
      continue;
    };

    if let Some(part) = snippet_part(fetch.bodystructure()) {
      by_part.entry(part.part.clone()).or_default().push((row.uid, part));
    }

    rows.insert(row.uid, row);
  }

  for (key, wanted) in by_part {
    let uids: Vec<u32> = wanted.iter().map(|(uid, _)| *uid).collect();
    let bodies =
      uid_fetch(connection.session(), &uids, &format!("(UID BODY.PEEK[{key}]<0.{SNIPPET_BYTES}>)")).await?;

    for body in &bodies {
      let Some(uid) = body.uid else {
        continue;
      };
      let one = wanted.iter().find(|(wanted, _)| *wanted == uid).map(|(_, part)| part);

      if let (Some(part), Some(bytes), Some(row)) = (one, body.section(&section(&key)), rows.get_mut(&uid))
      {
        let text = part_text(bytes, part.encoding.as_deref(), part.charset.as_deref());

        row.snippet = snippet_text(&text, part.kind == "text/html");
      }
    }
  }

  Ok(uids.iter().filter_map(|uid| rows.remove(uid)).collect())
}

/// The messages of a page gathered into the conversations they make up, in the order they were given: the
/// page is already ordered by the column the list stands on, and a conversation takes the place of its
/// first message in it. What the row says about the conversation — its date, its senders, its snippet — is
/// read from the newest message of the group whatever that order is.
pub fn conversations(messages: Vec<MailListMessage>) -> Vec<MailConversation> {
  let mut keys: Vec<String> = Vec::new();
  let mut groups: HashMap<String, Vec<MailListMessage>> = HashMap::new();

  for message in messages {
    if !groups.contains_key(&message.key) {
      keys.push(message.key.clone());
    }

    groups.entry(message.key.clone()).or_default().push(message);
  }

  keys
    .into_iter()
    .map(|key| {
      let group = groups.remove(&key).unwrap_or_default();
      let mut by_date: Vec<&MailListMessage> = group.iter().collect();

      by_date.sort_by(|a, b| b.date.cmp(&a.date));

      let newest = by_date[0];
      let oldest = by_date[by_date.len() - 1];
      let mut from: Vec<MailAddress> = Vec::new();

      for message in &by_date {
        for sender in &message.from {
          if !from.iter().any(|one| one.address == sender.address) {
            from.push(sender.clone());
          }
        }
      }

      let mut labels: Vec<String> = Vec::new();

      for label in group.iter().flat_map(|message| message.labels.iter()) {
        if !labels.contains(label) {
          labels.push(label.clone());
        }
      }

      MailConversation {
        key,
        subject: if oldest.subject.is_empty() { newest.subject.clone() } else { oldest.subject.clone() },
        date: newest.date,
        from,
        count: group.len(),
        unread: group.iter().any(|message| !message.seen),
        flagged: group.iter().any(|message| message.flagged),
        labels,
        has_attachments: group.iter().any(|message| message.has_attachments),
        snippet: newest.snippet.clone(),
        messages: group
          .iter()
          .map(|message| MailRef {
            folder: message.folder.clone(),
            uid: message.uid,
          })
          .collect(),
      }
    })
    .collect()
}

/// One folder's page read by each message's place in the folder rather than by a search: the only reading
/// of a list that does not touch every message the folder holds, and the one every list standing as it
/// stands by default is read by. It answers the date column alone, from whichever end that column runs.
pub async fn folder_page(
  session: &MailSession,
  folder: &str,
  offset: usize,
  ascending: bool,
) -> Result<MailPage, MailFault> {
  let mut connection = session.folder(folder).await?;
  let done = async {
    let total = connection.exists() as usize;

    if offset >= total {
      return Ok(MailPage {
        total,
        offset,
        conversations: Vec::new(),
      });
    }

    let (low, high) = if ascending {
      (offset + 1, total.min(offset + PAGE_SIZE))
    } else {
      ((total - offset).saturating_sub(PAGE_SIZE - 1).max(1), total - offset)
    };
    let found: Vec<Fetch> = connection
      .session()
      .fetch(format!("{low}:{high}"), "(UID)")
      .await
      .map_err(MailFault::server)?
      .try_collect()
      .await
      .map_err(MailFault::server)?;
    let mut uids: Vec<u32> = found.iter().filter_map(|fetch| fetch.uid).filter(|uid| *uid > 0).collect();

    uids.sort_by(|a, b| if ascending { a.cmp(b) } else { b.cmp(a) });

    let rows = list_rows(&mut connection, folder, &uids).await?;

    Ok(MailPage {
      total,
      offset,
      conversations: conversations(rows),
    })
  }
  .await;

  connection.settle(done)
}

/// Names and subjects compared the way the screen's language reads them, which for Korean is not the order
/// of their code points: letters alike whatever their case and accents, and numbers by their value.
pub struct NameOrder(CollatorBorrowed<'static>);

impl NameOrder {
  pub fn new(language: &str) -> Self {
    let mut preferences = match icu_locale::Locale::try_from_str(language) {
      Ok(locale) => CollatorPreferences::from(&locale),
      Err(_) => CollatorPreferences::default(),
    };
    let mut options = CollatorOptions::default();

    preferences.numeric_ordering = Some(CollationNumericOrdering::True);
    options.strength = Some(Strength::Primary);

    let collator = Collator::try_new(preferences, options)
      .or_else(|_| Collator::try_new(CollatorPreferences::default(), options))
      .expect("the root collation is compiled in");

    Self(collator)
  }

  fn compare(&self, a: &str, b: &str) -> Ordering {
    self.0.compare(a, b)
  }
}

/// What one message of a view is ordered by, before its row is read. A date is on every hit; a sender and a
/// subject are read from the headers, and only where the list stands on them.
struct Hit {
  folder: String,
  uid: u32,
  date: i64,
  from: String,
  subject: String,
}

/// The sender a row names, as far as the headers name it: the first sender's own name, or the part of their
/// address before `@`.
fn sender_of(heads: &Heads) -> String {
  heads
    .from
    .first()
    .map(|first| {
      if first.name.is_empty() {
        first.address.split('@').next().unwrap_or("").to_owned()
      } else {
        first.name.clone()
      }
    })
    .unwrap_or_default()
}

/// Ascending, always: the oldest date, the first name, the first subject. The other way round is this same
/// order read backwards, so one comparison answers both.
fn compare_hits(a: &Hit, b: &Hit, by: MailSortBy, order: &NameOrder) -> Ordering {
  match by {
    MailSortBy::From => order.compare(&a.from, &b.from).then(a.date.cmp(&b.date)),
    MailSortBy::Subject => order.compare(&a.subject, &b.subject).then(a.date.cmp(&b.date)),
    MailSortBy::Date => a.date.cmp(&b.date).then(a.uid.cmp(&b.uid)),
  }
}

/// A page of a view across folders, or of one folder ordered by something other than its own arrival
/// order: the search is run in each folder and every hit is read far enough to be put in order, which for
/// a name or a subject is the headers of every message the view holds.
pub async fn search_page(
  session: &MailSession,
  folders: &[MailFolder],
  search: &Search,
  offset: usize,
  sort: MailSort,
  order: &NameOrder,
) -> Result<MailPage, MailFault> {
  let mut hits: Vec<Hit> = Vec::new();
  let named = sort.by != MailSortBy::Date;
  let items = if named {
    "(UID INTERNALDATE BODY.PEEK[HEADER.FIELDS (FROM SUBJECT)])"
  } else {
    "(UID INTERNALDATE)"
  };

  for folder in folders {
    let mut connection = session.folder(&folder.path).await?;
    let done = async {
      let uids = uid_search(connection.session(), &search.command()).await?;

      uid_fetch(connection.session(), &uids, items).await
    }
    .await;

    for fetch in connection.settle(done)? {
      let Some(uid) = fetch.uid else {
        continue;
      };
      let heads = if named { heads(fetch.header()) } else { Heads::default() };

      hits.push(Hit {
        folder: folder.path.clone(),
        uid,
        date: fetch.internal_date().map_or(0, |date| date.timestamp_millis()),
        from: sender_of(&heads),
        subject: heads.subject.trim().to_owned(),
      });
    }
  }

  hits.sort_by(|a, b| {
    let ascending = compare_hits(a, b, sort.by, order);

    if sort.ascending { ascending } else { ascending.reverse() }
  });

  let total = hits.len();
  let page: Vec<&Hit> = hits.iter().skip(offset).take(PAGE_SIZE).collect();
  let mut folders_in_page: Vec<&str> = Vec::new();

  for hit in &page {
    if !folders_in_page.contains(&hit.folder.as_str()) {
      folders_in_page.push(&hit.folder);
    }
  }

  let mut read: HashMap<(String, u32), MailListMessage> = HashMap::new();

  for folder in folders_in_page {
    let uids: Vec<u32> = page.iter().filter(|hit| hit.folder == folder).map(|hit| hit.uid).collect();
    let mut connection = session.folder(folder).await?;
    let done = list_rows(&mut connection, folder, &uids).await;

    for row in connection.settle(done)? {
      read.insert((row.folder.clone(), row.uid), row);
    }
  }

  let ordered = page.iter().filter_map(|hit| read.remove(&(hit.folder.clone(), hit.uid))).collect();

  Ok(MailPage {
    total,
    offset,
    conversations: conversations(ordered),
  })
}

/// One body part's text, read up to the most a view reads.
async fn read_body(
  connection: &mut Connection,
  uid: u32,
  part: Option<&MailPart>,
) -> Result<Option<String>, MailFault> {
  let Some(part) = part else {
    return Ok(None);
  };
  let fetched =
    uid_fetch(connection.session(), &[uid], &format!("(UID BODY.PEEK[{}]<0.{BODY_BYTES}>)", part.part))
      .await?;

  Ok(
    fetched
      .first()
      .and_then(|fetch| fetch.section(&section(&part.part)))
      .map(|bytes| part_text(bytes, part.encoding.as_deref(), part.charset.as_deref())),
  )
}

/// One message of the selected folder with its bodies.
async fn read_in_folder(
  connection: &mut Connection,
  folder: &str,
  uid: u32,
) -> Result<Option<MailMessage>, MailFault> {
  let fetched = uid_fetch(connection.session(), &[uid], &row_items()).await?;
  let Some((listed, heads, parts)) = fetched.first().and_then(|fetch| read_row(folder, fetch)) else {
    return Ok(None);
  };
  let text = read_body(connection, uid, parts.text.as_ref()).await?;
  let html = read_body(connection, uid, parts.html.as_ref()).await?;

  Ok(Some(MailMessage {
    listed,
    cc: heads.cc,
    bcc: heads.bcc,
    reply_to: heads.reply_to,
    in_reply_to: heads.in_reply_to.first().map(|id| id_key(id)),
    references: heads.references.iter().map(|id| id_key(id)).collect(),
    receipt_to: heads.receipt_to,
    text,
    html,
    attachments: parts.attachments,
    inline: parts.inline,
  }))
}

/// One message with its bodies. Reading it does not mark it read; the screen says so on its own.
pub async fn read_message(session: &MailSession, at: &MailRef) -> Result<Option<MailMessage>, MailFault> {
  let mut connection = session.folder(&at.folder).await?;
  let done = read_in_folder(&mut connection, &at.folder, at.uid).await;

  connection.settle(done)
}

/// Every message of a conversation: those the conversation's key names, read from the inbox, the sent
/// folders and the folder it was opened from, oldest first, a message found in two folders once.
pub async fn read_thread(
  session: &MailSession,
  key: &str,
  opened_in: &str,
) -> Result<Vec<MailMessage>, MailFault> {
  if let Some((folder, uid)) = key.rsplit_once('\u{0}')
    && let Ok(uid) = uid.parse::<u32>()
  {
    let one = read_message(
      session,
      &MailRef {
        folder: folder.to_owned(),
        uid,
      },
    )
    .await?;

    return Ok(one.into_iter().collect());
  }

  let folders = folders_of(session).await?;
  let mut paths: Vec<String> = vec!["INBOX".to_owned()];

  for path in folders
    .iter()
    .filter(|folder| folder.role == Some(FolderRole::Sent))
    .map(|folder| folder.path.clone())
    .chain(std::iter::once(opened_in.to_owned()))
  {
    if !paths.contains(&path) {
      paths.push(path);
    }
  }

  let search = Search::Or(vec![
    Search::Header("message-id".to_owned(), key.to_owned()),
    Search::Header("references".to_owned(), key.to_owned()),
    Search::Header("in-reply-to".to_owned(), key.to_owned()),
  ]);
  let mut found: Vec<MailMessage> = Vec::new();

  for path in paths.iter().filter(|path| folders.iter().any(|folder| &folder.path == *path)) {
    let mut connection = session.folder(path).await?;
    let done = async {
      let mut uids = uid_search(connection.session(), &search.command()).await?;
      let mut read = Vec::new();

      uids.sort_unstable();

      for uid in uids {
        if let Some(message) = read_in_folder(&mut connection, path, uid).await?
          && message.listed.key == key
        {
          read.push(message);
        }
      }

      Ok(read)
    }
    .await;

    found.extend(connection.settle(done)?);
  }

  found.sort_by(|a, b| a.listed.date.cmp(&b.listed.date));

  let mut seen: HashSet<String> = HashSet::new();

  Ok(
    found
      .into_iter()
      .filter(|message| match &message.listed.message_id {
        None => true,
        Some(id) => seen.insert(id.clone()),
      })
      .collect(),
  )
}

/// Every leaf part of a message's structure.
fn leaves(fetch: &Fetch) -> Vec<MailPart> {
  let parts = message_parts(fetch.bodystructure());

  parts
    .text
    .into_iter()
    .chain(parts.html)
    .chain(parts.attachments)
    .chain(parts.inline)
    .collect()
}

/// A part of a message, its transfer encoding undone, with its type and its name.
pub struct OpenedPart {
  pub bytes: Vec<u8>,
  pub kind: Option<String>,
  pub filename: Option<String>,
}

/// A part of a message as it comes from the server, whole.
pub async fn open_part(session: &MailSession, at: &MailRef, part: &str) -> Result<OpenedPart, MailFault> {
  if !part_number(part) {
    return Err(MailFault::server("the part is not in the message"));
  }

  let mut connection = session.folder(&at.folder).await?;
  let done = async {
    let fetched =
      uid_fetch(connection.session(), &[at.uid], &format!("(UID BODYSTRUCTURE BODY.PEEK[{part}])")).await?;
    let fetch = fetched.first().ok_or_else(|| MailFault::server("the message is not on the server"))?;
    let meta = leaves(fetch).into_iter().find(|one| one.part == part);
    let bytes = fetch
      .section(&section(part))
      .ok_or_else(|| MailFault::server("the part is not in the message"))?;

    Ok(OpenedPart {
      bytes: decode_transfer(bytes, meta.as_ref().and_then(|meta| meta.encoding.as_deref())),
      kind: meta.as_ref().map(|meta| meta.kind.clone()),
      filename: meta.and_then(|meta| meta.filename),
    })
  }
  .await;

  connection.settle(done)
}

/// The part an HTML body names by `cid:`.
pub async fn find_inline_part(
  session: &MailSession,
  at: &MailRef,
  cid: &str,
) -> Result<Option<MailPart>, MailFault> {
  let wanted = cid.trim_start_matches('<').trim_end_matches('>');
  let mut connection = session.folder(&at.folder).await?;
  let done = uid_fetch(connection.session(), &[at.uid], "(UID BODYSTRUCTURE)").await;
  let fetched = connection.settle(done)?;

  Ok(fetched.first().and_then(|fetch| {
    let parts = message_parts(fetch.bodystructure());

    parts
      .inline
      .into_iter()
      .chain(parts.attachments)
      .find(|part| part.cid.as_deref() == Some(wanted))
  }))
}

/// The messages grouped by their folder, in the order the folders first come.
fn by_folder(refs: &[MailRef]) -> Vec<(String, Vec<u32>)> {
  let mut grouped: Vec<(String, Vec<u32>)> = Vec::new();

  for at in refs {
    match grouped.iter_mut().find(|(folder, _)| *folder == at.folder) {
      Some((_, uids)) => uids.push(at.uid),
      None => grouped.push((at.folder.clone(), vec![at.uid])),
    }
  }

  grouped
}

/// Whether a request may set or clear the flag.
pub fn settable_flag(flag: &str) -> bool {
  SETTABLE.contains(&flag) || label_id_of(flag).is_some()
}

/// Sets and clears flags on the given messages, in whichever folders they are.
pub async fn change_flags(
  session: &MailSession,
  refs: &[MailRef],
  add: &[String],
  remove: &[String],
) -> Result<(), MailFault> {
  if let Some(flag) = add.iter().chain(remove).find(|flag| !settable_flag(flag)) {
    return Err(Asked::new("bad_flag", "a flag is not one that may be set").with("flag", flag).into());
  }

  for (folder, uids) in by_folder(refs) {
    let mut connection = session.folder(&folder).await?;
    let done = async {
      store_flags(connection.session(), &uids, "+", add).await?;
      store_flags(connection.session(), &uids, "-", remove).await
    }
    .await;

    connection.settle(done)?;
  }

  Ok(())
}

/// Moves messages of the selected folder into another, and says where they landed where the server said. A
/// server without `MOVE` has it done by copy, flag and expunge.
async fn move_uids(
  connection: &mut Connection,
  uids: &[u32],
  destination: &str,
) -> Result<Option<Landed>, MailFault> {
  let capabilities = connection.session().capabilities().await.map_err(MailFault::server)?;
  let target = quoted(&wire_name(destination));

  if capabilities.has_str("MOVE") {
    return run_landing(connection.session(), &format!("UID MOVE {} {target}", uid_set(uids))).await;
  }

  let landed = run_landing(connection.session(), &format!("UID COPY {} {target}", uid_set(uids))).await?;

  store_flags(connection.session(), uids, "+", &["\\Deleted".to_owned()]).await?;
  expunge(connection.session(), uids, capabilities.has_str("UIDPLUS")).await?;

  Ok(landed)
}

/// Moves the given messages into one folder. A move into the inbox is noted with where the messages
/// landed, so the watcher does not take them for new mail.
pub async fn move_messages(
  session: &MailSession,
  refs: &[MailRef],
  destination: &str,
) -> Result<(), MailFault> {
  for (folder, uids) in by_folder(refs) {
    if folder == destination {
      continue;
    }

    let mut noted = (destination == "INBOX").then(|| begin_move_in(&session.account.address));
    let mut connection = session.folder(&folder).await?;
    let done = move_uids(&mut connection, &uids, destination).await;
    let landed = connection.settle(done)?;

    if let Some(noted) = noted.as_mut() {
      noted.landed(landed);
    }
  }

  Ok(())
}

/// Moves the given messages into the folder of the role, made where it does not exist yet.
pub async fn move_to_role(
  session: &MailSession,
  refs: &[MailRef],
  role: FolderRole,
) -> Result<String, MailFault> {
  let destination = ensure_role_folder(session, role).await?;

  move_messages(session, refs, &destination).await?;

  Ok(destination)
}

/// Spam reported: the learning keyword set, the messages moved into the junk folder.
pub async fn report_spam(session: &MailSession, refs: &[MailRef]) -> Result<(), MailFault> {
  change_flags(session, refs, &["$Junk".to_owned()], &["$NotJunk".to_owned()]).await?;
  move_to_role(session, refs, FolderRole::Junk).await.map(drop)
}

/// Not spam: the other keyword set, the messages moved back into the inbox.
pub async fn report_not_spam(session: &MailSession, refs: &[MailRef]) -> Result<(), MailFault> {
  change_flags(session, refs, &["$NotJunk".to_owned()], &["$Junk".to_owned()]).await?;
  move_to_role(session, refs, FolderRole::Inbox).await.map(drop)
}

/// Deletion for good, which only the junk and the trash folders allow.
pub async fn delete_forever(session: &MailSession, refs: &[MailRef]) -> Result<(), MailFault> {
  let folders = folders_of(session).await?;

  for (folder, uids) in by_folder(refs) {
    let role = folders.iter().find(|one| one.path == folder).and_then(|one| one.role);

    if role != Some(FolderRole::Junk) && role != Some(FolderRole::Trash) {
      return Err(MailFault::asked("delete_outside_trash", "a message to delete is not in spam or trash"));
    }

    let mut connection = session.folder(&folder).await?;
    let done = async {
      let uidplus =
        connection.session().capabilities().await.map_err(MailFault::server)?.has_str("UIDPLUS");

      store_flags(connection.session(), &uids, "+", &["\\Deleted".to_owned()]).await?;
      expunge(connection.session(), &uids, uidplus).await
    }
    .await;

    connection.settle(done)?;
  }

  Ok(())
}

/// Every folder of the mailbox with its unread and total counts, for the mail nav's marks.
pub async fn folder_states(session: &MailSession) -> Result<Vec<MailFolderState>, MailFault> {
  let mut connection = session.connection().await?;
  let done = async {
    let folders = list_folders(&mut connection).await?;
    let mut states = Vec::new();

    for folder in folders {
      let status = connection
        .session()
        .status(wire_name(&folder.path), "(MESSAGES UNSEEN)")
        .await
        .map_err(MailFault::server)?;

      states.push(MailFolderState {
        folder,
        unseen: status.unseen.unwrap_or(0),
        total: status.exists,
      });
    }

    Ok(states)
  }
  .await;

  connection.settle(done)
}

/// How many unread messages a summary names at most: the notice counts every new one but names only the
/// newest.
const SUMMARY_RECENT: usize = 10;

/// One account's inbox at a glance.
pub struct InboxSummary {
  pub unread: u32,
  pub newest: Option<MailSummaryMessage>,
  pub recent_unread: Vec<MailSummaryMessage>,
}

/// One account's inbox at a glance (`docs/mail.md`, 「Reaching the open tabs」): the unread count, the
/// newest message, and the newest unread ones.
pub async fn inbox_summary(session: &MailSession) -> Result<InboxSummary, MailFault> {
  let account = session.account.address.clone();
  let mut connection = session.folder("INBOX").await?;
  let done = async {
    let validity = connection.uid_validity();
    let exists = connection.exists();
    // The newest message is read before the unread ones, so every unread message at or below it is among
    // them. A message arriving between the two reads is then newer than `newest` and in the unread list,
    // never below `newest` and missing from it — which a tab would read as old news.
    let newest_uid: Option<u32> = if exists == 0 {
      None
    } else {
      let found: Vec<Fetch> = connection
        .session()
        .fetch(exists.to_string(), "(UID)")
        .await
        .map_err(MailFault::server)?
        .try_collect()
        .await
        .map_err(MailFault::server)?;

      found.iter().find_map(|fetch| fetch.uid)
    };
    let mut unread = uid_search(connection.session(), "UNSEEN").await?;

    unread.sort_unstable_by(|a, b| b.cmp(a));

    let mut wanted: Vec<u32> = newest_uid.into_iter().collect();

    for uid in unread.iter().take(SUMMARY_RECENT) {
      if !wanted.contains(uid) {
        wanted.push(*uid);
      }
    }

    let rows =
      uid_fetch(connection.session(), &wanted, &format!("(UID FLAGS INTERNALDATE {HEADER_FIELDS})"))
        .await?;
    let mut read: HashMap<u32, MailListMessage> = HashMap::new();

    for fetch in &rows {
      if let Some((row, _, _)) = read_row("INBOX", fetch) {
        read.insert(row.uid, row);
      }
    }

    let summary = |row: &MailListMessage| MailSummaryMessage {
      uid: row.uid,
      key: row.key.clone(),
      from: row.from.clone(),
      subject: row.subject.clone(),
      date: row.date,
      moved_in: moved_in(&account, &validity, row.uid),
    };

    Ok(InboxSummary {
      unread: unread.len() as u32,
      newest: newest_uid.and_then(|uid| read.get(&uid)).map(summary),
      recent_unread: unread
        .iter()
        .take(SUMMARY_RECENT)
        .filter_map(|uid| read.get(uid))
        .map(summary)
        .collect(),
    })
  }
  .await;

  connection.settle(done)
}
