//! The mail push (`docs/mail.md`, 「Push」): one notification per account per arrival, naming the newest
//! message's sender and subject and how many came, and opening that message's conversation. A second
//! arrival for the same account replaces the first on the screen, as a mailbox reads on a phone. The
//! service writes it in its own words; ringing it is the console's.

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Serialize;

use crate::watcher::arrivals::MailArrival;
use crate::wording::{Wording, fill};

/// The notification kind mail is.
pub const MAIL_PUSH_KIND: &str = "mail";

/// The most characters a notification's body holds, as a browser counts them.
const BODY_LIMIT: usize = 160;

/// What a path segment keeps unescaped: the characters a URI component keeps.
const SEGMENT_KEEPS: &AsciiSet = &NON_ALPHANUMERIC
  .remove(b'-')
  .remove(b'_')
  .remove(b'.')
  .remove(b'!')
  .remove(b'~')
  .remove(b'*')
  .remove(b'\'')
  .remove(b'(')
  .remove(b')');

/// One notification.
#[derive(Clone, Debug, Serialize)]
pub struct MailPush {
  pub kind: &'static str,
  pub title: String,
  pub body: String,
  /// The newest message's thread, under the console's base path.
  pub path: String,
  /// What the notification is about, so a second one for the account replaces the first.
  pub tag: String,
}

/// A notification's body on one line, cut to what a notification shows.
fn clip_body(text: &str) -> String {
  let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");

  if flat.encode_utf16().count() <= BODY_LIMIT {
    return flat;
  }

  let mut clipped = String::new();
  let mut taken = 0;

  for c in flat.chars() {
    taken += c.len_utf16();

    if taken > BODY_LIMIT - 1 {
      break;
    }

    clipped.push(c);
  }

  format!("{clipped}…")
}

/// A count with its thousands grouped.
fn grouped(count: usize) -> String {
  let digits = count.to_string();
  let mut out = String::new();

  for (index, digit) in digits.chars().enumerate() {
    if index > 0 && (digits.len() - index) % 3 == 0 {
      out.push(',');
    }

    out.push(digit);
  }

  out
}

/// The notification an arrival makes, or none where no message came.
pub fn mail_push(arrival: &MailArrival, wording: &Wording) -> Option<MailPush> {
  let newest = arrival.messages.iter().max_by_key(|message| message.date)?;
  let words = &wording.arrival;
  let who = match newest.from.first() {
    None => words.no_sender.clone(),
    Some(sender) if sender.name.is_empty() => sender.address.clone(),
    Some(sender) => sender.name.clone(),
  };
  let count = arrival.messages.len();
  let title = if count == 1 {
    fill(&words.title_one, &[("address", &arrival.address)])
  } else {
    fill(&words.title_many, &[("n", &grouped(count)), ("address", &arrival.address)])
  };
  let subject = if newest.subject.trim().is_empty() {
    words.no_subject.clone()
  } else {
    newest.subject.clone()
  };
  let path = format!(
    "/mail/{}/inbox/{}",
    utf8_percent_encode(&arrival.account, SEGMENT_KEEPS),
    utf8_percent_encode(&newest.key, SEGMENT_KEEPS)
  );

  Some(MailPush {
    kind: MAIL_PUSH_KIND,
    title,
    body: clip_body(&fill(&words.body, &[("who", &who), ("subject", &subject)])),
    path,
    tag: format!("mail:{}", arrival.account),
  })
}
