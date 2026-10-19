//! What a message is made of, read from the body structure the server reports, without fetching a byte of
//! it: the one plain body and the one HTML body the screen shows, the parts listed as attachments, and the
//! inline parts an HTML body points at by `cid:`.

use std::borrow::Cow;

use async_imap::imap_proto::{BodyContentCommon, BodyContentSinglePart, BodyStructure, ContentEncoding};
use mail_parser::{MessageParser, MimeHeaders};
use scraper::{Html, Node};

use crate::wire::{MailPart, MailParts};

/// The parameters of a part's header as the body structure lists them.
type Parameters<'a> = Option<Vec<(Cow<'a, str>, Cow<'a, str>)>>;

/// One parameter of a header, its encoded words (RFC 2047) and its continuations and charset (RFC 2231)
/// read: the header is written back from the parameters and read by the MIME parser.
fn parameter(header: &str, parameters: &Parameters<'_>, wanted: &str) -> Option<String> {
  let parameters = parameters.as_ref()?;

  if !parameters.iter().any(|(name, _)| {
    let name = name.to_ascii_lowercase();

    name == wanted || name.starts_with(&format!("{wanted}*"))
  }) {
    return None;
  }

  let mut line = format!("{header}: x/x");

  for (name, value) in parameters {
    if name.ends_with('*') {
      line.push_str(&format!("; {name}={value}"));
    } else {
      line.push_str(&format!("; {name}=\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\"")));
    }
  }

  line.push_str("\r\n\r\n");

  let parsed = MessageParser::new().with_mime_headers().parse_headers(line.as_bytes())?;
  let part = parsed.root_part();
  let read = if header == "Content-Disposition" {
    part.content_disposition()
  } else {
    part.content_type()
  };

  read?.attribute(wanted).map(str::to_owned)
}

fn encoding_name(encoding: &ContentEncoding<'_>) -> String {
  match encoding {
    ContentEncoding::SevenBit => "7bit".to_owned(),
    ContentEncoding::EightBit => "8bit".to_owned(),
    ContentEncoding::Binary => "binary".to_owned(),
    ContentEncoding::Base64 => "base64".to_owned(),
    ContentEncoding::QuotedPrintable => "quoted-printable".to_owned(),
    ContentEncoding::Other(name) => name.to_lowercase(),
  }
}

fn leaf(common: &BodyContentCommon<'_>, other: &BodyContentSinglePart<'_>, part: String) -> MailPart {
  let disposition = common.disposition.as_ref().map(|disposition| &disposition.params);
  let filename = disposition
    .and_then(|params| parameter("Content-Disposition", params, "filename"))
    .or_else(|| parameter("Content-Type", &common.ty.params, "name"));
  let charset = common.ty.params.as_ref().and_then(|params| {
    params
      .iter()
      .find(|(name, _)| name.eq_ignore_ascii_case("charset"))
      .map(|(_, value)| value.to_string())
  });
  let cid = other.id.as_ref().map(|id| id.trim_start_matches('<').trim_end_matches('>').to_owned());

  MailPart {
    part,
    kind: format!("{}/{}", common.ty.ty, common.ty.subtype).to_lowercase(),
    charset,
    encoding: Some(encoding_name(&other.transfer_encoding)),
    filename,
    size: u64::from(other.octets),
    cid,
  }
}

/// What a message is made of, sorted. A single-part message has no part number of its own; its body is
/// part 1. A message carried inside another is one part, an attachment.
pub fn message_parts(structure: Option<&BodyStructure<'_>>) -> MailParts {
  let mut parts = MailParts::default();

  fn walk(node: &BodyStructure<'_>, number: Option<String>, related: bool, parts: &mut MailParts) {
    let (common, other) = match node {
      BodyStructure::Multipart { common, bodies, .. } => {
        let related = related
          || format!("{}/{}", common.ty.ty, common.ty.subtype).eq_ignore_ascii_case("multipart/related");

        for (index, child) in bodies.iter().enumerate() {
          let child_number = match &number {
            Some(number) => format!("{number}.{}", index + 1),
            None => (index + 1).to_string(),
          };

          walk(child, Some(child_number), related, parts);
        }

        return;
      }
      BodyStructure::Basic { common, other, .. }
      | BodyStructure::Text { common, other, .. }
      | BodyStructure::Message { common, other, .. } => (common, other),
    };
    let one = leaf(common, other, number.unwrap_or_else(|| "1".to_owned()));
    let disposition = common.disposition.as_ref().map(|disposition| disposition.ty.to_lowercase());
    let attached = disposition.as_deref() == Some("attachment");

    if !attached && one.filename.is_none() && one.kind == "text/plain" && parts.text.is_none() {
      parts.text = Some(one);
    } else if !attached && one.filename.is_none() && one.kind == "text/html" && parts.html.is_none() {
      parts.html = Some(one);
    } else if !attached
      && one.cid.is_some()
      && (related || disposition.as_deref() == Some("inline"))
      && one.kind.starts_with("image/")
    {
      parts.inline.push(one);
    } else {
      parts.attachments.push(one);
    }
  }

  if let Some(structure) = structure {
    walk(structure, None, false, &mut parts);
  }

  parts
}

/// The part a snippet is read from: the plain body, else the HTML one.
pub fn snippet_part(structure: Option<&BodyStructure<'_>>) -> Option<MailPart> {
  let parts = message_parts(structure);

  parts.text.or(parts.html)
}

/// The words of an HTML body, its styles, scripts and head left out.
fn html_words(html: &str) -> String {
  let document = Html::parse_fragment(html);
  let mut words = Vec::new();

  for node in document.root_element().descendants() {
    let Node::Text(text) = node.value() else {
      continue;
    };
    let hidden = node.ancestors().any(|ancestor| {
      ancestor
        .value()
        .as_element()
        .is_some_and(|element| matches!(element.name(), "style" | "script" | "head"))
    });

    if !hidden {
      words.push(text.to_string());
    }
  }

  words.join(" ")
}

/// The most characters a snippet holds.
const SNIPPET_CHARACTERS: usize = 200;

/// A line of the body, for a list row: markup read for its words, quoted lines taken out, spaces folded.
pub fn snippet_text(text: &str, html: bool) -> String {
  let plain = if html { html_words(text) } else { text.to_owned() };
  let kept: Vec<&str> = plain.split('\n').filter(|line| !line.trim_start().starts_with('>')).collect();
  let folded = kept.join(" ").split_whitespace().collect::<Vec<_>>().join(" ");

  folded.chars().take(SNIPPET_CHARACTERS).collect()
}
