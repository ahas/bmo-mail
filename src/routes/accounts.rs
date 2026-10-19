//! The accounts and the summary (`docs/api.md`, 「Accounts and the summary」).

use axum::http::{Method, StatusCode};
use axum::response::Response;
use serde_json::json;

use crate::http::{Call, Refused, Route, answer};
use crate::imap::messages::inbox_summary;
use crate::labels::MAX_LABELS;
use crate::routes::requests::{configured, no_accounts};
use crate::wire::MailAccountSummary;

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

/// `GET /api/mail/summary`: every account's inbox at a glance, which a tab reads each time its stream opens
/// and each time the stream says an account changed (`docs/mail.md`, 「Reaching the open tabs」). An account
/// whose server does not answer is marked failed; the others still come back.
pub struct Summary;

impl Route for Summary {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/api/mail/summary";

  async fn answer(&self, call: Call) -> Result<Response, Refused> {
    let standing = call.standing();
    let Some(config) = standing.accounts.read() else {
      return Ok(answer(StatusCode::OK, json!({ "configured": false, "accounts": [] })));
    };
    let read = config.accounts.iter().map(|account| async {
      let session = standing.sessions.session(&config.imap, account);

      match inbox_summary(&session).await {
        Ok(summary) => MailAccountSummary {
          address: account.address.clone(),
          account: account.key.clone(),
          unread: summary.unread,
          newest: summary.newest,
          recent_unread: summary.recent_unread,
          failed: false,
        },
        Err(_) => MailAccountSummary {
          address: account.address.clone(),
          account: account.key.clone(),
          unread: 0,
          newest: None,
          recent_unread: Vec::new(),
          failed: true,
        },
      }
    });
    let accounts = futures::future::join_all(read).await;

    Ok(answer(StatusCode::OK, json!({ "configured": true, "accounts": accounts })))
  }
}

crate::route!(Summary);
