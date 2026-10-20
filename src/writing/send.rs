//! Sending (`docs/mail.md`, 「Writing」): a send waits in the service for the account's undo span while it can
//! still be taken back, then the draft goes over submission, its copy is kept in the sent folder and the draft
//! removed. A service that stops while a send waits leaves the draft where it was, never a message half sent.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mail_send::SmtpClientBuilder;
use mail_send::smtp::message::Message;
use rand::RngCore;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::task::AbortHandle;

use crate::accounts::{MailAccount, MailEndpoint};
use crate::http::Standing;
use crate::imap::folders::{ensure_role_folder, folders_of, searched_folders};
use crate::imap::search::Search;
use crate::imap::session::MailSession;
use crate::imap::{append_landing, store_flags, uid_search};
use crate::mail::{DraftKind, FolderRole, MailRef};
use crate::routes::requests::{Asked, MailFault};
use crate::wire::{Send, SendState};
use crate::writing::addresses::record_addresses;
use crate::writing::compose::{Purpose, build_message, new_message_id};
use crate::writing::drafts::{discard_draft, draft_named, resolve_attachments};

/// The kind of the event a send's change makes.
pub const SEND_CHANGED: &str = "send.changed";
/// How long a settled send is kept for a reader to ask about.
const SETTLED_KEPT: Duration = Duration::from_secs(60);
/// How long the submission server's size is trusted before it is asked again.
const LIMIT_KEPT: Duration = Duration::from_secs(60 * 60);
/// The submission port that speaks TLS from the first byte; any other with a TLS name is upgraded with
/// STARTTLS.
const IMPLICIT_TLS_PORT: u16 = 465;
/// How long a conversation with the submission server may take.
const SUBMISSION_TIMEOUT: Duration = Duration::from_secs(60);

/// A send kept in the service: where it stands, and its timer while it still waits.
struct Held {
  send: Send,
  timer: Option<AbortHandle>,
}

/// The sends kept and the sizes the submission servers take.
#[derive(Default)]
pub struct Sends {
  held: Mutex<HashMap<String, Held>>,
  limits: Mutex<HashMap<String, (Option<u64>, Instant)>>,
}

impl std::fmt::Debug for Sends {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter.debug_struct("Sends").finish_non_exhaustive()
  }
}

fn held(sends: &Sends) -> std::sync::MutexGuard<'_, HashMap<String, Held>> {
  sends.held.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn builder(endpoint: &MailEndpoint) -> Result<SmtpClientBuilder<String>, MailFault> {
  let mut builder = SmtpClientBuilder::new(endpoint.host.clone(), endpoint.port)
    .map_err(MailFault::server)?
    .timeout(SUBMISSION_TIMEOUT)
    .implicit_tls(endpoint.port == IMPLICIT_TLS_PORT);

  if let Some(name) = &endpoint.tls_name {
    builder.tls_hostname = name.clone();
  }

  Ok(builder)
}

async fn deliver<T: AsyncRead + AsyncWrite + Unpin>(
  mut client: mail_send::SmtpClient<T>,
  from: &str,
  to: &[String],
  raw: &[u8],
) -> Result<(), MailFault> {
  let message = Message::new(from.to_owned(), to.iter().cloned(), raw);

  client.send(message).await.map_err(MailFault::server)?;
  let _ = client.quit().await;

  Ok(())
}

/// Hands one built message to the submission server as the account, to the envelope's recipients: over
/// TLS on port 465, upgraded with STARTTLS on another port with a TLS name, and plain where the TLS name is
/// `-`.
pub async fn submit(
  endpoint: &MailEndpoint,
  account: &MailAccount,
  from: &str,
  to: &[String],
  raw: &[u8],
) -> Result<(), MailFault> {
  let builder = builder(endpoint)?.credentials((account.login.clone(), account.password.clone()));

  if endpoint.tls_name.is_none() {
    deliver(builder.connect_plain().await.map_err(MailFault::server)?, from, to, raw).await
  } else {
    deliver(builder.connect().await.map_err(MailFault::server)?, from, to, raw).await
  }
}

async fn advertised<T: AsyncRead + AsyncWrite + Unpin>(
  mut client: mail_send::SmtpClient<T>,
  local_host: &str,
) -> Option<u64> {
  let size = client.capabilities(local_host, false).await.ok().map(|said| said.size as u64);
  let _ = client.quit().await;

  size.filter(|size| *size > 0)
}

impl Sends {
  /// The largest message the submission server says it takes (its `SIZE`), or none where it names no
  /// limit or does not answer. Asked once an hour; asking needs no login.
  pub async fn submission_limit(&self, endpoint: &MailEndpoint) -> Option<u64> {
    let key = format!("{}:{}", endpoint.host, endpoint.port);
    let known = self.limits.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get(&key).copied();

    if let Some((limit, at)) = known
      && at.elapsed() < LIMIT_KEPT
    {
      return limit;
    }

    let limit = match builder(endpoint) {
      Err(_) => None,
      Ok(builder) => {
        let builder = builder.say_ehlo(false);

        if endpoint.tls_name.is_none() {
          match builder.connect_plain().await {
            Ok(client) => advertised(client, &builder.local_host).await,
            Err(_) => None,
          }
        } else {
          match builder.connect().await {
            Ok(client) => advertised(client, &builder.local_host).await,
            Err(_) => None,
          }
        }
      }
    };

    self
      .limits
      .lock()
      .unwrap_or_else(|poisoned| poisoned.into_inner())
      .insert(key, (limit, Instant::now()));

    limit
  }

  /// A send of the account as it stands; none once it has long ended.
  pub fn send(&self, account: &str, id: &str) -> Option<Send> {
    held(self)
      .get(id)
      .map(|entry| entry.send.clone())
      .filter(|send| send.account_name == account)
  }

  /// Takes a send back while it waits: the send as it now stands, cancelled; a conflict where it has already
  /// gone, and none where the account has no such send.
  pub fn cancel(&self, account: &str, id: &str) -> Option<Result<Send, Asked>> {
    let mut held = held(self);
    let entry = held.get_mut(id).filter(|entry| entry.send.account_name == account)?;
    let Some(timer) = entry.timer.take() else {
      return Some(Err(Asked::conflict("the send has already gone")));
    };

    timer.abort();
    entry.send.state = SendState::Cancelled;

    Some(Ok(entry.send.clone()))
  }

  /// Moves a send to where it now stands, and answers it.
  fn settle(&self, id: &str, state: SendState, failure: Option<String>) -> Option<Send> {
    let mut held = held(self);
    let entry = held.get_mut(id)?;

    entry.send.state = state;
    entry.send.failure = failure;

    Some(entry.send.clone())
  }
}

/// Marks the message a reply answered or a forward carried, wherever in the mailbox it is.
async fn mark_original(session: &MailSession, message_id: &str, flag: &str) -> Result<(), MailFault> {
  let search = Search::Header("message-id".to_owned(), message_id.to_owned());

  for folder in searched_folders(&folders_of(session).await?) {
    let mut connection = session.folder(&folder.path).await?;
    let done = async {
      let uids = uid_search(connection.session(), &search.command()).await?;

      store_flags(connection.session(), &uids, "+", &[flag.to_owned()]).await
    }
    .await;

    connection.settle(done)?;
  }

  Ok(())
}

fn random_hex(bytes: usize) -> String {
  let mut random = vec![0u8; bytes];

  rand::rng().fill_bytes(&mut random);
  random.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Sends the draft a `Message-ID` names now: what a send runs when its wait ends. The draft is read where it
/// stands at that moment, so a change made while the send waited goes with it.
async fn send_draft(
  standing: &Standing,
  session: &MailSession,
  smtp: &MailEndpoint,
  draft_id: &str,
) -> Result<(), MailFault> {
  let draft = draft_named(session, draft_id).await?;
  let at: MailRef = draft.saved.clone().ok_or_else(|| MailFault::server("the draft stands nowhere"))?;
  let recipients: Vec<_> = draft.to.iter().chain(&draft.cc).chain(&draft.bcc).cloned().collect();

  if recipients.is_empty() {
    return Err(Asked::rule("no-recipients", "a message to send has no recipient").into());
  }

  let staging = &standing.settings.staging.dir;
  let attachments = resolve_attachments(session, staging, &draft.attachments).await?;
  let message_id = new_message_id(&draft.from);
  let now = chrono::Utc::now();
  let raw = build_message(&draft, &attachments, Purpose::Send, Some(&message_id), now.timestamp())
    .map_err(MailFault::server)?;

  if let Some(limit) = standing.sends.submission_limit(smtp).await
    && raw.len() as u64 > limit
  {
    return Err(Asked::too_large("the encoded message is past the size the server takes").into());
  }

  let to: Vec<String> = recipients.iter().map(|one| one.address.clone()).collect();

  submit(smtp, &session.account, &draft.from, &to, &raw).await?;

  let copy = build_message(&draft, &attachments, Purpose::SentCopy, Some(&message_id), now.timestamp())
    .map_err(MailFault::server)?;
  let sent = ensure_role_folder(session, FolderRole::Sent).await?;
  let internal_date = now.format("%d-%b-%Y %H:%M:%S +0000").to_string();
  let mut connection = session.connection().await?;
  let appended = append_landing(connection.session(), &sent, "\\Seen", Some(&internal_date), &copy).await;

  connection.settle(appended)?;
  discard_draft(session, &at).await?;

  if let Some(original) = &draft.original
    && draft.kind != DraftKind::New
  {
    let flag = if draft.kind == DraftKind::Reply { "\\Answered" } else { "$Forwarded" };

    mark_original(session, original, flag).await?;
  }

  record_addresses(standing.store.pool(), &draft.from, &recipients, now.timestamp_millis()).await
}

/// Why a send failed, in words for the log and the send's `failure`.
fn failure(fault: MailFault) -> String {
  match fault {
    MailFault::Asked(asked) => asked.error,
    MailFault::Server(why) => why,
  }
}

/// A send of the draft a `Message-ID` names: it waits until `send_at`, and then goes. A draft that is not there,
/// or has no recipient, is refused before anything waits.
pub async fn queue_send(
  standing: Arc<Standing>,
  session: Arc<MailSession>,
  smtp: MailEndpoint,
  account: &str,
  draft_id: &str,
  send_at: i64,
) -> Result<Send, MailFault> {
  let draft = draft_named(&session, draft_id).await?;

  if draft.to.len() + draft.cc.len() + draft.bcc.len() == 0 {
    return Err(Asked::rule("no-recipients", "a message to send has no recipient").into());
  }

  let id = random_hex(9);
  let send = Send {
    id: id.clone(),
    account_name: account.to_owned(),
    draft_id: draft_id.to_owned(),
    state: SendState::Waiting,
    send_at,
    failure: None,
  };

  held(&standing.sends).insert(
    id.clone(),
    Held {
      send: send.clone(),
      timer: None,
    },
  );

  let task = {
    let standing = Arc::clone(&standing);
    let id = id.clone();
    let draft_id = draft_id.to_owned();
    let account = account.to_owned();
    let wait =
      Duration::from_millis(u64::try_from(send_at - chrono::Utc::now().timestamp_millis()).unwrap_or(0));

    tokio::spawn(async move {
      tokio::time::sleep(wait).await;

      // The wait has ended: from here the send can no longer be taken back.
      let fired = held(&standing.sends).get_mut(&id).and_then(|entry| entry.timer.take()).is_some();

      if !fired {
        return;
      }

      let settled = match send_draft(&standing, &session, &smtp, &draft_id).await {
        Ok(()) => standing.sends.settle(&id, SendState::Sent, None),
        Err(fault) => standing.sends.settle(&id, SendState::Failed, Some(failure(fault))),
      };

      if let Some(send) = settled {
        standing.events.publish(SEND_CHANGED, Some(&account), &send).await;
      }

      tokio::time::sleep(SETTLED_KEPT).await;
      held(&standing.sends).remove(&id);
    })
  };

  if let Some(entry) = held(&standing.sends).get_mut(&id) {
    entry.timer = Some(task.abort_handle());
  }

  Ok(send)
}

/// Forgets a cancelled send once a reader has had its while to ask about it.
pub fn forget_later(standing: Arc<Standing>, id: String) {
  tokio::spawn(async move {
    tokio::time::sleep(SETTLED_KEPT).await;
    held(&standing.sends).remove(&id);
  });
}
