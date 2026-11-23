//! The IMAP/SMTP adapter (`docs/mail.md`, 「Accounts and credentials」, 「The mail server」): an account of the
//! kind `imap-smtp` is a mail server reached over IMAP and submission, signed in with a login and a password.
//! Its mailbox is the service's IMAP reading and writing (`crate::imap`, `crate::writing`), its rules carried
//! out with IMAP commands (`crate::rules`), its labels kept as IMAP keywords (`crate::labels`), and its watcher
//! a connection standing in `IDLE` (`crate::imap::watcher`).

use std::path::PathBuf;
use std::sync::Arc;

use futures::future::BoxFuture;
use sqlx::SqlitePool;

use crate::accounts::Account;
use crate::adapters::servers::{
  LOGIN, MailAccount, MailEndpoint, PASSWORD, SMTP_HOST, SMTP_PORT, SMTP_TLS_NAME, field, refused_sign_in,
};
use crate::adapters::{Adapter, Context, Mailbox, Pending, Watch};
use crate::imap::folders::folders_of;
use crate::imap::messages::{
  delete_forever, find_part_by_cid, folder_state, folder_states, inbox_unseen, mark_folder, move_message,
  open_part, read_message, read_row_of, read_thread, set_flags,
};
use crate::imap::part_number;
use crate::imap::session::{MailSession, open};
use crate::imap::views::page_of;
use crate::imap::watcher::Watcher;
use crate::labels::strip_label;
use crate::mail::{Draft, MailAddress, MailFolder, MailFolderState, MailMessage, MailRef, OpenedPart};
use crate::routes::downloads::Disposition;
use crate::routes::requests::{Asked, MailFault};
use crate::rules::{RulesRun, run_rules_over_folder};
use crate::wire::{AccountKind, FieldKind, Fields, MessageQuery, SignIn};
use crate::wording::Wording;
use crate::writing::drafts::{discard_draft, draft_named, find_draft, save_and_open};
use crate::writing::receipts::send_receipt;
use crate::writing::submission::{self, SendingLimit, send_draft};

/// The kind's name.
pub const KIND: &str = "imap-smtp";

const IMAP_HOST: &str = "imapHost";
const IMAP_PORT: &str = "imapPort";
const IMAP_TLS_NAME: &str = "imapTlsName";

/// The settings and secrets of an account of the accounts file, as this kind keeps them.
pub fn file_fields(imap: &MailEndpoint, smtp: &MailEndpoint, account: &MailAccount) -> (Fields, Fields) {
  let settings = Fields::from([
    (IMAP_HOST.to_owned(), imap.host.clone()),
    (IMAP_PORT.to_owned(), imap.port.to_string()),
    (IMAP_TLS_NAME.to_owned(), imap.tls_field()),
    (SMTP_HOST.to_owned(), smtp.host.clone()),
    (SMTP_PORT.to_owned(), smtp.port.to_string()),
    (SMTP_TLS_NAME.to_owned(), smtp.tls_field()),
    (LOGIN.to_owned(), account.login.clone()),
  ]);
  let secrets = Fields::from([(PASSWORD.to_owned(), account.password.clone())]);

  (settings, secrets)
}

/// An account's IMAP server, submission server and login, read from its fields.
fn servers(
  address: &str,
  settings: &Fields,
  secrets: &Fields,
) -> Result<(MailEndpoint, MailEndpoint, MailAccount), MailFault> {
  let imap = MailEndpoint::of(settings, IMAP_HOST, IMAP_PORT, IMAP_TLS_NAME)?;
  let smtp = MailEndpoint::of(settings, SMTP_HOST, SMTP_PORT, SMTP_TLS_NAME)?;

  Ok((imap, smtp, MailAccount::of(address, settings, secrets)?))
}

/// The adapter.
pub struct ImapSmtp;

impl Adapter for ImapSmtp {
  fn kind(&self) -> AccountKind {
    AccountKind {
      name: KIND.to_owned(),
      sign_in: SignIn::Password,
      moves: true,
      fields: vec![
        field(IMAP_HOST, FieldKind::Text, true, None),
        field(IMAP_PORT, FieldKind::Port, true, Some("993")),
        field(IMAP_TLS_NAME, FieldKind::Text, false, None),
        field(SMTP_HOST, FieldKind::Text, true, None),
        field(SMTP_PORT, FieldKind::Port, true, Some("587")),
        field(SMTP_TLS_NAME, FieldKind::Text, false, None),
        field(LOGIN, FieldKind::Text, false, None),
        field(PASSWORD, FieldKind::Secret, true, None),
      ],
    }
  }

  fn sign_in<'a>(&'a self, address: &'a str, settings: &'a Fields, secrets: &'a Fields) -> Pending<'a, ()> {
    Box::pin(async move {
      let refused = |why: MailFault| refused_sign_in(address, why);
      let (imap, smtp, login) = servers(address, settings, secrets).map_err(refused)?;
      let mut session = open(&imap, &login).await.map_err(refused)?;
      let _ = session.logout().await;

      submission::sign_in(&smtp, &login).await.map_err(refused)
    })
  }

  fn open(&self, account: &Account, context: &Context) -> Result<Arc<dyn Mailbox>, MailFault> {
    let (imap, smtp, login) = servers(&account.address, &account.settings, &account.secrets)?;

    Ok(Arc::new(ImapSmtpMailbox {
      session: Arc::new(MailSession::new(imap, login)),
      smtp,
      pool: context.pool.clone(),
      key: account.store_key(),
      staging: context.staging.clone(),
      limit: SendingLimit::default(),
    }))
  }
}

crate::adapter!(ImapSmtp);

/// An `imap-smtp` account's mailbox: its work connection, its submission server, and the store its labels,
/// rules and marks stand in under its key.
pub struct ImapSmtpMailbox {
  session: Arc<MailSession>,
  smtp: MailEndpoint,
  pool: SqlitePool,
  key: String,
  staging: PathBuf,
  /// The size the submission server takes.
  limit: SendingLimit,
}

impl Mailbox for ImapSmtpMailbox {
  fn inbox_unread(&self) -> Pending<'_, u32> {
    Box::pin(self.session.read_twice(|| inbox_unseen(&self.session)))
  }

  fn folders(&self) -> Pending<'_, Vec<MailFolder>> {
    Box::pin(self.session.read_twice(|| folders_of(&self.session)))
  }

  fn folder_states(&self) -> Pending<'_, Vec<MailFolderState>> {
    Box::pin(self.session.read_twice(|| folder_states(&self.session)))
  }

  fn folder_state<'a>(&'a self, path: &'a str) -> Pending<'a, Option<(usize, MailFolderState)>> {
    Box::pin(self.session.read_twice(|| folder_state(&self.session, path)))
  }

  fn mark_folder<'a>(&'a self, path: &'a str, read: bool) -> Pending<'a, ()> {
    Box::pin(mark_folder(&self.session, path, read))
  }

  fn page<'a>(&'a self, query: &'a MessageQuery) -> Pending<'a, (Vec<MailMessage>, bool)> {
    Box::pin(self.session.read_twice(|| page_of(&self.pool, &self.session, &self.key, query)))
  }

  fn message<'a>(&'a self, at: &'a MailRef) -> Pending<'a, Option<MailMessage>> {
    Box::pin(self.session.read_twice(|| read_message(&self.session, at)))
  }

  fn row<'a>(&'a self, at: &'a MailRef) -> Pending<'a, Option<MailMessage>> {
    Box::pin(self.session.read_twice(|| read_row_of(&self.session, at)))
  }

  fn thread<'a>(&'a self, key: &'a str, folder: Option<&'a str>) -> Pending<'a, Vec<MailMessage>> {
    Box::pin(self.session.read_twice(move || read_thread(&self.session, key, folder)))
  }

  fn part<'a>(&'a self, at: &'a MailRef, part: &'a str) -> Pending<'a, Option<(OpenedPart, Disposition)>> {
    Box::pin(self.session.read_twice(move || async move {
      let (number, disposition) = if part_number(part) {
        (part.to_owned(), Disposition::Attachment)
      } else {
        let Some(found) = find_part_by_cid(&self.session, at, part).await? else {
          return Ok(None);
        };

        (found.part, Disposition::Inline)
      };

      Ok(open_part(&self.session, at, &number).await?.map(|opened| (opened, disposition)))
    }))
  }

  fn set_flags<'a>(&'a self, at: &'a MailRef, wanted: &'a [String]) -> Pending<'a, bool> {
    Box::pin(set_flags(&self.session, at, wanted))
  }

  fn move_message<'a>(
    &'a self,
    at: &'a MailRef,
    message_id: Option<&'a str>,
    folder: &'a str,
  ) -> Pending<'a, Option<MailRef>> {
    Box::pin(move_message(&self.session, at, message_id, folder))
  }

  fn delete_forever<'a>(&'a self, at: &'a MailRef) -> Pending<'a, ()> {
    Box::pin(delete_forever(&self.session, at))
  }

  fn drop_label<'a>(&'a self, label_id: &'a str) -> Pending<'a, ()> {
    Box::pin(strip_label(&self.session, label_id))
  }

  fn run_rules<'a>(&'a self, folder: &'a str) -> Pending<'a, RulesRun> {
    Box::pin(run_rules_over_folder(&self.pool, &self.session, &self.key, folder))
  }

  fn draft<'a>(&'a self, draft_id: &'a str) -> Pending<'a, Draft> {
    Box::pin(self.session.read_twice(|| draft_named(&self.session, draft_id)))
  }

  fn save_draft<'a>(&'a self, draft: &'a Draft) -> Pending<'a, Draft> {
    Box::pin(save_and_open(&self.session, &self.staging, draft))
  }

  fn discard_draft<'a>(&'a self, draft_id: &'a str) -> Pending<'a, ()> {
    Box::pin(async move {
      let Some(at) = find_draft(&self.session, draft_id).await? else {
        return Err(Asked::not_found("no draft has that id").into());
      };

      discard_draft(&self.session, &at).await
    })
  }

  fn sending_limit(&self) -> BoxFuture<'_, Option<u64>> {
    Box::pin(self.limit.of(&self.smtp))
  }

  fn send_draft<'a>(&'a self, draft_id: &'a str) -> Pending<'a, Vec<MailAddress>> {
    Box::pin(async move {
      let limit = self.limit.of(&self.smtp).await;

      send_draft(&self.session, &self.smtp, &self.staging, limit, draft_id).await
    })
  }

  fn send_receipt<'a>(&'a self, at: &'a MailRef, wording: &'a Wording) -> Pending<'a, ()> {
    Box::pin(send_receipt(&self.session, &self.smtp, wording, at))
  }

  fn watch(self: Arc<Self>, watch: Watch) -> BoxFuture<'static, ()> {
    let watcher = Watcher {
      standing: watch.standing,
      account: watch.account,
      endpoint: self.session.endpoint.clone(),
      login: self.session.account.clone(),
      mailbox: self,
    };

    Box::pin(watcher.run(watch.stop))
  }

  fn close(&self) -> BoxFuture<'_, ()> {
    Box::pin(self.session.close())
  }
}
