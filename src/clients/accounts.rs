//! The account service, as the hosted tool asks it: the account a sender is linked to (bmo-web's
//! `docs/boundary.md`, `GET /senders/{sender}/user`).

use serde::Deserialize;
use serde_json::Value;

use super::{Failure, Reach};
use crate::settings::Upstream;

/// The unit's name in a sentence.
const NAME: &str = "the account service";

#[derive(Deserialize)]
struct Linked {
  user: Value,
}

/// The account service's sender links.
pub struct Accounts(Reach);

impl Accounts {
  pub fn new(upstream: &Upstream, http: reqwest::Client) -> Result<Self, Failure> {
    Reach::of(NAME, upstream, http).map(Self)
  }

  /// The account `sender` is linked to, its id written out; none where it is linked to none.
  pub async fn user_of(&self, sender: i64) -> Result<Option<String>, Failure> {
    match self.0.get::<Linked>(&format!("/senders/{sender}/user")).await {
      Ok(linked) => Ok(super::account_id(&linked.user)),
      Err(Failure::Refused { status: 404, .. }) => Ok(None),
      Err(failure) => Err(failure),
    }
  }
}
