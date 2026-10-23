//! What every mail route stands on (`docs/api.md`, 「Keys」, 「Refusals」): the owner's account the path names
//! and its mailbox, and the faults a route's work comes to. A fault of the request is refused under its status
//! and code; anything else is the mail server not answering.

use std::fmt::Display;
use std::future::Future;
use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::Response;

use crate::accounts::Account;
use crate::adapters::Mailbox;
use crate::http::{CONFLICT, Call, NOT_FOUND, Refused, Standing, TOO_LARGE, UPSTREAM_FAILED};
use crate::keys::message_ref;
use crate::mail::MailRef;

/// A fault of a request: its status, its code and an English sentence for the log.
#[derive(Debug)]
pub struct Asked {
  pub status: StatusCode,
  pub code: &'static str,
  pub error: String,
}

impl Asked {
  /// A request a rule of the service refuses: 422 under the rule's own code.
  pub fn rule(code: &'static str, error: impl Into<String>) -> Self {
    Self {
      status: StatusCode::UNPROCESSABLE_ENTITY,
      code,
      error: error.into(),
    }
  }

  /// A request whose body is not the shape the route takes.
  pub fn bad_request(error: impl Into<String>) -> Self {
    Self {
      status: StatusCode::BAD_REQUEST,
      code: crate::http::BAD_REQUEST,
      error: error.into(),
    }
  }

  /// Nothing stands at the key the path names.
  pub fn not_found(error: impl Into<String>) -> Self {
    Self {
      status: StatusCode::NOT_FOUND,
      code: NOT_FOUND,
      error: error.into(),
    }
  }

  /// The member's state does not allow the change, or the key is taken.
  pub fn conflict(error: impl Into<String>) -> Self {
    Self {
      status: StatusCode::CONFLICT,
      code: CONFLICT,
      error: error.into(),
    }
  }

  /// The service cannot do this yet: it has no key file to seal secrets with.
  pub fn not_ready(error: impl Into<String>) -> Self {
    Self {
      status: StatusCode::SERVICE_UNAVAILABLE,
      code: crate::http::NOT_READY,
      error: error.into(),
    }
  }

  /// A file or a message past the size the server takes.
  pub fn too_large(error: impl Into<String>) -> Self {
    Self {
      status: StatusCode::PAYLOAD_TOO_LARGE,
      code: TOO_LARGE,
      error: error.into(),
    }
  }
}

/// Why a route's work did not end in an answer.
#[derive(Debug)]
pub enum MailFault {
  /// The request asked for something wrong: refused under its own status and code.
  Asked(Asked),
  /// The mail server, or what stands behind it, failed: refused 502.
  Server(String),
}

impl MailFault {
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
      MailFault::Asked(asked) => Refused::new(asked.status, asked.code, asked.error),
      MailFault::Server(why) => {
        eprintln!("mail: the mail server did not answer: {why}");

        Refused::new(StatusCode::BAD_GATEWAY, UPSTREAM_FAILED, "the mail server did not answer")
      }
    }
  }
}

impl From<Refused> for MailFault {
  fn from(refused: Refused) -> Self {
    Self::Asked(Asked {
      status: refused.status,
      code: refused.code,
      error: refused.error,
    })
  }
}

/// What a route's work on an account stands on.
pub struct MailContext {
  /// What the service stands on.
  pub standing: Arc<Standing>,
  pub account: Account,
  /// The account's mailbox, which its adapter opened.
  pub mailbox: Arc<dyn Mailbox>,
}

impl MailContext {
  /// The account's name, as every member under it carries it.
  pub fn name(&self) -> &str {
    &self.account.name
  }

  /// The key the store holds the account's rows under.
  pub fn key(&self) -> String {
    self.account.store_key()
  }
}

/// The owner's account the path's `accountName` names. An account that does not stand, or is another
/// owner's, is 404.
pub async fn owned_account(call: &mut Call) -> Result<Account, Refused> {
  let owner = call.owner()?;
  let name = call.key("accountName").await?;

  call
    .standing()
    .accounts
    .find(&owner, &name)
    .await?
    .ok_or_else(|| Refused::not_found("no account has that name"))
}

/// The route's work on the owner's account its path's `accountName` names, through the account's mailbox. An
/// account that does not stand, or is another owner's, is 404; a fault of the request is refused under its
/// code and a fault of the mail server is a 502.
pub async fn with_account<Work, Done>(call: &mut Call, work: Work) -> Result<Response, Refused>
where
  Work: FnOnce(MailContext) -> Done,
  Done: Future<Output = Result<Response, MailFault>>,
{
  let account = owned_account(call).await?;
  let standing = Arc::clone(call.standing());
  let mailbox = standing.mailboxes.of(&account, &standing.context())?;

  work(MailContext {
    standing,
    account,
    mailbox,
  })
  .await
  .map_err(Refused::from)
}

/// The message a message id names, or 404.
pub fn message_at(id: &str) -> Result<MailRef, Asked> {
  message_ref(id).ok_or_else(|| Asked::not_found("no message has that id"))
}

/// Whether a text has the shape `<local>@<domain>`, neither part holding `@`, a space or an angle bracket.
pub fn address_shaped(address: &str) -> bool {
  let plain = |part: &str| {
    !part.is_empty() && !part.chars().any(|c| c == '@' || c == '<' || c == '>' || c.is_whitespace())
  };

  address.split_once('@').is_some_and(|(local, domain)| plain(local) && plain(domain))
}

/// A label's colour, as a screen picks it: `#` and six hex digits and nothing else, since a screen writes it
/// into its own styles; kept in lower case.
pub fn colour_of(colour: &str) -> Result<String, Asked> {
  let digits = colour.strip_prefix('#').unwrap_or("");

  if digits.len() == 6 && digits.chars().all(|digit| digit.is_ascii_hexdigit()) {
    Ok(colour.to_lowercase())
  } else {
    Err(Asked::bad_request("a colour is not # and six hex digits"))
  }
}

/// A text on one line: each run of line breaks in it becomes one space.
pub fn one_line(text: &str) -> String {
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
