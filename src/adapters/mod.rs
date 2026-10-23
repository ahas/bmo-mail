//! The adapters (`docs/mail.md`, 「Accounts and credentials」): one for each kind of account, each reaching
//! the mailboxes of its kind. An adapter declares its kind — how an account of it signs in and the fields a
//! person writes — checks a new account's sign-in, and opens an account's mailbox, which every route and the
//! watchers then read and change through `Mailbox` alone. Nothing outside an adapter knows a kind by name: an
//! account names its adapter by the kind it stores, and each adapter registers itself beside its own code.

pub mod imap_smtp;

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use futures::future::BoxFuture;
use sqlx::SqlitePool;
use tokio::sync::watch;

use crate::accounts::Account;
use crate::http::Standing;
use crate::mail::{Draft, MailAddress, MailFolder, MailFolderState, MailMessage, MailRef, OpenedPart};
use crate::routes::downloads::Disposition;
use crate::routes::requests::{Asked, MailFault};
use crate::rules::RulesRun;
use crate::wire::{AccountKind, FieldKind, Fields, MessageQuery, SignIn};
use crate::wording::Wording;

pub use inventory;

/// What an adapter's work comes to: its answer, or why it did not.
pub type Pending<'a, T> = BoxFuture<'a, Result<T, MailFault>>;

/// An account whose sign-in the provider's page made: its address, and its fields as they are to be kept.
pub struct SignedIn {
  pub address: String,
  pub settings: Fields,
  pub secrets: Fields,
}

/// An authorization begun: the provider's page, and what the adapter keeps until the browser comes back.
pub struct Begun {
  pub url: String,
  pub kept: Fields,
}

/// What a mailbox stands on beside its account: the store, and the folder posted files wait in.
#[derive(Clone, Debug)]
pub struct Context {
  pub pool: SqlitePool,
  pub staging: PathBuf,
}

/// One kind of account: what reaches the mailboxes of that kind.
pub trait Adapter: Send + Sync + 'static {
  /// The kind as the adapter declares it: its name, how it signs in, its fields.
  fn kind(&self) -> AccountKind;

  /// Signs in as an account of the kind with its address and its fields, and lets go: what adding or changing
  /// an account that signs in by password checks before anything is kept. A sign-in the servers refuse is
  /// `sign-in-failed`.
  fn sign_in<'a>(&'a self, address: &'a str, settings: &'a Fields, secrets: &'a Fields) -> Pending<'a, ()>;

  /// Begins an authorization on the provider's page, the browser to come back to `return_to` with `state`
  /// in its query; none for a kind that signs in by password.
  fn begin(&self, _state: &str, _return_to: &str) -> Option<Result<Begun, MailFault>> {
    None
  }

  /// Finishes an authorization with what the adapter kept and what the provider answered.
  fn finish<'a>(&'a self, _kept: &'a Fields, _answer: &'a Fields) -> Pending<'a, SignedIn> {
    Box::pin(async { Err(Asked::rule("no-authorization", "the kind signs in by password").into()) })
  }

  /// The mailbox of an account of the kind.
  fn open(&self, account: &Account, context: &Context) -> Result<Arc<dyn Mailbox>, MailFault>;
}

/// What a watcher is started with: the service, the account it watches, and the word that stops it.
pub struct Watch {
  pub standing: Arc<Standing>,
  pub account: Account,
  pub stop: watch::Receiver<bool>,
}

/// One account's mailbox, as every route and the watchers read and change it.
pub trait Mailbox: Send + Sync {
  /// The unread messages of the inbox.
  fn inbox_unread(&self) -> Pending<'_, u32>;
  /// Every folder.
  fn folders(&self) -> Pending<'_, Vec<MailFolder>>;
  /// Every folder with its counts, in the order the nav lists them.
  fn folder_states(&self) -> Pending<'_, Vec<MailFolderState>>;
  /// One folder with its counts and its place, or none where it does not stand.
  fn folder_state<'a>(&'a self, path: &'a str) -> Pending<'a, Option<(usize, MailFolderState)>>;
  /// Every message of the folder marked read, or every one unread.
  fn mark_folder<'a>(&'a self, path: &'a str, read: bool) -> Pending<'a, ()>;
  /// One page of the messages a query asks for, without their bodies, and whether more stand past it.
  fn page<'a>(&'a self, query: &'a MessageQuery) -> Pending<'a, (Vec<MailMessage>, bool)>;
  /// One message with its bodies.
  fn message<'a>(&'a self, at: &'a MailRef) -> Pending<'a, Option<MailMessage>>;
  /// One message as a list shows it, without its bodies.
  fn row<'a>(&'a self, at: &'a MailRef) -> Pending<'a, Option<MailMessage>>;
  /// Every message of a conversation, oldest first, each with its bodies.
  fn thread<'a>(&'a self, key: &'a str, folder: Option<&'a str>) -> Pending<'a, Vec<MailMessage>>;
  /// A part's bytes, by its number or its content id, and how it is handed over.
  fn part<'a>(&'a self, at: &'a MailRef, part: &'a str) -> Pending<'a, Option<(OpenedPart, Disposition)>>;
  /// Sets the whole of the flags a message carries among those a request may set; false where it is gone.
  fn set_flags<'a>(&'a self, at: &'a MailRef, wanted: &'a [String]) -> Pending<'a, bool>;
  /// Moves a message into a folder, and answers where it landed where that is known.
  fn move_message<'a>(
    &'a self,
    at: &'a MailRef,
    message_id: Option<&'a str>,
    folder: &'a str,
  ) -> Pending<'a, Option<MailRef>>;
  /// Deletes a message for good, from spam or trash alone.
  fn delete_forever<'a>(&'a self, at: &'a MailRef) -> Pending<'a, ()>;
  /// Takes a label off every message that carries it.
  fn drop_label<'a>(&'a self, label_id: &'a str) -> Pending<'a, ()>;
  /// Runs the account's rules over a folder now.
  fn run_rules<'a>(&'a self, folder: &'a str) -> Pending<'a, RulesRun>;
  /// The draft a `Message-ID` names; a 404 where none does.
  fn draft<'a>(&'a self, draft_id: &'a str) -> Pending<'a, Draft>;
  /// Saves a draft under the `Message-ID` it keeps, and answers it as it now stands.
  fn save_draft<'a>(&'a self, draft: &'a Draft) -> Pending<'a, Draft>;
  /// Throws the draft a `Message-ID` names away; a 404 where none does.
  fn discard_draft<'a>(&'a self, draft_id: &'a str) -> Pending<'a, ()>;
  /// The largest message the account may send, where its server names one.
  fn sending_limit(&self) -> BoxFuture<'_, Option<u64>>;
  /// Sends the draft a `Message-ID` names as it stands now, and answers its recipients.
  fn send_draft<'a>(&'a self, draft_id: &'a str) -> Pending<'a, Vec<MailAddress>>;
  /// Sends the read receipt a message asks for, in the service's own words, and marks it answered.
  fn send_receipt<'a>(&'a self, at: &'a MailRef, wording: &'a Wording) -> Pending<'a, ()>;
  /// Watches the inbox until told to stop (`docs/mail.md`, 「Connections and the watcher」).
  fn watch(self: Arc<Self>, watch: Watch) -> BoxFuture<'static, ()>;
  /// Lets the mailbox's connections go.
  fn close(&self) -> BoxFuture<'_, ()>;
}

/// One registered adapter.
pub struct Registered(pub &'static dyn Adapter);

inventory::collect!(Registered);

/// Registers an adapter with the service: `bmo_mail::adapter!(ImapSmtp);`
#[macro_export]
macro_rules! adapter {
  ($adapter:expr) => {
    $crate::adapters::inventory::submit! { $crate::adapters::Registered(&$adapter) }
  };
}

/// Every adapter built into this binary, ordered by its kind's name.
pub fn adapters() -> Vec<&'static dyn Adapter> {
  let mut all: Vec<&'static dyn Adapter> =
    inventory::iter::<Registered>.into_iter().map(|entry| entry.0).collect();

  all.sort_by_key(|adapter| adapter.kind().name);
  all
}

/// The adapter of a kind, or none where no adapter of that name is built in.
pub fn adapter(kind: &str) -> Option<&'static dyn Adapter> {
  adapters().into_iter().find(|adapter| adapter.kind().name == kind)
}

/// The adapter of a kind, or a refusal naming the kind as one the service does not declare.
pub fn declared(kind: &str) -> Result<&'static dyn Adapter, Asked> {
  adapter(kind)
    .ok_or_else(|| Asked::rule("unknown-kind", "the service declares no kind of account of that name"))
}

/// An account's fields checked against its kind's: each one declared and in its place, a port a port, every
/// required one given; an empty one is none, and a field left out takes its default.
pub fn checked_fields(
  kind: &AccountKind,
  settings: &Fields,
  secrets: &Fields,
) -> Result<(Fields, Fields), Asked> {
  let secret = |name: &str| {
    kind
      .fields
      .iter()
      .find(|field| field.name == name)
      .map(|field| field.kind == FieldKind::Secret)
  };

  for (name, is_secret) in
    settings.keys().map(|name| (name, false)).chain(secrets.keys().map(|name| (name, true)))
  {
    if secret(name) != Some(is_secret) {
      let place = if is_secret { "secrets" } else { "settings" };

      return Err(Asked::bad_request(format!(
        "the kind {} has no field {name} among its {place}",
        kind.name
      )));
    }
  }

  let mut kept_settings = Fields::new();
  let mut kept_secrets = Fields::new();

  for field in &kind.fields {
    let given = if field.kind == FieldKind::Secret { secrets } else { settings };
    let value = given
      .get(&field.name)
      .map(|value| value.trim().to_owned())
      .filter(|value| !value.is_empty())
      .or_else(|| field.default.clone());
    let Some(value) = value else {
      if field.required {
        return Err(Asked::bad_request(format!("the field {} is required", field.name)));
      }

      continue;
    };

    if field.kind == FieldKind::Port && !value.parse::<u16>().is_ok_and(|port| port > 0) {
      return Err(Asked::bad_request(format!("the field {} is not a port", field.name)));
    }

    if field.kind == FieldKind::Secret {
      kept_secrets.insert(field.name.clone(), value);
    } else {
      kept_settings.insert(field.name.clone(), value);
    }
  }

  Ok((kept_settings, kept_secrets))
}

/// Whether a kind signs in by password.
pub fn by_password(kind: &AccountKind) -> bool {
  kind.sign_in == SignIn::Password
}

/// The open mailboxes, by their account's id, each with the account as it stood when it was opened.
type Opened = HashMap<i64, (Account, Arc<dyn Mailbox>)>;

/// Every account's open mailbox, kept while the account stands as it was when it was opened: a changed
/// account — a new password, another server — opens a new one and lets the old one's connections go.
#[derive(Default)]
pub struct Mailboxes {
  open: Mutex<Opened>,
}

impl fmt::Debug for Mailboxes {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.debug_struct("Mailboxes").finish_non_exhaustive()
  }
}

impl Mailboxes {
  fn open(&self) -> std::sync::MutexGuard<'_, Opened> {
    self.open.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
  }

  /// The mailbox of an account, opened through its kind's adapter where it is not open as the account now
  /// stands.
  pub fn of(&self, account: &Account, context: &Context) -> Result<Arc<dyn Mailbox>, MailFault> {
    if let Some((opened, mailbox)) = self.open().get(&account.id)
      && opened == account
    {
      return Ok(Arc::clone(mailbox));
    }

    let adapter = adapter(&account.kind)
      .ok_or_else(|| MailFault::server(format!("no adapter of the kind {} is built in", account.kind)))?;
    let mailbox = adapter.open(account, context)?;
    let stale = self.open().insert(account.id, (account.clone(), Arc::clone(&mailbox)));

    if let Some((_, stale)) = stale {
      tokio::spawn(async move { stale.close().await });
    }

    Ok(mailbox)
  }

  /// Lets an account's mailbox go, as the account is removed.
  pub async fn forget(&self, id: i64) {
    let gone = self.open().remove(&id);

    if let Some((_, mailbox)) = gone {
      mailbox.close().await;
    }
  }

  /// Lets every mailbox go, as the service stops.
  pub async fn close_all(&self) {
    let all: Vec<Arc<dyn Mailbox>> = self.open().drain().map(|(_, (_, mailbox))| mailbox).collect();

    futures::future::join_all(all.iter().map(|mailbox| mailbox.close())).await;
  }
}
