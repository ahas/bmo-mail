//! Which conversation a message belongs to (`docs/mail.md`, 「Reading」): the first message ID its
//! `References` names, else its `In-Reply-To`, else its own `Message-ID`. Every reply a conforming mailer
//! writes carries the first message of the conversation first in `References`, so this names the
//! conversation's opening message without asking the server to thread anything.

use std::sync::LazyLock;

use regex::Regex;

/// A message ID as a key: in angle brackets and in lower case.
pub fn id_key(id: &str) -> String {
  format!("<{}>", id.trim().trim_start_matches('<').trim_end_matches('>').to_lowercase())
}

/// The conversation key, or none for a message that carries no message ID at all; such a message is a
/// conversation of its own, which the caller names by its place.
pub fn conversation_key(
  message_id: Option<&str>,
  in_reply_to: &[String],
  references: &[String],
) -> Option<String> {
  references
    .first()
    .or(in_reply_to.first())
    .map(String::as_str)
    .or(message_id)
    .map(id_key)
}

/// The prefixes a mailer stacks in front of a subject as it replies or forwards, in Korean and English
/// alike, with a count such as `[2]` and the colon.
static PREFIX: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"(?i)^(re|fw|fwd|답장|전달|회신)\s*(\[\d+\])?\s*:\s*").expect("the prefix pattern reads")
});

/// The subject a conversation is known by: the reply and forward prefixes taken off.
pub fn base_subject(subject: &str) -> String {
  let mut rest = subject.trim().to_owned();

  loop {
    let next = PREFIX.replace(&rest, "").into_owned();

    if next == rest {
      return rest;
    }

    rest = next;
  }
}
