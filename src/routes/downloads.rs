//! The headers a part of a message is served with (`docs/mail.md`, 「Reading」; `docs/api.md`, 「Byte
//! answers」). Whatever a sender put in a part, it is never run in the console's own origin: the browser is
//! told not to guess its type, a document opened from it is sandboxed with nothing allowed, and an
//! attachment is handed over as a download.

use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

/// What a filename keeps unescaped in its UTF-8 form: the characters a URI component keeps.
const NAME_KEEPS: &AsciiSet = &NON_ALPHANUMERIC
  .remove(b'-')
  .remove(b'_')
  .remove(b'.')
  .remove(b'!')
  .remove(b'~')
  .remove(b'*')
  .remove(b'\'')
  .remove(b'(')
  .remove(b')');

/// How a part is handed over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
  Attachment,
  Inline,
}

impl Disposition {
  fn name(self) -> &'static str {
    match self {
      Self::Attachment => "attachment",
      Self::Inline => "inline",
    }
  }
}

/// A filename as a header carries it: an ASCII stand-in for old readers and the whole name encoded for the
/// rest.
pub fn disposition_header(disposition: Disposition, filename: Option<&str>) -> String {
  let name: String = filename
    .unwrap_or("attachment")
    .chars()
    .map(|c| if c == '\r' || c == '\n' || c == '"' { '_' } else { c })
    .collect();
  let ascii: String = name.chars().map(|c| if (' '..='~').contains(&c) { c } else { '_' }).collect();

  format!(
    "{}; filename=\"{ascii}\"; filename*=UTF-8''{}",
    disposition.name(),
    utf8_percent_encode(&name, NAME_KEEPS)
  )
}

/// Whether a type is a bare `type/subtype` of word characters, dots, pluses and dashes.
fn plain_type(kind: &str) -> bool {
  let word = |part: &str| {
    !part.is_empty()
      && part
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '+' || c == '-')
  };

  kind.split_once('/').is_some_and(|(main, sub)| word(main) && word(sub))
}

/// A part's bytes as an answer, with the headers that keep it out of the console's origin.
pub fn part_answer(
  bytes: Vec<u8>,
  kind: Option<&str>,
  filename: Option<&str>,
  disposition: Disposition,
) -> Response {
  let kind = kind.filter(|kind| plain_type(kind)).unwrap_or("application/octet-stream");
  let headers = [
    (header::CONTENT_TYPE, kind.to_owned()),
    (header::CONTENT_DISPOSITION, disposition_header(disposition, filename)),
    (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_owned()),
    (header::CONTENT_SECURITY_POLICY, "default-src 'none'; sandbox".to_owned()),
    (header::CACHE_CONTROL, "private, max-age=3600".to_owned()),
  ];
  let mut response = (StatusCode::OK, bytes).into_response();

  for (name, value) in headers {
    if let Ok(value) = HeaderValue::from_str(&value) {
      response.headers_mut().insert(name, value);
    }
  }

  response
}
