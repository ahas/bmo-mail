//! Read receipts answered (`docs/mail.md`, 「Read receipts」). Nothing is ever sent without the reader's
//! press: sending writes a disposition notification (RFC 8098) from the account the message came to, and
//! either answer sets `$MDNSent`, so the question is asked once.

use mail_builder::MessageBuilder;
use mail_builder::headers::address::Address;
use mail_builder::headers::content_type::ContentType;
use mail_builder::mime::{BodyPart, MimePart};
use rand::RngCore;

use crate::accounts::MailEndpoint;
use crate::imap::messages::{change_flags, read_message};
use crate::imap::session::MailSession;
use crate::routes::requests::MailFault;
use crate::wire::{MailMessage, MailRef};
use crate::wording::{Wording, fill};
use crate::writing::send::submit;

/// What the report names the service as.
const REPORTING_AGENT: &str = "bmo-mail";

fn bare(id: &str) -> String {
  id.trim().trim_start_matches('<').trim_end_matches('>').to_owned()
}

/// The disposition notification for a message that asked for one: a line for a person to read, and the
/// machine-readable report.
pub fn build_receipt(
  original: &MailMessage,
  account: &str,
  wording: &Wording,
  date: i64,
) -> Result<Vec<u8>, MailFault> {
  let Some(receipt_to) = &original.receipt_to else {
    return Err(MailFault::asked("no_receipt_asked", "the message asks for no read receipt"));
  };
  let subject = &original.listed.subject;
  let human = format!("{}\r\n", fill(&wording.receipt.body, &[("account", account), ("subject", subject)]));
  let mut report =
    vec![format!("Reporting-UA: {REPORTING_AGENT}"), format!("Final-Recipient: rfc822;{account}")];

  if let Some(id) = &original.listed.message_id {
    report.push(format!("Original-Message-ID: {id}"));
  }

  report.push("Disposition: manual-action/MDN-sent-manually; displayed".to_owned());
  report.push(String::new());

  let mut random = [0u8; 12];

  rand::rng().fill_bytes(&mut random);

  let domain = account.split_once('@').map_or("localhost", |(_, domain)| domain);
  let message_id =
    format!("{}@{domain}", random.iter().map(|byte| format!("{byte:02x}")).collect::<String>());
  let body = MimePart::new(
    ContentType::new("multipart/report").attribute("report-type", "disposition-notification"),
    vec![
      MimePart::new("text/plain", BodyPart::Text(human.into())),
      MimePart::new("message/disposition-notification", BodyPart::Text(report.join("\r\n").into())),
    ],
  );
  let mut message = MessageBuilder::new()
    .from(Address::from(account.to_owned()))
    .to(Address::from(receipt_to.clone()))
    .subject(fill(&wording.receipt.subject, &[("subject", subject)]))
    .date(date)
    .message_id(message_id);

  if let Some(id) = &original.listed.message_id {
    message = message.in_reply_to(bare(id)).references(bare(id));
  }

  message.body(body).write_to_vec().map_err(MailFault::server)
}

/// Send or don't send, on a message's read-receipt bar.
pub async fn answer_receipt(
  session: &MailSession,
  smtp: &MailEndpoint,
  wording: &Wording,
  at: &MailRef,
  send: bool,
) -> Result<(), MailFault> {
  let Some(message) = read_message(session, at).await? else {
    return Err(MailFault::asked("message_not_found", "the message is not on the server"));
  };

  if send {
    let account = &session.account.address;
    let raw = build_receipt(&message, account, wording, chrono::Utc::now().timestamp())?;
    let to = message.receipt_to.clone().into_iter().collect::<Vec<_>>();

    submit(smtp, &session.account, account, &to, &raw).await?;
  }

  change_flags(session, std::slice::from_ref(at), &["$MDNSent".to_owned()], &[]).await
}
