//! The POP3/SMTP adapter (`docs/mail.md`, 「Accounts and credentials」, 「A POP3 account」): an account of the
//! kind `pop3-smtp` is a mail server reached over POP3 and submission, signed in with a login and a password. Its
//! mailbox holds two folders: the inbox, which is the maildrop read one session at a time (`crate::pop3`), and a
//! drafts folder of the service's own in the store. Flags and labels are the service's own, kept on each row;
//! mail never moves between the folders, and writing goes over the same submission as any account's.

use std::path::PathBuf;
use std::sync::Arc;

use futures::future::BoxFuture;
use sqlx::SqlitePool;

use crate::accounts::Account;
use crate::adapters::servers::{LOGIN, MailEndpoint, PASSWORD, field, refused_sign_in, servers_of};
use crate::adapters::{Adapter, Context, Mailbox, Pending, Watch};
use crate::imap::messages::{Cursor, heads, message_of, settable_flag};
use crate::imap::part_number;
use crate::imap::views::{page_cursor, page_scope};
use crate::labels::label_id_of;
use crate::mail::{
  Draft, FolderRole, MailAddress, MailFolder, MailFolderState, MailMessage, MailRef, OpenedPart,
};
use crate::members::bare_id;
use crate::pop3::drafts::{
  DraftRow, draft_message, draft_row, draft_row_named, draft_rows, forget_draft, keep_draft,
  keep_draft_flags,
};
use crate::pop3::maildrop::{Maildrop, Row, keep_flags, row, rows};
use crate::pop3::mime::{open_part, part_by_cid, read_whole};
use crate::pop3::search::Judged;
use crate::pop3::session::open;
use crate::pop3::watcher::Pop3Watcher;
use crate::pop3::{DRAFTS, INBOX};
use crate::routes::downloads::Disposition;
use crate::routes::requests::{Asked, MailFault};
use crate::rules::{RulesRun, message_facts, plan_rules, rule_context};
use crate::wire::{AccountKind, FieldKind, KindServers, MessageQuery, Protocol, SignIn};
use crate::wording::Wording;
use crate::writing::compose::{Purpose, build_message, new_message_id};
use crate::writing::drafts::{PartSource, draft_of, resolve_attachments};
use crate::writing::receipts::build_receipt;
use crate::writing::submission::{self, SendingLimit, original_mark, outgoing, submit};

/// The kind's name.
pub const KIND: &str = "pop3-smtp";

/// The refusal of what moves mail on a POP3 account.
fn not_offered() -> MailFault {
  Asked::rule("not-offered", "mail on a POP3 account never moves between folders").into()
}

/// The adapter.
pub struct Pop3Smtp;

impl Adapter for Pop3Smtp {
  fn kind(&self) -> AccountKind {
    AccountKind {
      name: KIND.to_owned(),
      sign_in: SignIn::Password,
      moves: false,
      servers: Some(KindServers {
        incoming: Protocol::Pop3,
        outgoing: Protocol::Smtp,
      }),
      fields: vec![
        field(LOGIN, FieldKind::Text, false, None),
        field(PASSWORD, FieldKind::Secret, true, None),
      ],
    }
  }

  fn sign_in<'a>(&'a self, account: &'a Account) -> Pending<'a, ()> {
    Box::pin(async move {
      let refused = |why: MailFault| refused_sign_in(&account.address, why);
      let (pop3, smtp, login) = servers_of(account).map_err(refused)?;
      let mut session = open(&pop3, &login).await.map_err(refused)?;
      let listed = session.listing().await;

      session.quit().await;
      listed.map_err(refused)?;
      submission::sign_in(&smtp, &login).await.map_err(refused)
    })
  }

  fn open(&self, account: &Account, context: &Context) -> Result<Arc<dyn Mailbox>, MailFault> {
    let (pop3, smtp, login) = servers_of(account)?;

    Ok(Arc::new(Pop3SmtpMailbox {
      maildrop: Arc::new(Maildrop::new(pop3, login, context.pool.clone(), account.store_key())),
      smtp,
      staging: context.staging.clone(),
      limit: SendingLimit::default(),
    }))
  }
}

crate::adapter!(Pop3Smtp);

/// A `pop3-smtp` account's mailbox: its maildrop, its submission server and the folder posted files wait in.
pub struct Pop3SmtpMailbox {
  maildrop: Arc<Maildrop>,
  smtp: MailEndpoint,
  staging: PathBuf,
  limit: SendingLimit,
}

/// The two folders, in the order the nav lists them.
fn folders() -> Vec<MailFolder> {
  vec![
    MailFolder {
      path: INBOX.to_owned(),
      name: INBOX.to_owned(),
      role: Some(FolderRole::Inbox),
    },
    MailFolder {
      path: DRAFTS.to_owned(),
      name: DRAFTS.to_owned(),
      role: Some(FolderRole::Drafts),
    },
  ]
}

/// A message of the inbox as a list shows it, from what its row keeps.
fn listed(row: &Row) -> MailMessage {
  let mut message = message_of(
    INBOX,
    row.id,
    &row.flags,
    heads(Some(&row.head)),
    row.parts.clone().unwrap_or_default(),
    row.at,
  );

  message.listed.snippet = row.snippet.clone().unwrap_or_default();
  message
}

/// The flags a message ends up carrying when a request names the whole of the settable ones it should: what it
/// carries that no request may set stays, and a flag outside the settable ones is refused unless it is held.
fn flags_wanted(held: &[String], wanted: &[String]) -> Result<Vec<String>, MailFault> {
  if let Some(flag) = wanted.iter().find(|flag| !settable_flag(flag) && !held.contains(flag)) {
    return Err(Asked::rule("flag-not-settable", format!("{flag} is not a flag a request may set")).into());
  }

  let mut kept: Vec<String> = held.iter().filter(|flag| !settable_flag(flag)).cloned().collect();

  for flag in wanted.iter().filter(|flag| settable_flag(flag)) {
    if !kept.contains(flag) {
      kept.push(flag.clone());
    }
  }

  Ok(kept)
}

/// The first `limit` messages, and every one after them of the same time as the last of them.
fn through_run(sorted: &mut Vec<MailMessage>, limit: usize) -> bool {
  let mut end = limit.min(sorted.len());

  while end > 0 && end < sorted.len() && sorted[end].listed.date == sorted[end - 1].listed.date {
    end += 1;
  }

  let more = end < sorted.len();

  sorted.truncate(end);
  more
}

/// A page cut out of the messages a view found, newest first, and whether more stand past it.
fn page_cut(mut found: Vec<MailMessage>, cursor: Cursor, limit: usize) -> (Vec<MailMessage>, bool) {
  let newest_first = |a: &MailMessage, b: &MailMessage| {
    b.listed
      .date
      .cmp(&a.listed.date)
      .then_with(|| a.listed.folder.cmp(&b.listed.folder))
      .then(b.listed.uid.cmp(&a.listed.uid))
  };

  match cursor {
    Cursor::Newest => {}
    Cursor::Before(before) => found.retain(|one| one.listed.date < before),
    Cursor::After(after) => found.retain(|one| one.listed.date > after),
  }

  if let Cursor::After(_) = cursor {
    found.sort_by(|a, b| newest_first(b, a));

    let more = through_run(&mut found, limit);

    found.sort_by(newest_first);

    return (found, more);
  }

  found.sort_by(newest_first);

  let more = through_run(&mut found, limit);

  (found, more)
}

impl Pop3SmtpMailbox {
  fn pool(&self) -> &SqlitePool {
    &self.maildrop.pool
  }

  fn key(&self) -> &str {
    &self.maildrop.key
  }

  async fn draft_at(&self, at: &MailRef) -> Result<Option<DraftRow>, MailFault> {
    if at.folder != DRAFTS {
      return Ok(None);
    }

    draft_row(self.pool(), self.key(), at.uid).await
  }

  /// The whole message at a place: the maildrop's, read from the server, or a draft's, from the store.
  async fn raw(&self, at: &MailRef) -> Result<Option<Vec<u8>>, MailFault> {
    match at.folder.as_str() {
      INBOX => Ok(self.maildrop.whole(at.uid).await?.map(|(_, raw)| raw)),
      DRAFTS => Ok(self.draft_at(at).await?.map(|draft| draft.message)),
      _ => Ok(None),
    }
  }

  /// The flags a message at a place carries, or none where it is not there.
  async fn flags_at(&self, at: &MailRef) -> Result<Option<Vec<String>>, MailFault> {
    match at.folder.as_str() {
      INBOX => Ok(row(self.pool(), self.key(), at.uid).await?.map(|row| row.flags)),
      DRAFTS => Ok(self.draft_at(at).await?.map(|draft| draft.flags)),
      _ => Ok(None),
    }
  }

  async fn keep_flags_at(&self, at: &MailRef, flags: &[String]) -> Result<(), MailFault> {
    match at.folder.as_str() {
      DRAFTS => keep_draft_flags(self.pool(), self.key(), at.uid, flags).await,
      _ => keep_flags(self.pool(), self.key(), at.uid, flags).await,
    }
  }

  /// A folder with its counts, read from the store.
  async fn state_of(&self, folder: MailFolder) -> Result<MailFolderState, MailFault> {
    let flags: Vec<Vec<String>> = match folder.path.as_str() {
      INBOX => rows(self.pool(), self.key()).await?.into_iter().map(|row| row.flags).collect(),
      _ => draft_rows(self.pool(), self.key())
        .await?
        .into_iter()
        .map(|draft| draft.flags)
        .collect(),
    };
    let unseen = flags.iter().filter(|flags| !flags.iter().any(|flag| flag == "\\Seen")).count();

    Ok(MailFolderState {
      folder,
      unseen: u32::try_from(unseen).unwrap_or(u32::MAX),
      total: u32::try_from(flags.len()).unwrap_or(u32::MAX),
    })
  }

  /// Every message of a folder as a list shows it, each with what a search judges it by.
  async fn listed_in(&self, folder: &str) -> Result<Vec<(MailMessage, Vec<u8>, Vec<String>)>, MailFault> {
    Ok(match folder {
      INBOX => rows(self.pool(), self.key())
        .await?
        .into_iter()
        .map(|row| (listed(&row), row.head, row.flags))
        .collect(),
      DRAFTS => draft_rows(self.pool(), self.key())
        .await?
        .into_iter()
        .map(|draft| (draft_message(&draft, false), draft.message, draft.flags))
        .collect(),
      _ => Vec::new(),
    })
  }

  async fn page_of(&self, query: &MessageQuery) -> Result<(Vec<MailMessage>, bool), MailFault> {
    let (cursor, limit) = page_cursor(query)?;

    self.maildrop.fresh().await?;

    let Some(scope) = page_scope(self.pool(), &folders(), self.key(), query).await? else {
      return Ok((Vec::new(), false));
    };
    let mut found = Vec::new();

    for folder in &scope.folders {
      for (message, head, flags) in self.listed_in(&folder.path).await? {
        let read = heads(Some(&head));
        let facts = message_facts(Some(&head));
        let judged = Judged {
          heads: &read,
          facts: &facts,
          flags: &flags,
          at: message.listed.date,
        };

        if judged.meets(&scope.search) {
          found.push(message);
        }
      }
    }

    Ok(page_cut(found, cursor, limit))
  }

  async fn message_at(&self, at: &MailRef) -> Result<Option<MailMessage>, MailFault> {
    if at.folder == DRAFTS {
      return Ok(self.draft_at(at).await?.map(|draft| draft_message(&draft, true)));
    }

    if at.folder != INBOX {
      return Ok(None);
    }

    let Some((row, raw)) = self.maildrop.whole(at.uid).await? else {
      return Ok(None);
    };

    Ok(Some(whole_message(&row, &raw)))
  }

  async fn thread_of(&self, key: &str, folder: Option<&str>) -> Result<Vec<MailMessage>, MailFault> {
    if let Some((folder, uid)) = key.rsplit_once('\u{0}')
      && let Ok(uid) = uid.parse::<u32>()
    {
      let at = MailRef {
        folder: folder.to_owned(),
        uid,
      };

      return Ok(self.message_at(&at).await?.into_iter().collect());
    }

    let mut found: Vec<MailMessage> = Vec::new();
    let ids: Vec<u32> = rows(self.pool(), self.key())
      .await?
      .iter()
      .filter(|row| listed(row).listed.key == key)
      .map(|row| row.id)
      .collect();

    if !ids.is_empty() {
      let mut turn = self.maildrop.turn().await?;

      for id in ids {
        match turn.whole(id).await {
          Ok(Some((row, Some(raw)))) => found.push(whole_message(&row, &raw)),
          // A message past what the POP3 client reads stands in its conversation as its row does.
          Ok(Some((row, None))) => found.push(listed(&row)),
          Ok(None) => {}
          Err(why) => {
            turn.done().await;
            return Err(why);
          }
        }
      }

      turn.done().await;
    }

    if folder == Some(DRAFTS) {
      found.extend(
        draft_rows(self.pool(), self.key())
          .await?
          .iter()
          .map(|draft| draft_message(draft, true))
          .filter(|message| message.listed.key == key),
      );
    }

    found.sort_by_key(|message| message.listed.date);

    let mut seen = std::collections::HashSet::new();

    Ok(
      found
        .into_iter()
        .filter(|message| message.listed.message_id.as_ref().is_none_or(|id| seen.insert(id.clone())))
        .collect(),
    )
  }

  async fn part_at(
    &self,
    at: &MailRef,
    part: &str,
  ) -> Result<Option<(OpenedPart, Disposition)>, MailFault> {
    let Some(raw) = self.raw(at).await? else {
      return Ok(None);
    };

    if part_number(part) {
      return Ok(open_part(&raw, part).map(|opened| (opened, Disposition::Attachment)));
    }

    let Some(found) = part_by_cid(&raw, part) else {
      return Ok(None);
    };

    Ok(open_part(&raw, &found.part).map(|opened| (opened, Disposition::Inline)))
  }

  async fn set_flags_at(&self, at: &MailRef, wanted: &[String]) -> Result<bool, MailFault> {
    let Some(held) = self.flags_at(at).await? else {
      return Ok(false);
    };

    self.keep_flags_at(at, &flags_wanted(&held, wanted)?).await?;

    Ok(true)
  }

  async fn mark_folder_at(&self, path: &str, read: bool) -> Result<(), MailFault> {
    let seen = "\\Seen".to_owned();
    let marked = |flags: &[String]| {
      let mut flags: Vec<String> = flags.iter().filter(|flag| **flag != seen).cloned().collect();

      if read {
        flags.push(seen.clone());
      }

      flags
    };

    match path {
      INBOX => {
        for row in rows(self.pool(), self.key()).await? {
          keep_flags(self.pool(), self.key(), row.id, &marked(&row.flags)).await?;
        }
      }
      DRAFTS => {
        for draft in draft_rows(self.pool(), self.key()).await? {
          keep_draft_flags(self.pool(), self.key(), draft.id, &marked(&draft.flags)).await?;
        }
      }
      _ => return Err(Asked::not_found("the mailbox holds no folder of that name").into()),
    }

    Ok(())
  }

  async fn drop_label_of(&self, label_id: &str) -> Result<(), MailFault> {
    let without = |flags: &[String]| -> Vec<String> {
      flags.iter().filter(|flag| label_id_of(flag) != Some(label_id)).cloned().collect()
    };

    for row in rows(self.pool(), self.key()).await? {
      keep_flags(self.pool(), self.key(), row.id, &without(&row.flags)).await?;
    }

    for draft in draft_rows(self.pool(), self.key()).await? {
      keep_draft_flags(self.pool(), self.key(), draft.id, &without(&draft.flags)).await?;
    }

    Ok(())
  }

  /// The rules run over the inbox: their label, star and read actions carried out on each row, their moves passed
  /// over.
  async fn run_rules_on(&self, folder: &str) -> Result<RulesRun, MailFault> {
    let mut run = RulesRun::default();

    if folder != INBOX {
      return Ok(run);
    }

    let (rules, spam_senders) = rule_context(self.pool(), self.key()).await?;

    for row in rows(self.pool(), self.key()).await? {
      let plan = plan_rules(&rules, &spam_senders, &message_facts(Some(&row.head)));
      let added: Vec<String> = plan.flags.into_iter().filter(|flag| !row.flags.contains(flag)).collect();

      if added.is_empty() {
        continue;
      }

      if added.iter().any(|flag| label_id_of(flag).is_some()) {
        run.labelled += 1;
      }

      let mut flags = row.flags.clone();

      flags.extend(added);
      keep_flags(self.pool(), self.key(), row.id, &flags).await?;
    }

    Ok(run)
  }

  async fn draft_named(&self, draft_id: &str) -> Result<Draft, MailFault> {
    let unknown = || MailFault::from(Asked::not_found("no draft has that id"));
    let found = draft_row_named(self.pool(), self.key(), &bare_id(draft_id))
      .await?
      .ok_or_else(unknown)?;
    let at = MailRef {
      folder: DRAFTS.to_owned(),
      uid: found.id,
    };

    draft_of(&found.message, &draft_message(&found, false), &at, &self.maildrop.login.address)
      .ok_or_else(unknown)
  }

  /// Saves a draft under the `Message-ID` it keeps, its attachments read from where they stand before the kept
  /// message is replaced, and opens it again as it now stands.
  async fn save(&self, draft: &Draft) -> Result<Draft, MailFault> {
    let message_id =
      draft.message_id.clone().unwrap_or_else(|| format!("<{}>", new_message_id(&draft.from)));
    let attachments = resolve_attachments(self, &self.staging, &draft.attachments).await?;
    let now = chrono::Utc::now();
    let raw = build_message(draft, &attachments, Purpose::Draft, Some(&message_id), now.timestamp())
      .map_err(MailFault::server)?;

    keep_draft(self.pool(), self.key(), &bare_id(&message_id), &raw, now.timestamp_millis()).await?;

    self.draft_named(&message_id).await
  }

  async fn discard(&self, draft_id: &str) -> Result<(), MailFault> {
    let Some(found) = draft_row_named(self.pool(), self.key(), &bare_id(draft_id)).await? else {
      return Err(Asked::not_found("no draft has that id").into());
    };

    forget_draft(self.pool(), self.key(), found.id).await
  }

  /// Sends a draft over submission, forgets it, and marks the message it answered or carried. No copy is kept.
  async fn send(&self, draft_id: &str) -> Result<Vec<MailAddress>, MailFault> {
    let draft = self.draft_named(draft_id).await?;
    let at = draft.saved.clone().ok_or_else(|| MailFault::server("the draft stands nowhere"))?;
    let limit = self.limit.of(&self.smtp).await;
    let going = outgoing(self, &self.staging, limit, &draft).await?;

    submit(&self.smtp, &self.maildrop.login, &draft.from, &going.envelope(), &going.raw).await?;
    forget_draft(self.pool(), self.key(), at.uid).await?;

    if let Some((original, flag)) = original_mark(&draft) {
      let original = bare_id(original);

      for row in rows(self.pool(), self.key()).await? {
        if heads(Some(&row.head)).message_id.is_some_and(|id| bare_id(&id) == original)
          && !row.flags.iter().any(|held| held == flag)
        {
          let mut flags = row.flags.clone();

          flags.push(flag.to_owned());
          keep_flags(self.pool(), self.key(), row.id, &flags).await?;
        }
      }
    }

    Ok(going.recipients)
  }

  async fn receipt(&self, at: &MailRef, wording: &Wording) -> Result<(), MailFault> {
    let Some(message) = self.message_at(at).await? else {
      return Err(Asked::not_found("no message has that id").into());
    };
    let Some(to) = message.receipt_to.clone() else {
      return Err(Asked::rule("no-receipt-asked", "the message asks for no read receipt").into());
    };
    let account = &self.maildrop.login.address;
    let raw = build_receipt(&message, account, wording, chrono::Utc::now().timestamp())?;

    submit(&self.smtp, &self.maildrop.login, account, &[to], &raw).await?;

    let mut flags = self.flags_at(at).await?.unwrap_or_default();

    if !flags.iter().any(|flag| flag == "$MDNSent") {
      flags.push("$MDNSent".to_owned());
    }

    self.keep_flags_at(at, &flags).await
  }
}

/// A message of the inbox with its bodies, from its row and its whole bytes.
fn whole_message(row: &Row, raw: &[u8]) -> MailMessage {
  let whole = read_whole(raw);
  let mut message = message_of(INBOX, row.id, &row.flags, heads(Some(&row.head)), whole.parts, row.at);

  message.listed.snippet = whole.snippet;
  message.text = whole.text;
  message.html = whole.html;
  message
}

impl PartSource for Pop3SmtpMailbox {
  fn open_part<'a>(&'a self, at: &'a MailRef, part: &'a str) -> Pending<'a, Option<OpenedPart>> {
    Box::pin(async move { Ok(self.raw(at).await?.and_then(|raw| open_part(&raw, part))) })
  }
}

impl Mailbox for Pop3SmtpMailbox {
  fn inbox_unread(&self) -> Pending<'_, u32> {
    Box::pin(async move {
      self.maildrop.fresh().await?;

      Ok(self.state_of(folders().remove(0)).await?.unseen)
    })
  }

  fn folders(&self) -> Pending<'_, Vec<MailFolder>> {
    Box::pin(async { Ok(folders()) })
  }

  fn folder_states(&self) -> Pending<'_, Vec<MailFolderState>> {
    Box::pin(async move {
      self.maildrop.fresh().await?;

      let mut states = Vec::new();

      for folder in folders() {
        states.push(self.state_of(folder).await?);
      }

      Ok(states)
    })
  }

  fn folder_state<'a>(&'a self, path: &'a str) -> Pending<'a, Option<(usize, MailFolderState)>> {
    Box::pin(async move {
      let Some((index, folder)) = folders().into_iter().enumerate().find(|(_, folder)| folder.path == path)
      else {
        return Ok(None);
      };

      self.maildrop.fresh().await?;

      Ok(Some((index, self.state_of(folder).await?)))
    })
  }

  fn mark_folder<'a>(&'a self, path: &'a str, read: bool) -> Pending<'a, ()> {
    Box::pin(self.mark_folder_at(path, read))
  }

  fn page<'a>(&'a self, query: &'a MessageQuery) -> Pending<'a, (Vec<MailMessage>, bool)> {
    Box::pin(self.page_of(query))
  }

  fn message<'a>(&'a self, at: &'a MailRef) -> Pending<'a, Option<MailMessage>> {
    Box::pin(self.message_at(at))
  }

  fn row<'a>(&'a self, at: &'a MailRef) -> Pending<'a, Option<MailMessage>> {
    Box::pin(async move {
      match at.folder.as_str() {
        INBOX => Ok(row(self.pool(), self.key(), at.uid).await?.map(|row| listed(&row))),
        DRAFTS => Ok(self.draft_at(at).await?.map(|draft| draft_message(&draft, false))),
        _ => Ok(None),
      }
    })
  }

  fn thread<'a>(&'a self, key: &'a str, folder: Option<&'a str>) -> Pending<'a, Vec<MailMessage>> {
    Box::pin(self.thread_of(key, folder))
  }

  fn part<'a>(&'a self, at: &'a MailRef, part: &'a str) -> Pending<'a, Option<(OpenedPart, Disposition)>> {
    Box::pin(self.part_at(at, part))
  }

  fn set_flags<'a>(&'a self, at: &'a MailRef, wanted: &'a [String]) -> Pending<'a, bool> {
    Box::pin(self.set_flags_at(at, wanted))
  }

  fn move_message<'a>(
    &'a self,
    _at: &'a MailRef,
    _message_id: Option<&'a str>,
    _folder: &'a str,
  ) -> Pending<'a, Option<MailRef>> {
    Box::pin(async { Err(not_offered()) })
  }

  fn delete_forever<'a>(&'a self, _at: &'a MailRef) -> Pending<'a, ()> {
    Box::pin(async {
      Err(Asked::rule("outside-trash", "a POP3 account has no spam or trash folder to delete from").into())
    })
  }

  fn drop_label<'a>(&'a self, label_id: &'a str) -> Pending<'a, ()> {
    Box::pin(self.drop_label_of(label_id))
  }

  fn run_rules<'a>(&'a self, folder: &'a str) -> Pending<'a, RulesRun> {
    Box::pin(self.run_rules_on(folder))
  }

  fn draft<'a>(&'a self, draft_id: &'a str) -> Pending<'a, Draft> {
    Box::pin(self.draft_named(draft_id))
  }

  fn save_draft<'a>(&'a self, draft: &'a Draft) -> Pending<'a, Draft> {
    Box::pin(self.save(draft))
  }

  fn discard_draft<'a>(&'a self, draft_id: &'a str) -> Pending<'a, ()> {
    Box::pin(self.discard(draft_id))
  }

  fn sending_limit(&self) -> BoxFuture<'_, Option<u64>> {
    Box::pin(self.limit.of(&self.smtp))
  }

  fn send_draft<'a>(&'a self, draft_id: &'a str) -> Pending<'a, Vec<MailAddress>> {
    Box::pin(self.send(draft_id))
  }

  fn send_receipt<'a>(&'a self, at: &'a MailRef, wording: &'a Wording) -> Pending<'a, ()> {
    Box::pin(self.receipt(at, wording))
  }

  fn watch(self: Arc<Self>, watch: Watch) -> BoxFuture<'static, ()> {
    let watcher = Pop3Watcher {
      standing: watch.standing,
      account: watch.account,
      maildrop: Arc::clone(&self.maildrop),
      mailbox: self,
    };

    Box::pin(watcher.run(watch.stop))
  }

  fn close(&self) -> BoxFuture<'_, ()> {
    Box::pin(async {})
  }
}
