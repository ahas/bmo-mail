//! Sending (`docs/mail.md`, 「Writing」): the draft saved, the message held for a few seconds while undo
//! send can still take it back, then sent over submission, its copy kept in the sent folder and the draft
//! removed. A service that stops during the hold leaves the draft where it was, never a message half sent.

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
use crate::routes::requests::MailFault;
use crate::wire::{Draft, DraftKind, FolderRole, MailRef, SendState};
use crate::writing::addresses::record_addresses;
use crate::writing::compose::{Purpose, build_message, new_message_id};
use crate::writing::drafts::{discard_draft, open_draft, resolve_attachments, save_draft};

/// How long a message is held before it goes, while undo send can still take it back.
pub const SEND_HOLD: Duration = Duration::from_secs(10);
/// How long a settled hold is kept for the screen to ask about.
const SETTLED_KEPT: Duration = Duration::from_secs(60);
/// How long the submission server's size is trusted before it is asked again.
const LIMIT_KEPT: Duration = Duration::from_secs(60 * 60);
/// The submission port that speaks TLS from the first byte; any other with a TLS name is upgraded with
/// STARTTLS.
const IMPLICIT_TLS_PORT: u16 = 465;
/// How long a conversation with the submission server may take.
const SUBMISSION_TIMEOUT: Duration = Duration::from_secs(60);

/// A message held before it goes.
struct Held {
  draft: MailRef,
  state: SendState,
  /// The hold's timer while it still runs; none once it has fired.
  timer: Option<AbortHandle>,
}

/// The messages held and the sizes the submission servers take.
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

  /// Where a send stands: held, sent, taken back or failed; none once it has long ended.
  pub fn state(&self, id: &str) -> Option<SendState> {
    held(self).get(id).map(|entry| entry.state.clone())
  }

  /// Undo send: ends the hold before the message goes. Answers the draft to reopen, or none when the hold
  /// has already ended.
  pub fn cancel(&self, id: &str) -> Option<MailRef> {
    let mut held = held(self);
    let timer = held.get_mut(id)?.timer.take()?;

    timer.abort();

    held.remove(id).map(|entry| entry.draft)
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

/// Sends a saved draft now: what the hold runs when it ends.
async fn send_draft(
  standing: &Standing,
  session: &MailSession,
  smtp: &MailEndpoint,
  at: &MailRef,
) -> Result<(), MailFault> {
  let draft = open_draft(session, at).await?;
  let recipients: Vec<_> = draft.to.iter().chain(&draft.cc).chain(&draft.bcc).cloned().collect();

  if recipients.is_empty() {
    return Err(MailFault::asked("no_recipients", "a message to send has no recipient"));
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
    return Err(MailFault::asked(
      "message_too_large",
      "the encoded message is past the size the server takes",
    ));
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
  discard_draft(session, at).await?;

  if let Some(original) = &draft.original
    && draft.kind != DraftKind::New
  {
    let flag = if draft.kind == DraftKind::Reply { "\\Answered" } else { "$Forwarded" };

    mark_original(session, original, flag).await?;
  }

  record_addresses(standing.store.pool(), &draft.from, &recipients, now.timestamp_millis()).await
}

/// Why a send failed, in words for the log and the screen's reason.
fn failure(fault: MailFault) -> String {
  match fault {
    MailFault::Asked(asked) => asked.error,
    MailFault::Server(why) => why,
  }
}

/// Send: saves the draft, then holds the message for the hold's length before it goes. Answers the hold's
/// id, which undo send names, and where the draft now stands.
pub async fn queue_send(
  standing: Arc<Standing>,
  session: Arc<MailSession>,
  smtp: MailEndpoint,
  draft: Draft,
) -> Result<(String, MailRef), MailFault> {
  if draft.to.len() + draft.cc.len() + draft.bcc.len() == 0 {
    return Err(MailFault::asked("no_recipients", "a message to send has no recipient"));
  }

  let saved = save_draft(&session, &standing.settings.staging.dir, &draft).await?;
  let id = random_hex(9);

  held(&standing.sends).insert(
    id.clone(),
    Held {
      draft: saved.clone(),
      state: SendState::Holding,
      timer: None,
    },
  );

  let task = {
    let standing = Arc::clone(&standing);
    let id = id.clone();
    let saved = saved.clone();

    tokio::spawn(async move {
      tokio::time::sleep(SEND_HOLD).await;

      // The hold has fired: from here undo send can no longer take it back.
      let fired = held(&standing.sends).get_mut(&id).and_then(|entry| entry.timer.take()).is_some();

      if !fired {
        return;
      }

      let state = match send_draft(&standing, &session, &smtp, &saved).await {
        Ok(()) => SendState::Sent,
        Err(fault) => SendState::Failed {
          reason: failure(fault),
        },
      };

      if let Some(entry) = held(&standing.sends).get_mut(&id) {
        entry.state = state;
      }

      tokio::time::sleep(SETTLED_KEPT).await;
      held(&standing.sends).remove(&id);
    })
  };

  if let Some(entry) = held(&standing.sends).get_mut(&id) {
    entry.timer = Some(task.abort_handle());
  }

  Ok((id, saved))
}
