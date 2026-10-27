//! The IMAP/SMTP adapter (`docs/mail.md`, 「Accounts and credentials」, 「The mail server」): an account of the
//! kind `imap-smtp` is a mail server reached over IMAP and submission, signed in with a login and a password.
//! Its mailbox is the service's IMAP reading and writing (`crate::imap`, `crate::writing`), its rules carried
//! out with IMAP commands (`crate::rules`), its labels kept as IMAP keywords (`crate::labels`), and its watcher
//! a connection standing in `IDLE` (`crate::imap::watcher`).

use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::future::BoxFuture;
use sqlx::SqlitePool;

use crate::accounts::Account;
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
use crate::wire::{AccountKind, FieldKind, Fields, KindField, MessageQuery, SignIn};
use crate::wording::Wording;
use crate::writing::drafts::{discard_draft, draft_named, find_draft, save_and_open};
use crate::writing::receipts::send_receipt;
use crate::writing::submission::{self, send_draft, submission_limit};

/// The kind's name.
pub const KIND: &str = "imap-smtp";

const IMAP_HOST: &str = "imapHost";
const IMAP_PORT: &str = "imapPort";
const IMAP_TLS_NAME: &str = "imapTlsName";
const SMTP_HOST: &str = "smtpHost";
const SMTP_PORT: &str = "smtpPort";
const SMTP_TLS_NAME: &str = "smtpTlsName";
const LOGIN: &str = "login";
const PASSWORD: &str = "password";

/// The TLS name that means a plain connection, which only a stand-in server uses.
const PLAIN: &str = "-";
/// How long the submission server's size is trusted before it is asked again.
const LIMIT_KEPT: Duration = Duration::from_secs(60 * 60);

/// A server an account names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MailEndpoint {
  pub host: String,
  pub port: u16,
  /// The name the server's certificate is checked against, or none for a plain connection, which only a
  /// stand-in server uses.
  pub tls_name: Option<String>,
}

/// An account's login to its servers.
#[derive(Clone, PartialEq, Eq)]
pub struct MailAccount {
  pub address: String,
  pub login: String,
  pub password: String,
}

// A login written into a log names its address alone.
impl fmt::Debug for MailAccount {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter
      .debug_struct("MailAccount")
      .field("address", &self.address)
      .finish_non_exhaustive()
  }
}

/// The settings and secrets of an account of the accounts file, as this kind keeps them.
pub fn file_fields(imap: &MailEndpoint, smtp: &MailEndpoint, account: &MailAccount) -> (Fields, Fields) {
  let tls = |endpoint: &MailEndpoint| endpoint.tls_name.clone().unwrap_or_else(|| PLAIN.to_owned());
  let settings = Fields::from([
    (IMAP_HOST.to_owned(), imap.host.clone()),
    (IMAP_PORT.to_owned(), imap.port.to_string()),
    (IMAP_TLS_NAME.to_owned(), tls(imap)),
    (SMTP_HOST.to_owned(), smtp.host.clone()),
    (SMTP_PORT.to_owned(), smtp.port.to_string()),
    (SMTP_TLS_NAME.to_owned(), tls(smtp)),
    (LOGIN.to_owned(), account.login.clone()),
  ]);
  let secrets = Fields::from([(PASSWORD.to_owned(), account.password.clone())]);

  (settings, secrets)
}

/// A server of the settings: its host, its port, and its TLS name — the host where none is written, none
/// where it is `-`.
fn endpoint(settings: &Fields, host: &str, port: &str, tls_name: &str) -> Result<MailEndpoint, MailFault> {
  let unread = || MailFault::server("the account's servers do not read");
  let host = settings.get(host).filter(|host| !host.is_empty()).ok_or_else(unread)?.clone();
  let port = settings
    .get(port)
    .and_then(|port| port.parse::<u16>().ok())
    .filter(|port| *port > 0)
    .ok_or_else(unread)?;
  let tls_name = match settings.get(tls_name).map(String::as_str) {
    Some(PLAIN) => None,
    Some(name) if !name.is_empty() => Some(name.to_owned()),
    _ => Some(host.clone()),
  };

  Ok(MailEndpoint {
    host,
    port,
    tls_name,
  })
}

/// An account's IMAP server, submission server and login, read from its fields.
fn servers(
  address: &str,
  settings: &Fields,
  secrets: &Fields,
) -> Result<(MailEndpoint, MailEndpoint, MailAccount), MailFault> {
  let imap = endpoint(settings, IMAP_HOST, IMAP_PORT, IMAP_TLS_NAME)?;
  let smtp = endpoint(settings, SMTP_HOST, SMTP_PORT, SMTP_TLS_NAME)?;
  let login = settings
    .get(LOGIN)
    .filter(|login| !login.is_empty())
    .cloned()
    .unwrap_or_else(|| address.to_owned());
  let password = secrets
    .get(PASSWORD)
    .cloned()
    .ok_or_else(|| MailFault::server("the account has no password"))?;

  Ok((
    imap,
    smtp,
    MailAccount {
      address: address.to_owned(),
      login,
      password,
    },
  ))
}

fn field(name: &str, kind: FieldKind, required: bool, default: Option<&str>) -> KindField {
  KindField {
    name: name.to_owned(),
    kind,
    required,
    default: default.map(str::to_owned),
  }
}

/// The adapter.
pub struct ImapSmtp;

impl Adapter for ImapSmtp {
  fn kind(&self) -> AccountKind {
    AccountKind {
      name: KIND.to_owned(),
      sign_in: SignIn::Password,
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
      let refused = |why: MailFault| {
        let why = match why {
          MailFault::Asked(asked) => asked.error,
          MailFault::Server(why) => why,
        };

        eprintln!("mail: an account of {address} did not sign in: {why}");

        MailFault::from(Asked::rule("sign-in-failed", "the account's servers did not take its sign-in"))
      };
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
      limit: Mutex::new(None),
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
  /// The size the submission server takes, and when it was asked.
  limit: Mutex<Option<(Option<u64>, Instant)>>,
}

impl ImapSmtpMailbox {
  fn known_limit(&self) -> Option<Option<u64>> {
    let known = *self.limit.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

    known.filter(|(_, at)| at.elapsed() < LIMIT_KEPT).map(|(limit, _)| limit)
  }

  /// The size the submission server takes, asked once an hour.
  async fn limit(&self) -> Option<u64> {
    if let Some(limit) = self.known_limit() {
      return limit;
    }

    let limit = submission_limit(&self.smtp).await;

    *self.limit.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some((limit, Instant::now()));

    limit
  }
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
    Box::pin(self.limit())
  }

  fn send_draft<'a>(&'a self, draft_id: &'a str) -> Pending<'a, Vec<MailAddress>> {
    Box::pin(async move {
      let limit = self.limit().await;

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
