//! The page-token service, as the hosted tool asks it: a Graph access token lent for one account, on its
//! page token road (`docs/api.md`, 「The hosted tool」).

use serde::{Deserialize, Serialize};

use super::{Failure, Reach};
use crate::settings::Upstream;

/// The unit's name in a sentence.
const NAME: &str = "page-token service";

#[derive(Serialize)]
struct PageTokenAsk<'a> {
  account: u64,
  scopes: &'a [&'a str],
}

/// A person's Graph access token. It is used for the read it was asked for and goes nowhere else, so it is
/// never printed.
#[derive(Deserialize)]
pub struct PageToken {
  pub token: String,
}

/// The page token road, reached with this service's own caller token.
pub struct PageTokens(Reach);

impl PageTokens {
  pub fn new(upstream: &Upstream, http: reqwest::Client) -> Result<Self, Failure> {
    Reach::of(NAME, upstream, http).map(Self)
  }

  /// The token the service lends for `account`, holding one of `scopes`.
  pub async fn page_token(&self, account: &str, scopes: &[&str]) -> Result<PageToken, Failure> {
    let account = account
      .parse()
      .map_err(|_| Failure::Unreadable(format!("the account {account} is no number {NAME} takes")))?;

    self.0.post("/page-tokens", &PageTokenAsk { account, scopes }).await
  }
}
