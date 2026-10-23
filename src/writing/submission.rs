//! Submission (`docs/mail.md`, 「Writing」): the IMAP/SMTP adapter's sending. A message is handed to the
//! submission server as the account, the size the server takes is asked of it, and a draft sent has its copy
//! kept in the sent folder, the draft removed and the message it answered or carried marked.

use std::path::Path;
use std::time::Duration;

use mail_send::SmtpClientBuilder;
use mail_send::smtp::message::Message;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::adapters::imap_smtp::{MailAccount, MailEndpoint};
use crate::imap::folders::{ensure_role_folder, folders_of, searched_folders};
use crate::imap::search::Search;
use crate::imap::session::MailSession;
use crate::imap::{append_landing, store_flags, uid_search};
use crate::mail::{DraftKind, FolderRole, MailAddress, MailRef};
use crate::routes::requests::{Asked, MailFault};
use crate::writing::compose::{Purpose, build_message, new_message_id};
use crate::writing::drafts::{discard_draft, draft_named, resolve_attachments};

/// The submission port that speaks TLS from the first byte; any other with a TLS name is upgraded with
/// STARTTLS.
const IMPLICIT_TLS_PORT: u16 = 465;
/// How long a conversation with the submission server may take.
const SUBMISSION_TIMEOUT: Duration = Duration::from_secs(60);

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

/// Signs in to the submission server as the account and lets go, sending nothing.
pub async fn sign_in(endpoint: &MailEndpoint, account: &MailAccount) -> Result<(), MailFault> {
  let builder = builder(endpoint)?.credentials((account.login.clone(), account.password.clone()));

  if endpoint.tls_name.is_none() {
    let _ = builder.connect_plain().await.map_err(MailFault::server)?.quit().await;
  } else {
    let _ = builder.connect().await.map_err(MailFault::server)?.quit().await;
  }

  Ok(())
}

async fn advertised<T: AsyncRead + AsyncWrite + Unpin>(
  mut client: mail_send::SmtpClient<T>,
  local_host: &str,
) -> Option<u64> {
  let size = client.capabilities(local_host, false).await.ok().map(|said| said.size as u64);
  let _ = client.quit().await;

  size.filter(|size| *size > 0)
}

/// The largest message the submission server says it takes (its `SIZE`), or none where it names no limit or
/// does not answer. Asking needs no login.
pub async fn submission_limit(endpoint: &MailEndpoint) -> Option<u64> {
  let builder = builder(endpoint).ok()?.say_ehlo(false);

  if endpoint.tls_name.is_none() {
    advertised(builder.connect_plain().await.ok()?, &builder.local_host).await
  } else {
    advertised(builder.connect().await.ok()?, &builder.local_host).await
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
  let recipients: Vec<_> = draft.to.iter().chain(&draft.cc).chain(&draft.bcc).cloned().collect();

  if recipients.is_empty() {
    return Err(Asked::rule("no-recipients", "a message to send has no recipient").into());
  }

  let attachments = resolve_attachments(session, staging, &draft.attachments).await?;
  let message_id = new_message_id(&draft.from);
  let now = chrono::Utc::now();
  let raw = build_message(&draft, &attachments, Purpose::Send, Some(&message_id), now.timestamp())
    .map_err(MailFault::server)?;

  if let Some(limit) = limit
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

  Ok(recipients)
}
