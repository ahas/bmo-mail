//! The accounts and the summary of their inboxes (`docs/api.md`, 「Accounts and the summary」).

use axum::http::{Method, StatusCode};
use axum::response::Response;
use futures::future::join_all;

use crate::http::{Answer, Call, Operation, Refused, Route, Standing, answer, inline, schema};
use crate::imap::messages::inbox_unseen;
use crate::wire::{Account, ListQuery, Page, Summary};

/// Every account of the accounts file with its inbox's unread count, the accounts asked side by side; an
/// account whose server did not answer is unreachable rather than empty.
async fn accounts_now(standing: &Standing) -> Vec<Account> {
  let Some(config) = standing.accounts.read() else {
    return Vec::new();
  };
  let counts = config.accounts.iter().map(|account| async {
    let session = standing.sessions.session(&config.imap, account);

    inbox_unseen(&session).await.ok()
  });
  let counts = join_all(counts).await;

  config
    .accounts
    .iter()
    .zip(counts)
    .enumerate()
    .map(|(index, (account, unread))| Account {
      name: account.key.clone(),
      address: account.address.clone(),
      position: index as i64 + 1,
      unread_count: unread.unwrap_or(0),
      reachable: unread.is_some(),
    })
    .collect()
}

/// `GET /accounts`.
pub struct Accounts;

impl Route for Accounts {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/accounts";
  const OPERATION: Operation = Operation {
    id: "listAccounts",
    summary: "The mail accounts.",
    tag: "accounts",
    keys: &[],
    query: Some(inline::<ListQuery>),
    body: None,
    status: 200,
    answer: Answer::Json(schema::<Page<Account>>),
    codes: &[],
  };

  async fn answer(&self, call: Call) -> Result<Response, Refused> {
    let query: ListQuery = call.query()?;
    let accounts = accounts_now(call.standing()).await;

    Ok(answer(StatusCode::OK, &Page::by_position(accounts, |account| account.position, &query)))
  }
}

crate::route!(Accounts);

/// `GET /summary`.
pub struct SummaryRoute;

impl Route for SummaryRoute {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/summary";
  const OPERATION: Operation = Operation {
    id: "listSummary",
    summary: "Unread counts across the accounts.",
    tag: "summary",
    keys: &[],
    query: None,
    body: None,
    status: 200,
    answer: Answer::Json(schema::<Summary>),
    codes: &[],
  };

  async fn answer(&self, call: Call) -> Result<Response, Refused> {
    let accounts = accounts_now(call.standing()).await;
    let unread_count = accounts.iter().map(|account| account.unread_count).sum();

    Ok(answer(
      StatusCode::OK,
      &Summary {
        accounts,
        unread_count,
      },
    ))
  }
}

crate::route!(SummaryRoute);
