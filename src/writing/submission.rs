//! Submission (`docs/mail.md`, 「Writing」): the IMAP/SMTP adapter's sending. A message is handed to the
//! submission server as the account, the size the server takes is asked of it, and a draft sent has its copy
//! kept in the sent folder, the draft removed and the message it answered or carried marked.

use std::path::Path;
use std::time::Duration;

use mail_send::smtp::AssertReply;
use mail_send::smtp::message::Message;
use mail_send::{Credentials, SmtpClient, SmtpClientBuilder};
use smtp_proto::{EXT_START_TLS, EhloResponse};

use crate::adapters::servers::{MailAccount, MailEndpoint, MailStream};
use crate::imap::folders::{ensure_role_folder, folders_of, searched_folders};
use crate::imap::search::Search;
use crate::imap::session::MailSession;
use crate::imap::{append_landing, store_flags, uid_search};
use crate::mail::{Draft, DraftKind, FolderRole, MailAddress, MailRef};
use crate::routes::requests::{Asked, MailFault};
use crate::writing::compose::{Purpose, ResolvedAttachment, build_message, new_message_id};
use crate::writing::drafts::{PartSource, discard_draft, draft_named, resolve_attachments};

/// The submission port that speaks TLS from the first byte; any other with a TLS name is upgraded with
/// STARTTLS.
const IMPLICIT_TLS_PORT: u16 = 465;
/// The span the SMTP client takes for each step of a conversation, given as one it never reaches: a step
/// waits for the server's answer, and the connection's keep-alive probes break it where the server is gone
/// (`docs/mail.md`, 「Connections and the watcher」).
const UNBOUNDED: Duration = Duration::MAX;

/// A conversation with the submission server, over TLS or over a plain stream where the TLS name is `-`.
type Submission = SmtpClient<Box<dyn MailStream>>;

fn boxed<T: MailStream + 'static>(client: SmtpClient<T>) -> Submission {
  SmtpClient {
    stream: Box::new(client.stream),
    timeout: client.timeout,
  }
}

/// Reads the server's greeting.
async fn greeted<T: MailStream>(client: &mut SmtpClient<T>) -> Result<(), MailFault> {
  client
    .read()
    .await
    .map_err(MailFault::server)?
    .assert_positive_completion()
    .map_err(MailFault::server)
}

/// A conversation opened with the submission server: over TLS on port 465, upgraded with STARTTLS on
/// another port with a TLS name, and plain where the TLS name is `-`; greeted, its capabilities asked, and
/// where a login is given, signed in as it.
async fn conversation(
  endpoint: &MailEndpoint,
  login: Option<&MailAccount>,
) -> Result<(Submission, EhloResponse<String>), MailFault> {
  // The builder is read for the TLS connector and the name the service greets the server with.
  let builder = SmtpClientBuilder::new(endpoint.host.clone(), endpoint.port).map_err(MailFault::server)?;
  let mut opened = SmtpClient {
    stream: endpoint.connect().await?,
    timeout: UNBOUNDED,
  };
  let mut client = match endpoint.tls_name.as_deref() {
    None => {
      greeted(&mut opened).await?;
      boxed(opened)
    }
    Some(name) if endpoint.port == IMPLICIT_TLS_PORT => {
      let mut client = opened.into_tls(&builder.tls_connector, name).await.map_err(MailFault::server)?;

      greeted(&mut client).await?;
      boxed(client)
    }
    Some(name) => {
      greeted(&mut opened).await?;

      if !opened
        .ehlo(&builder.local_host)
        .await
        .map_err(MailFault::server)?
        .has_capability(EXT_START_TLS)
      {
        return Err(MailFault::server("the submission server offers no STARTTLS"));
      }

      // After STARTTLS the server greets no more; the conversation goes on over TLS.
      boxed(opened.start_tls(&builder.tls_connector, name).await.map_err(MailFault::server)?)
    }
  };
  let capabilities = client.capabilities(&builder.local_host, false).await.map_err(MailFault::server)?;

  if let Some(login) = login {
    let credentials = Credentials::new(login.login.as_str(), login.password.as_str());

    client.authenticate(&credentials, &capabilities).await.map_err(MailFault::server)?;
  }

  Ok((client, capabilities))
}

/// Hands one built message to the submission server as the account, to the envelope's recipients.
pub async fn submit(
  endpoint: &MailEndpoint,
  account: &MailAccount,
  from: &str,
  to: &[String],
  raw: &[u8],
) -> Result<(), MailFault> {
  let (mut client, _) = conversation(endpoint, Some(account)).await?;
  let message = Message::new(from.to_owned(), to.iter().cloned(), raw);

  client.send(message).await.map_err(MailFault::server)?;
  let _ = client.quit().await;

  Ok(())
}

/// Signs in to the submission server as the account and lets go, sending nothing.
pub async fn sign_in(endpoint: &MailEndpoint, account: &MailAccount) -> Result<(), MailFault> {
  let (client, _) = conversation(endpoint, Some(account)).await?;
  let _ = client.quit().await;

  Ok(())
}

/// The largest message the submission server says it takes (its `SIZE`), or none where it names no limit or
/// does not answer. Asking needs no login.
pub async fn submission_limit(endpoint: &MailEndpoint) -> Option<u64> {
  let (client, capabilities) = conversation(endpoint, None).await.ok()?;
  let _ = client.quit().await;

  Some(capabilities.size as u64).filter(|size| *size > 0)
}

/// How long the submission server's size is trusted before it is asked again.
const LIMIT_KEPT: Duration = Duration::from_secs(60 * 60);

/// The size an account's submission server takes, and when it was asked.
#[derive(Debug, Default)]
pub struct SendingLimit {
  known: std::sync::Mutex<Option<(Option<u64>, std::time::Instant)>>,
}

impl SendingLimit {
  fn known(&self) -> Option<Option<u64>> {
    let known = *self.known.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

    known.filter(|(_, at)| at.elapsed() < LIMIT_KEPT).map(|(limit, _)| limit)
  }

  /// The size the submission server takes, asked once an hour.
  pub async fn of(&self, smtp: &MailEndpoint) -> Option<u64> {
    if let Some(limit) = self.known() {
      return limit;
    }

    let limit = submission_limit(smtp).await;

    *self.known.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) =
      Some((limit, std::time::Instant::now()));

    limit
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

/// A draft made ready to go: its recipients, its attachments read, the `Message-ID` it goes out under, when
/// it was built and the message as it is sent.
pub struct Outgoing {
  pub recipients: Vec<MailAddress>,
  pub attachments: Vec<ResolvedAttachment>,
  pub message_id: String,
  pub at: chrono::DateTime<chrono::Utc>,
  pub raw: Vec<u8>,
}

impl Outgoing {
  /// The envelope's recipients.
  pub fn envelope(&self) -> Vec<String> {
    self.recipients.iter().map(|one| one.address.clone()).collect()
  }
}

/// Builds the message a draft sends, its attachments read from where they stand. A draft with no recipient,
/// and a message past the size the submission server takes, are refused.
pub async fn outgoing(
  source: &dyn PartSource,
  staging: &Path,
  limit: Option<u64>,
  draft: &Draft,
) -> Result<Outgoing, MailFault> {
  let recipients: Vec<_> = draft.to.iter().chain(&draft.cc).chain(&draft.bcc).cloned().collect();

  if recipients.is_empty() {
    return Err(Asked::rule("no-recipients", "a message to send has no recipient").into());
  }

  let attachments = resolve_attachments(source, staging, &draft.attachments).await?;
  let message_id = new_message_id(&draft.from);
  let at = chrono::Utc::now();
  let raw = build_message(draft, &attachments, Purpose::Send, Some(&message_id), at.timestamp())
    .map_err(MailFault::server)?;

  if let Some(limit) = limit
    && raw.len() as u64 > limit
  {
    return Err(Asked::too_large("the encoded message is past the size the server takes").into());
  }

  Ok(Outgoing {
    recipients,
    attachments,
    message_id,
    at,
    raw,
  })
}

/// The message a reply answered or a forward carried, by its message ID, and the flag it is marked with once
/// the draft is sent; none for a new message.
pub fn original_mark(draft: &Draft) -> Option<(&str, &'static str)> {
  let original = draft.original.as_deref()?;

  match draft.kind {
    DraftKind::New => None,
    DraftKind::Reply => Some((original, "\\Answered")),
    DraftKind::Forward => Some((original, "$Forwarded")),
  }
}

/// Sends the draft a `Message-ID` names now, read where it stands at that moment, so a change made while the
/// send waited goes with it; then keeps its copy in the sent folder, removes the draft and marks the message
/// it answered or carried. Answers the recipients.
pub async fn send_draft(
  session: &MailSession,
  smtp: &MailEndpoint,
  staging: &Path,
  limit: Option<u64>,
  draft_id: &str,
) -> Result<Vec<MailAddress>, MailFault> {
  let draft = draft_named(session, draft_id).await?;
  let at: MailRef = draft.saved.clone().ok_or_else(|| MailFault::server("the draft stands nowhere"))?;
  let going = outgoing(session, staging, limit, &draft).await?;

  submit(smtp, &session.account, &draft.from, &going.envelope(), &going.raw).await?;

  let copy = build_message(
    &draft,
    &going.attachments,
    Purpose::SentCopy,
    Some(&going.message_id),
    going.at.timestamp(),
  )
  .map_err(MailFault::server)?;
  let sent = ensure_role_folder(session, FolderRole::Sent).await?;
  let internal_date = going.at.format("%d-%b-%Y %H:%M:%S +0000").to_string();
  let mut connection = session.connection().await?;
  let appended = append_landing(connection.session(), &sent, "\\Seen", Some(&internal_date), &copy).await;

  connection.settle(appended)?;
  discard_draft(session, &at).await?;

  if let Some((original, flag)) = original_mark(&draft) {
    mark_original(session, original, flag).await?;
  }

  Ok(going.recipients)
}
