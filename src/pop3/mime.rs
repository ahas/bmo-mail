//! A whole message read as an IMAP message's parts are (`docs/mail.md`, 「A POP3 account」): its MIME tree walked
//! by the MIME parser, each leaf numbered as IMAP numbers it and sorted by the same rule, its bodies decoded by
//! their charset and a part's bytes served by its number.

use mail_parser::{Message, MessageParser, MessagePart, MimeHeaders, PartType};

use crate::imap::charset::{decode_transfer, part_text};
use crate::imap::structure::{place, snippet_text};
use crate::mail::{MailPart, MailParts, OpenedPart};

/// The most of one body part a message's view reads. A body past it is shown cut.
const BODY_BYTES: usize = 2 * 1024 * 1024;

/// A whole message, read: its parts, its decoded bodies and the snippet a list shows of it.
#[derive(Debug, Default)]
pub struct Whole {
  pub parts: MailParts,
  pub text: Option<String>,
  pub html: Option<String>,
  pub snippet: String,
}

/// A leaf part of the message and its place in the raw bytes.
struct Leaf {
  part: MailPart,
  body: (usize, usize),
  disposition: Option<String>,
  related: bool,
}

fn parsed(raw: &[u8]) -> Option<Message<'_>> {
  MessageParser::new().with_mime_headers().parse(raw)
}

fn leaf(part: &MessagePart<'_>, number: String, related: bool) -> Leaf {
  let kind = part
    .content_type()
    .map(|kind| format!("{}/{}", kind.ctype(), kind.subtype().unwrap_or("plain")))
    .unwrap_or_else(|| "text/plain".to_owned())
    .to_lowercase();
  let one = MailPart {
    part: number,
    kind,
    charset: part.content_type().and_then(|kind| kind.attribute("charset")).map(str::to_owned),
    encoding: Some(
      part
        .content_transfer_encoding()
        .map(|encoding| encoding.trim().to_lowercase())
        .unwrap_or_else(|| "7bit".to_owned()),
    ),
    filename: part.attachment_name().map(str::to_owned),
    size: u64::from(part.raw_end_offset().saturating_sub(part.raw_body_offset())),
    cid: part
      .content_id()
      .map(|cid| cid.trim().trim_start_matches('<').trim_end_matches('>').to_owned()),
  };

  Leaf {
    part: one,
    body: (part.raw_body_offset() as usize, part.raw_end_offset() as usize),
    disposition: part.content_disposition().map(|disposition| disposition.ctype().to_lowercase()),
    related,
  }
}

/// Every leaf of the message in order, numbered as IMAP numbers them: a single-part message's body is part 1,
/// and a message carried inside another is one part.
fn leaves(message: &Message<'_>) -> Vec<Leaf> {
  fn walk(message: &Message<'_>, index: u32, number: Option<String>, related: bool, out: &mut Vec<Leaf>) {
    let Some(part) = message.part(index) else {
      return;
    };

    if let PartType::Multipart(children) = &part.body {
      let related = related || part.is_content_type("multipart", "related");

      for (place, child) in children.iter().enumerate() {
        let child_number = match &number {
          Some(number) => format!("{number}.{}", place + 1),
          None => (place + 1).to_string(),
        };

        walk(message, *child, Some(child_number), related, out);
      }

      return;
    }

    out.push(leaf(part, number.unwrap_or_else(|| "1".to_owned()), related));
  }

  let mut out = Vec::new();

  walk(message, 0, None, false, &mut out);
  out
}

fn body<'a>(raw: &'a [u8], leaf: &Leaf) -> &'a [u8] {
  let (start, end) = leaf.body;

  raw.get(start.min(end)..end.min(raw.len())).unwrap_or_default()
}

/// A whole message read: its parts, its bodies decoded up to the most a view reads, and its snippet.
pub fn read_whole(raw: &[u8]) -> Whole {
  let Some(message) = parsed(raw) else {
    return Whole::default();
  };
  let all = leaves(&message);
  let mut parts = MailParts::default();

  for one in &all {
    place(&mut parts, one.part.clone(), one.disposition.as_deref(), one.related);
  }

  let decoded = |wanted: &Option<MailPart>| {
    let wanted = wanted.as_ref()?;
    let found = all.iter().find(|one| one.part.part == wanted.part)?;
    let bytes = body(raw, found);
    let cut = &bytes[..bytes.len().min(BODY_BYTES)];

    Some(part_text(cut, wanted.encoding.as_deref(), wanted.charset.as_deref()))
  };
  let text = decoded(&parts.text);
  let html = decoded(&parts.html);
  let snippet = match (&text, &html) {
    (Some(text), _) => snippet_text(text, false),
    (None, Some(html)) => snippet_text(html, true),
    (None, None) => String::new(),
  };

  Whole {
    parts,
    text,
    html,
    snippet,
  }
}

/// A part of the message by its number, its transfer encoding undone; none where it has no such part.
pub fn open_part(raw: &[u8], number: &str) -> Option<OpenedPart> {
  let message = parsed(raw)?;
  let found = leaves(&message).into_iter().find(|one| one.part.part == number)?;

  Some(OpenedPart {
    bytes: decode_transfer(body(raw, &found), found.part.encoding.as_deref()),
    kind: Some(found.part.kind),
    filename: found.part.filename,
  })
}

/// The part of the message a content id names.
pub fn part_by_cid(raw: &[u8], cid: &str) -> Option<MailPart> {
  let wanted = cid.trim_start_matches('<').trim_end_matches('>');
  let message = parsed(raw)?;

  leaves(&message)
    .into_iter()
    .map(|one| one.part)
    .find(|part| part.cid.as_deref().is_some_and(|cid| cid.eq_ignore_ascii_case(wanted)))
}
