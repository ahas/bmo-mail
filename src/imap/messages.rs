//! Reading and changing the messages of one mailbox (`docs/mail.md`, 「Reading」 and 「Where each piece of
//! state lives」). Nothing here keeps a copy of a message: every page and every thread is read from the
//! server when it is asked for.

use std::collections::{BTreeMap, HashMap, HashSet};

use async_imap::imap_proto::SectionPath;
use async_imap::types::Fetch;
use chrono::{Days, NaiveDate};
use mail_parser::{Address, HeaderName, MessageParser};

use crate::imap::charset::{decode_transfer, part_text};
use crate::imap::conversation::{conversation_key, id_key};
use crate::imap::folders::{folders_of, list_folders};
use crate::imap::moved_in::begin_move_in;
use crate::imap::search::{Search, all_of};
use crate::imap::session::{Connection, MailSession};
use crate::imap::structure::{message_parts, snippet_part, snippet_text};
use crate::imap::{
  Landed, expunge, flag_name, part_number, quoted, run_landing, store_flags, uid_fetch, uid_search,
  uid_set, wire_name,
};
use crate::labels::label_id_of;
use crate::mail::{
  FolderRole, MailAddress, MailFolder, MailFolderState, MailListMessage, MailMessage, MailPart, MailParts,
  MailRef, OpenedPart,
};
use crate::routes::requests::{Asked, MailFault};

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

/// The first window of days a page searches for its messages; each window after it is twice as long.
const FIRST_WINDOW_DAYS: u64 = 7;

/// How far a message's own time zone may stand from UTC. A search's day is read in the message's own zone, so
/// a window of days is trusted only this far inside its first day.
const ZONE_REACH_MS: i64 = 14 * 60 * 60 * 1000;

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
    .header_address(HeaderName::DispositionNotificationTo);
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
      .header(HeaderName::DispositionNotificationTo)
      .and_then(|value| value.as_address())
      .and_then(|address| address.first())
      .and_then(|one| one.address())
      .map(|address| address.trim().to_lowercase()),
  }
}

/// A message without its bodies, from what a fetch answered for it.
fn read_row(folder: &str, fetch: &Fetch) -> Option<MailMessage> {
  let uid = fetch.uid?;
  let flags: Vec<String> = fetch.flags().map(|flag| flag_name(&flag)).collect();
  let heads = heads(fetch.header());
  let date = fetch
    .internal_date()
    .map(|date| date.timestamp_millis())
    .filter(|date| *date != 0)
    .or(heads.date)
    .unwrap_or(0);

  Some(message_of(folder, uid, &flags, heads, message_parts(fetch.bodystructure()), date))
}

/// A message without its bodies, from the flags it carries, its headers, its parts and when it reached its
/// folder.
pub fn message_of(
  folder: &str,
  uid: u32,
  flags: &[String],
  heads: Heads,
  parts: MailParts,
  date: i64,
) -> MailMessage {
  let key = conversation_key(heads.message_id.as_deref(), &heads.in_reply_to, &heads.references)
    .unwrap_or_else(|| format!("{folder}\u{0}{uid}"));
  let has = |name: &str| flags.iter().any(|flag| flag == name);
  let listed = MailListMessage {
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
    flags: flags.iter().filter(|flag| label_id_of(flag).is_none()).cloned().collect(),
    labels: flags.iter().filter_map(|flag| label_id_of(flag).map(str::to_owned)).collect(),
    has_attachments: !parts.attachments.is_empty(),
    snippet: String::new(),
  };

  MailMessage {
    listed,
    cc: heads.cc,
    bcc: heads.bcc,
    reply_to: heads.reply_to,
    in_reply_to: heads.in_reply_to.first().map(|id| id_key(id)),
    references: heads.references.iter().map(|id| id_key(id)).collect(),
    receipt_to: heads.receipt_to,
    text: None,
    html: None,
    attachments: parts.attachments,
    inline: parts.inline,
    text_part: parts.text,
    html_part: parts.html,
  }
}

/// The section a part number names in a fetch's answer.
fn section(part: &str) -> SectionPath {
  SectionPath::Part(part.split('.').filter_map(|number| number.parse().ok()).collect(), None)
}

/// The given messages of the selected folder without their bodies, in the order given, their snippets read in
/// one fetch per part number rather than one per message.
async fn rows(
  connection: &mut Connection,
  folder: &str,
  uids: &[u32],
) -> Result<Vec<MailMessage>, MailFault> {
  let fetched = uid_fetch(connection.session(), uids, &row_items()).await?;
  let mut rows: HashMap<u32, MailMessage> = HashMap::new();
  let mut by_part: BTreeMap<String, Vec<(u32, MailPart)>> = BTreeMap::new();

  for fetch in &fetched {
    let Some(row) = read_row(folder, fetch) else {
      continue;
    };

    if let Some(part) = snippet_part(fetch.bodystructure()) {
      by_part.entry(part.part.clone()).or_default().push((row.listed.uid, part));
    }

    rows.insert(row.listed.uid, row);
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

        row.listed.snippet = snippet_text(&text, part.kind == "text/html");
      }
    }
  }

  Ok(uids.iter().filter_map(|uid| rows.remove(uid)).collect())
}

/// Where a page starts: before a time, after one, or at the newest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cursor {
  Newest,
  Before(i64),
  After(i64),
}

/// One message a page's search found, before its row is read.
#[derive(Clone, Debug)]
struct Hit {
  folder: String,
  uid: u32,
  at: i64,
}

fn day_of(millis: i64) -> NaiveDate {
  chrono::DateTime::from_timestamp_millis(millis).unwrap_or_default().date_naive()
}

fn midnight(day: NaiveDate) -> i64 {
  day.and_hms_opt(0, 0, 0).map_or(0, |at| at.and_utc().timestamp_millis())
}

/// The messages of the selected folder the search finds among `uids`, each with the time it reached the
/// folder, those already seen passed over.
async fn hits_of(
  connection: &mut Connection,
  folder: &str,
  uids: Vec<u32>,
  seen: &mut HashSet<u32>,
) -> Result<Vec<Hit>, MailFault> {
  let fresh: Vec<u32> = uids.into_iter().filter(|uid| seen.insert(*uid)).collect();

  if fresh.is_empty() {
    return Ok(Vec::new());
  }

  let fetched = uid_fetch(connection.session(), &fresh, "(UID INTERNALDATE)").await?;

  Ok(
    fetched
      .iter()
      .filter_map(|fetch| {
        Some(Hit {
          folder: folder.to_owned(),
          uid: fetch.uid?,
          at: fetch.internal_date().map_or(0, |date| date.timestamp_millis()),
        })
      })
      .collect(),
  )
}

/// The messages of one folder the search finds older than `before` (or the newest, with none), read a window
/// of days at a time back from it until `wanted` of them stand where the window can be trusted, or the folder
/// holds nothing older. The second answer says whether the folder was read to its oldest message.
async fn older_hits(
  connection: &mut Connection,
  folder: &str,
  search: &Search,
  before: Option<i64>,
  wanted: usize,
) -> Result<(Vec<Hit>, bool), MailFault> {
  // A search's day is the message's own, so the top of the window stands a day past the cursor's day.
  let top = before.map_or_else(|| chrono::Utc::now().date_naive(), day_of) + Days::new(2);
  let mut span = FIRST_WINDOW_DAYS;
  let mut seen = HashSet::new();
  let mut hits: Vec<Hit> = Vec::new();

  loop {
    let since = top.checked_sub_days(Days::new(span)).unwrap_or(NaiveDate::MIN);
    let found = uid_search(
      connection.session(),
      &all_of(vec![search.clone(), Search::Since(since), Search::Before(top)]).command(),
    )
    .await?;

    for hit in hits_of(connection, folder, found, &mut seen).await? {
      if before.is_none_or(|before| hit.at < before) {
        hits.push(hit);
      }
    }

    let trusted_from = midnight(since) + ZONE_REACH_MS;

    if hits.iter().filter(|hit| hit.at >= trusted_from).count() >= wanted {
      return Ok((hits, false));
    }

    let older =
      uid_search(connection.session(), &all_of(vec![search.clone(), Search::Before(since)]).command())
        .await?;

    if older.is_empty() {
      return Ok((hits, true));
    }

    span = span.saturating_mul(2);
  }
}

/// The messages of one folder the search finds newer than `after`.
async fn newer_hits(
  connection: &mut Connection,
  folder: &str,
  search: &Search,
  after: i64,
) -> Result<Vec<Hit>, MailFault> {
  let since = day_of(after).checked_sub_days(Days::new(1)).unwrap_or(NaiveDate::MIN);
  let found =
    uid_search(connection.session(), &all_of(vec![search.clone(), Search::Since(since)]).command()).await?;
  let hits = hits_of(connection, folder, found, &mut HashSet::new()).await?;

  Ok(hits.into_iter().filter(|hit| hit.at > after).collect())
}

/// One page of the messages the search finds across the folders, newest first, and whether more stand past
/// it. A page never ends inside a run of messages of one time, so the next page's cursor is exact.
pub async fn messages_page(
  session: &MailSession,
  folders: &[MailFolder],
  search: &Search,
  cursor: Cursor,
  limit: usize,
) -> Result<(Vec<MailMessage>, bool), MailFault> {
  let mut hits: Vec<Hit> = Vec::new();
  let mut every_folder_read = true;

  for folder in folders {
    let mut connection = session.folder(&folder.path).await?;
    let done = async {
      match cursor {
        Cursor::After(after) => Ok((newer_hits(&mut connection, &folder.path, search, after).await?, true)),
        Cursor::Before(before) => {
          older_hits(&mut connection, &folder.path, search, Some(before), limit + 1).await
        }
        Cursor::Newest => older_hits(&mut connection, &folder.path, search, None, limit + 1).await,
      }
    }
    .await;
    let (found, read_whole) = connection.settle(done)?;

    every_folder_read &= read_whole;
    hits.extend(found);
  }

  let newest_first =
    |a: &Hit, b: &Hit| b.at.cmp(&a.at).then_with(|| a.folder.cmp(&b.folder)).then(b.uid.cmp(&a.uid));
  let page: Vec<Hit> = match cursor {
    Cursor::After(_) => {
      hits.sort_by(|a, b| newest_first(b, a));

      let mut page = through_run(&hits, limit);

      every_folder_read = page.len() == hits.len();
      page.sort_by(newest_first);
      page
    }
    _ => {
      hits.sort_by(newest_first);

      let page = through_run(&hits, limit);

      every_folder_read &= page.len() == hits.len();
      page
    }
  };
  let mut read: HashMap<(String, u32), MailMessage> = HashMap::new();
  let mut folders_in_page: Vec<&str> = Vec::new();

  for hit in &page {
    if !folders_in_page.contains(&hit.folder.as_str()) {
      folders_in_page.push(&hit.folder);
    }
  }

  for folder in folders_in_page {
    let uids: Vec<u32> = page.iter().filter(|hit| hit.folder == folder).map(|hit| hit.uid).collect();
    let mut connection = session.folder(folder).await?;
    let done = rows(&mut connection, folder, &uids).await;

    for row in connection.settle(done)? {
      read.insert((row.listed.folder.clone(), row.listed.uid), row);
    }
  }

  let ordered = page.iter().filter_map(|hit| read.remove(&(hit.folder.clone(), hit.uid))).collect();

  Ok((ordered, !every_folder_read))
}

/// The first `limit` hits, and every one after them of the same time as the last of them.
fn through_run(hits: &[Hit], limit: usize) -> Vec<Hit> {
  let mut end = limit.min(hits.len());

  while end > 0 && end < hits.len() && hits[end].at == hits[end - 1].at {
    end += 1;
  }

  hits[..end].to_vec()
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
  let Some(mut message) = fetched.first().and_then(|fetch| read_row(folder, fetch)) else {
    return Ok(None);
  };

  message.text = read_body(connection, uid, message.text_part.as_ref()).await?;
  message.html = read_body(connection, uid, message.html_part.as_ref()).await?;
  message.listed.snippet = match (&message.text, &message.html) {
    (Some(text), _) => snippet_text(text, false),
    (None, Some(html)) => snippet_text(html, true),
    (None, None) => String::new(),
  };

  Ok(Some(message))
}

/// One message with its bodies. Reading it does not mark it read.
pub async fn read_message(session: &MailSession, at: &MailRef) -> Result<Option<MailMessage>, MailFault> {
  let mut connection = session.folder(&at.folder).await?;
  let done = read_in_folder(&mut connection, &at.folder, at.uid).await;

  connection.settle(done)
}

/// One message without its bodies, as a page and an event carry it.
pub async fn read_row_of(session: &MailSession, at: &MailRef) -> Result<Option<MailMessage>, MailFault> {
  let mut connection = session.folder(&at.folder).await?;
  let done = rows(&mut connection, &at.folder, &[at.uid]).await;

  Ok(connection.settle(done)?.into_iter().next())
}

/// Every message of a conversation: those the conversation's key names, read from the inbox, the sent
/// folders and the folder it was opened from, oldest first, a message found in two folders once.
pub async fn read_thread(
  session: &MailSession,
  key: &str,
  opened_in: Option<&str>,
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
    .chain(opened_in.map(str::to_owned))
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

  found.sort_by_key(|message| message.listed.date);

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

/// A part of a message as it comes from the server, whole; none where the message or the part is not there.
pub async fn open_part(
  session: &MailSession,
  at: &MailRef,
  part: &str,
) -> Result<Option<OpenedPart>, MailFault> {
  if !part_number(part) {
    return Ok(None);
  }

  let mut connection = session.folder(&at.folder).await?;
  let done = async {
    let fetched =
      uid_fetch(connection.session(), &[at.uid], &format!("(UID BODYSTRUCTURE BODY.PEEK[{part}])")).await?;
    let Some(fetch) = fetched.first() else {
      return Ok(None);
    };
    let meta = leaves(fetch).into_iter().find(|one| one.part == part);
    let Some(bytes) = fetch.section(&section(part)) else {
      return Ok(None);
    };

    Ok(Some(OpenedPart {
      bytes: decode_transfer(bytes, meta.as_ref().and_then(|meta| meta.encoding.as_deref())),
      kind: meta.as_ref().map(|meta| meta.kind.clone()),
      filename: meta.and_then(|meta| meta.filename),
    }))
  }
  .await;

  connection.settle(done)
}

/// The part of a message a content id names.
pub async fn find_part_by_cid(
  session: &MailSession,
  at: &MailRef,
  cid: &str,
) -> Result<Option<MailPart>, MailFault> {
  let wanted = cid.trim_start_matches('<').trim_end_matches('>');
  let mut connection = session.folder(&at.folder).await?;
  let done = uid_fetch(connection.session(), &[at.uid], "(UID BODYSTRUCTURE)").await;
  let fetched = connection.settle(done)?;

  Ok(fetched.first().and_then(|fetch| {
    leaves(fetch)
      .into_iter()
      .find(|part| part.cid.as_deref().is_some_and(|cid| cid.eq_ignore_ascii_case(wanted)))
  }))
}

/// Whether a request may set or clear the flag.
pub fn settable_flag(flag: &str) -> bool {
  SETTABLE.contains(&flag) || label_id_of(flag).is_some()
}

/// Sets and clears flags on one message.
pub async fn change_flags(
  session: &MailSession,
  at: &MailRef,
  add: &[String],
  remove: &[String],
) -> Result<(), MailFault> {
  let mut connection = session.folder(&at.folder).await?;
  let done = async {
    store_flags(connection.session(), &[at.uid], "+", add).await?;
    store_flags(connection.session(), &[at.uid], "-", remove).await
  }
  .await;

  connection.settle(done)
}

/// Gives a message the whole of the settable flags and label keywords `wanted` names: what it carries beyond
/// them is taken off, what it lacks is set, and a flag no request may set is left as it is. A flag outside
/// the settable ones is refused unless the message carries it already. Answers whether the message is there.
pub async fn set_flags(session: &MailSession, at: &MailRef, wanted: &[String]) -> Result<bool, MailFault> {
  let mut connection = session.folder(&at.folder).await?;
  let done = async {
    let found = uid_fetch(connection.session(), &[at.uid], "(UID FLAGS)").await?;
    let Some(fetch) = found.first() else {
      return Ok(false);
    };
    let held: Vec<String> = fetch.flags().map(|flag| flag_name(&flag)).collect();

    if let Some(flag) = wanted.iter().find(|flag| !settable_flag(flag) && !held.contains(flag)) {
      return Err(
        Asked::rule("flag-not-settable", format!("{flag} is not a flag a request may set")).into(),
      );
    }

    let add: Vec<String> = wanted
      .iter()
      .filter(|flag| settable_flag(flag) && !held.contains(flag))
      .cloned()
      .collect();
    let remove: Vec<String> = held
      .iter()
      .filter(|flag| settable_flag(flag) && !wanted.contains(flag))
      .cloned()
      .collect();

    store_flags(connection.session(), &[at.uid], "+", &add).await?;
    store_flags(connection.session(), &[at.uid], "-", &remove).await?;

    Ok(true)
  }
  .await;

  connection.settle(done)
}

/// Moves messages of the selected folder into another, and says where they landed where the server said. A
/// server without `MOVE` has it done by copy, flag and expunge.
pub async fn move_uids(
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

/// Moves one message into another folder, and answers where it now stands: where the server said it landed,
/// else where its `Message-ID` finds it there, else nowhere known. A move into the inbox is noted with where
/// the message landed, so the watcher does not take it for new mail.
pub async fn move_message(
  session: &MailSession,
  at: &MailRef,
  message_id: Option<&str>,
  destination: &str,
) -> Result<Option<MailRef>, MailFault> {
  let mut noted = (destination == "INBOX").then(|| begin_move_in(&session.account.address));
  let mut connection = session.folder(&at.folder).await?;
  let done = move_uids(&mut connection, &[at.uid], destination).await;
  let landed = connection.settle(done)?;
  let uid = landed.as_ref().and_then(|landed| landed.uids.first().copied());

  if let Some(noted) = noted.as_mut() {
    noted.landed(landed);
  }

  if let Some(uid) = uid {
    return Ok(Some(MailRef {
      folder: destination.to_owned(),
      uid,
    }));
  }

  let Some(message_id) = message_id else {
    return Ok(None);
  };
  let mut connection = session.folder(destination).await?;
  let search = Search::Header("message-id".to_owned(), message_id.to_owned());
  let done = uid_search(connection.session(), &search.command()).await;
  let found = connection.settle(done)?;

  Ok(found.into_iter().max().map(|uid| MailRef {
    folder: destination.to_owned(),
    uid,
  }))
}

/// Deletion for good, which only the junk and the trash folders allow.
pub async fn delete_forever(session: &MailSession, at: &MailRef) -> Result<(), MailFault> {
  let folders = folders_of(session).await?;
  let role = folders.iter().find(|one| one.path == at.folder).and_then(|one| one.role);

  if role != Some(FolderRole::Junk) && role != Some(FolderRole::Trash) {
    return Err(Asked::rule("outside-trash", "a message to delete is not in spam or trash").into());
  }

  let mut connection = session.folder(&at.folder).await?;
  let done = async {
    let found = uid_fetch(connection.session(), &[at.uid], "(UID)").await?;

    if found.is_empty() {
      return Err(Asked::not_found("no message has that id").into());
    }

    let uidplus = connection.session().capabilities().await.map_err(MailFault::server)?.has_str("UIDPLUS");

    store_flags(connection.session(), &[at.uid], "+", &["\\Deleted".to_owned()]).await?;
    expunge(connection.session(), &[at.uid], uidplus).await
  }
  .await;

  connection.settle(done)
}

/// Marks every message of a folder read, or every one unread.
pub async fn mark_folder(session: &MailSession, path: &str, read: bool) -> Result<(), MailFault> {
  let mut connection = session.folder(path).await?;
  let done = async {
    let uids = uid_search(connection.session(), if read { "UNSEEN" } else { "SEEN" }).await?;

    store_flags(connection.session(), &uids, if read { "+" } else { "-" }, &["\\Seen".to_owned()]).await
  }
  .await;

  connection.settle(done)
}

/// Every folder of the mailbox with its unread and total counts.
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

/// One folder with its counts, and its place among the folders; none where the mailbox holds no folder of
/// that name.
pub async fn folder_state(
  session: &MailSession,
  path: &str,
) -> Result<Option<(usize, MailFolderState)>, MailFault> {
  let mut connection = session.connection().await?;
  let done = async {
    let folders = list_folders(&mut connection).await?;
    let Some((index, folder)) = folders.into_iter().enumerate().find(|(_, folder)| folder.path == path)
    else {
      return Ok(None);
    };
    let status = connection
      .session()
      .status(wire_name(&folder.path), "(MESSAGES UNSEEN)")
      .await
      .map_err(MailFault::server)?;

    Ok(Some((
      index,
      MailFolderState {
        folder,
        unseen: status.unseen.unwrap_or(0),
        total: status.exists,
      },
    )))
  }
  .await;

  connection.settle(done)
}

/// The unread messages of the inbox.
pub async fn inbox_unseen(session: &MailSession) -> Result<u32, MailFault> {
  let mut connection = session.connection().await?;
  let done = async {
    let status = connection
      .session()
      .status(wire_name("INBOX"), "(UNSEEN)")
      .await
      .map_err(MailFault::server)?;

    Ok(status.unseen.unwrap_or(0))
  }
  .await;

  connection.settle(done)
}
