//! The accounts (`docs/api.md`, 「Accounts and the summary」).

use axum::http::Method;
use axum::response::Response;
use serde_json::json;

use crate::http::{Call, Refused, Route};
use crate::labels::MAX_LABELS;
use crate::routes::requests::{configured, no_accounts};

/// `GET /api/mail/accounts`: the mail accounts, by their addresses alone: nothing else of the accounts file
/// ever reaches the browser (`docs/mail.md`, 「Accounts and credentials」).
pub struct Accounts;

impl Route for Accounts {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/api/mail/accounts";

  async fn answer(&self, call: Call) -> Result<Response, Refused> {
    let Some(config) = call.standing().accounts.read() else {
      return Ok(no_accounts());
    };

    Ok(configured(json!({ "accounts": config.public_accounts(), "maxLabels": MAX_LABELS })))
  }
}

crate::route!(Accounts);
