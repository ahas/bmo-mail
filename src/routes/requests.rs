//! What every mail route stands on (`docs/api.md`, 「Conventions」, 「Refusals」): the account the path
//! names, the answer a service with no accounts file gives, and the fields a request carries, checked before
//! anything is asked of the mail server. A fault of the request is refused by its code; anything else is the
//! mail server not answering.

use std::fmt::Display;
use std::future::Future;
use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::Response;
use serde_json::{Map, Value, json};

use crate::accounts::{MailAccount, MailConfig};
use crate::http::{Refused, Standing, answer};
use crate::imap::session::MailSession;
use crate::wire::{
  AttachmentRef, Draft, DraftKind, FolderRole, MailAddress, MailRef, MailSort, MailSortBy, MailView,
};

/// The code of an account key no account has.
pub const NO_SUCH_ACCOUNT: &str = "no_such_account";
/// The code of a mail server that did not answer, or failed.
pub const SERVER_SILENT: &str = "server_silent";

/// A fault of a request: its code, an English sentence for the log, and the values a screen fills its words
/// with.
#[derive(Debug)]
pub struct Asked {
  pub code: &'static str,
  pub error: String,
  pub values: Map<String, Value>,
}

impl Asked {
  pub fn new(code: &'static str, error: impl Into<String>) -> Self {
    Self {
      code,
      error: error.into(),
      values: Map::new(),
    }
  }

  /// The fault with one named value beside its code.
  pub fn with(mut self, name: &str, value: impl serde::Serialize) -> Self {
    self.values.insert(name.to_owned(), json!(value));
    self
  }
}

/// Why a route's work did not end in an answer.
#[derive(Debug)]
pub enum MailFault {
  /// The request asked for something wrong: refused 400 under its code.
  Asked(Asked),
  /// The mail server, or what stands behind it, failed: refused 502.
  Server(String),
}

impl MailFault {
  /// The request's fault under `code`.
  pub fn asked(code: &'static str, error: impl Into<String>) -> Self {
    Self::Asked(Asked::new(code, error))
  }

  /// A failure of the mail server or the store, in its own words.
  pub fn server(why: impl Display) -> Self {
    Self::Server(why.to_string())
  }
}

impl From<Asked> for MailFault {
  fn from(asked: Asked) -> Self {
    Self::Asked(asked)
  }
}

impl From<sqlx::Error> for MailFault {
  fn from(why: sqlx::Error) -> Self {
    Self::server(why)
  }
}

impl From<MailFault> for Refused {
  fn from(fault: MailFault) -> Self {
    match fault {
      MailFault::Asked(asked) => {
        let mut refused = Refused::new(StatusCode::BAD_REQUEST, asked.code, asked.error);

        refused.more = asked.values;
        refused
      }
      MailFault::Server(why) => {
        eprintln!("mail: the mail server did not answer: {why}");

        Refused::new(StatusCode::BAD_GATEWAY, SERVER_SILENT, "the mail server did not answer")
      }
    }
  }
}

/// What a route's work on an account stands on.
pub struct MailContext {
  /// What the service stands on.
  pub standing: Arc<Standing>,
  pub config: Arc<MailConfig>,
  pub account: MailAccount,
  pub session: Arc<MailSession>,
}

/// The answer every route gives where the service has no accounts file.
pub fn no_accounts() -> Response {
  answer(StatusCode::OK, json!({ "configured": false }))
}

/// An answer of a service with accounts: `configured` and the fields given, in their order.
pub fn configured(fields: Value) -> Response {
  let mut body = Map::new();

  body.insert("configured".into(), Value::Bool(true));

  if let Value::Object(fields) = fields {
    body.extend(fields);
  }

  answer(StatusCode::OK, Value::Object(body))
}

/// The route's work on the account its path names, or the no-accounts answer. A fault of the request is a
/// 400 under its code; a fault of the mail server is a 502.
pub async fn with_mail_account<Work, Done>(
  standing: &Arc<Standing>,
  key: Option<&str>,
  work: Work,
) -> Result<Response, Refused>
where
  Work: FnOnce(MailContext) -> Done,
  Done: Future<Output = Result<Response, MailFault>>,
{
  let Some(config) = standing.accounts.read() else {
    return Ok(no_accounts());
  };
  let Some(account) = key.and_then(|key| config.find_account(key)).cloned() else {
    return Err(Refused::new(StatusCode::NOT_FOUND, NO_SUCH_ACCOUNT, "no account has that key"));
  };
  let session = standing.sessions.session(&config.imap, &account);

  work(MailContext {
    standing: Arc::clone(standing),
    config,
    account,
    session,
  })
  .await
  .map_err(Refused::from)
}

/// A field's text, refused under `missing` where it is not there or empty.
fn text(value: Option<&Value>, missing: &'static str) -> Result<String, Asked> {
  match value {
    Some(Value::String(text)) if !text.is_empty() => Ok(text.clone()),
    _ => Err(Asked::new(
      missing,
      format!("the request names no {}", missing.trim_start_matches("missing_")),
    )),
  }
}

/// The largest whole number a request's number holds exactly.
const SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// A UID, given as a number or as the text of one: a positive whole number.
fn uid(value: Option<&Value>) -> Result<u32, Asked> {
  let number = match value {
    Some(Value::Number(number)) => number.as_f64(),
    Some(Value::String(text)) if text.trim().is_empty() => Some(0.0),
    Some(Value::String(text)) => text.trim().parse::<f64>().ok(),
    _ => None,
  };

  number
    .filter(|number| number.fract() == 0.0 && *number > 0.0 && *number <= SAFE_INTEGER)
    .and_then(|number| u32::try_from(number as u64).ok())
    .ok_or_else(|| Asked::new("bad_uid", "a uid is not a positive whole number"))
}

/// A message by its `folder` and `uid`.
pub fn ref_of(value: &Value) -> Result<MailRef, Asked> {
  Ok(MailRef {
    folder: text(value.get("folder"), "missing_folder")?,
    uid: uid(value.get("uid"))?,
  })
}

/// The messages a bulk action names: at least one.
pub fn refs_of(value: Option<&Value>) -> Result<Vec<MailRef>, Asked> {
  match value {
    Some(Value::Array(refs)) if !refs.is_empty() => refs.iter().map(ref_of).collect(),
    _ => Err(Asked::new("nothing_picked", "the request names no message")),
  }
}

/// The texts of a list, anything else in it passed over; no list is an empty one.
pub fn strings_of(value: Option<&Value>) -> Vec<String> {
  match value {
    Some(Value::Array(items)) => items.iter().filter_map(|item| item.as_str().map(str::to_owned)).collect(),
    _ => Vec::new(),
  }
}

/// What a list shows, from its `view` and `q`: a folder role (`inbox` where none is given), `starred`,
/// `label:<id>`, `folder:<path>`, or `search` with the query in `q`.
pub fn view_of(value: &Value) -> Result<MailView, Asked> {
  let view = value.get("view").and_then(Value::as_str).unwrap_or("inbox");

  if let Some(role) = FolderRole::named(view) {
    return Ok(MailView::Role(role));
  }

  if view == "starred" {
    return Ok(MailView::Starred);
  }

  if view == "search" {
    return Ok(MailView::Search(value.get("q").and_then(Value::as_str).unwrap_or("").to_owned()));
  }

  if let Some(id) = view.strip_prefix("label:") {
    return Ok(MailView::Label(text(Some(&json!(id)), "missing_label")?));
  }

  if let Some(path) = view.strip_prefix("folder:") {
    return Ok(MailView::Folder(text(Some(&json!(path)), "missing_folder")?));
  }

  Err(Asked::new("bad_view", "the view is none of the kinds a list shows"))
}

/// A label's colour, as the screen picks it: `#` and six hex digits and nothing else, since it is written
/// into the screen's own styles; kept in lower case.
pub fn color_of(value: Option<&Value>) -> Result<String, Asked> {
  let color = value.and_then(Value::as_str).unwrap_or("");
  let digits = color.strip_prefix('#').unwrap_or("");

  if digits.len() == 6 && digits.chars().all(|digit| digit.is_ascii_hexdigit()) {
    Ok(color.to_lowercase())
  } else {
    Err(Asked::new("bad_label_color", "a colour is not # and six hex digits"))
  }
}

/// How a list was asked to stand, from its `sort`, `dir` and `unread`. Anything else reads as the default,
/// since a list stands rather than refusing to be read.
pub fn sort_of(value: &Value) -> MailSort {
  let by = match value.get("sort").and_then(Value::as_str) {
    Some("from") => MailSortBy::From,
    Some("subject") => MailSortBy::Subject,
    _ => MailSortBy::Date,
  };
  let unread = matches!(value.get("unread"), Some(Value::Bool(true)))
    || value.get("unread").and_then(Value::as_str) == Some("1");

  MailSort {
    by,
    ascending: value.get("dir").and_then(Value::as_str) == Some("asc"),
    unread,
  }
}

/// Where a page starts: a whole number from 0, else 0.
pub fn offset_of(value: Option<&Value>) -> usize {
  let number = match value {
    None | Some(Value::Null) => Some(0.0),
    Some(Value::Number(number)) => number.as_f64(),
    Some(Value::String(text)) if text.trim().is_empty() => Some(0.0),
    Some(Value::String(text)) => text.trim().parse::<f64>().ok(),
    _ => None,
  };

  number
    .filter(|number| number.fract() == 0.0 && *number >= 0.0 && *number <= SAFE_INTEGER)
    .map_or(0, |number| number as usize)
}

/// Whether a text has the shape `<local>@<domain>`, neither part holding `@`, a space or an angle bracket.
fn address_shaped(address: &str) -> bool {
  let plain = |part: &str| {
    !part.is_empty() && !part.chars().any(|c| c == '@' || c == '<' || c == '>' || c.is_whitespace())
  };

  address.split_once('@').is_some_and(|(local, domain)| plain(local) && plain(domain))
}

fn addresses_of(value: Option<&Value>) -> Result<Vec<MailAddress>, Asked> {
  let Some(Value::Array(items)) = value else {
    return Ok(Vec::new());
  };

  items
    .iter()
    .map(|item| {
      let address = item.get("address").and_then(Value::as_str).unwrap_or("").trim().to_lowercase();

      if !address_shaped(&address) {
        return Err(Asked::new("bad_address", "an address is not one").with("address", &address));
      }

      let name = item.get("name").and_then(Value::as_str).unwrap_or("").trim().to_owned();

      Ok(MailAddress { name, address })
    })
    .collect()
}

fn attachments_of(value: Option<&Value>) -> Result<Vec<AttachmentRef>, Asked> {
  let Some(Value::Array(items)) = value else {
    return Ok(Vec::new());
  };

  items
    .iter()
    .map(|item| {
      let filename = item.get("filename").and_then(Value::as_str).unwrap_or("attachment").to_owned();
      let kind = item
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("application/octet-stream")
        .to_owned();
      let size = item.get("size").and_then(Value::as_f64).map_or(0, |size| size.max(0.0) as u64);

      match item.get("kind").and_then(Value::as_str) {
        Some("upload") => Ok(AttachmentRef::Upload {
          id: text(item.get("id"), "missing_attachment")?,
          filename,
          kind,
          size,
        }),
        Some("part") => {
          let MailRef { folder, uid } = ref_of(item)?;

          Ok(AttachmentRef::Part {
            folder,
            uid,
            part: text(item.get("part"), "missing_attachment")?,
            filename,
            kind,
            size,
          })
        }
        _ => Err(Asked::new("bad_attachment", "an attachment is neither an upload nor a part")),
      }
    })
    .collect()
}

/// Whether a text is a message ID in angle brackets.
fn message_id_shaped(text: &str) -> bool {
  text.strip_prefix('<').and_then(|rest| rest.strip_suffix('>')).is_some_and(|inner| {
    !inner.is_empty() && !inner.chars().any(|c| c == '<' || c == '>' || c.is_whitespace())
  })
}

/// A text on one line: each run of line breaks in it becomes one space.
fn one_line(text: &str) -> String {
  let mut line = String::with_capacity(text.len());
  let mut breaking = false;

  for c in text.chars() {
    if c == '\r' || c == '\n' {
      if !breaking {
        line.push(' ');
      }

      breaking = true;
    } else {
      line.push(c);
      breaking = false;
    }
  }

  line
}

/// A draft as the composer sent it, checked. It goes out from the account whose mailbox it is saved in, and
/// from nowhere else.
pub fn draft_of(value: &Value, account: &MailAccount) -> Result<Draft, Asked> {
  if let Some(from) = value.get("from").filter(|from| !from.is_null())
    && from.as_str() != Some(account.address.as_str())
  {
    return Err(Asked::new("sender_not_account", "a draft's from is not the account's address"));
  }

  let kind = match value.get("kind").and_then(Value::as_str) {
    Some("reply") => DraftKind::Reply,
    Some("forward") => DraftKind::Forward,
    _ => DraftKind::New,
  };
  let saved = match value.get("saved") {
    None | Some(Value::Null) => None,
    Some(saved) => Some(ref_of(saved)?),
  };
  let id = |field: &str| {
    value
      .get(field)
      .and_then(Value::as_str)
      .filter(|id| message_id_shaped(id))
      .map(str::to_owned)
  };
  let subject = value.get("subject").and_then(Value::as_str).unwrap_or("");

  Ok(Draft {
    from: account.address.clone(),
    to: addresses_of(value.get("to"))?,
    cc: addresses_of(value.get("cc"))?,
    bcc: addresses_of(value.get("bcc"))?,
    subject: one_line(subject),
    html: value.get("html").and_then(Value::as_str).unwrap_or("").to_owned(),
    attachments: attachments_of(value.get("attachments"))?,
    receipt: matches!(value.get("receipt"), Some(Value::Bool(true))),
    kind,
    original: id("original"),
    in_reply_to: id("inReplyTo"),
    references: strings_of(value.get("references"))
      .into_iter()
      .filter(|id| message_id_shaped(id))
      .collect(),
    saved,
  })
}
