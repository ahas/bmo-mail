//! The receiver, as the hosted tool asks it: the owner as a sender, which the owner's account is read from for a
//! caller that acts for no account (the receiver's `docs/api.md`, `GET /owner`).

use serde::Deserialize;

use super::{Failure, Reach};
use crate::settings::Upstream;

/// The unit's name in a sentence.
const NAME: &str = "the receiver";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Owner {
  sender_id: i64,
}

/// The receiver's owner route.
pub struct Receiver(Reach);

impl Receiver {
  pub fn new(upstream: &Upstream, http: reqwest::Client) -> Result<Self, Failure> {
    Reach::of(NAME, upstream, http).map(Self)
  }

  /// The owner's sender number.
  pub async fn owner_sender(&self) -> Result<i64, Failure> {
    self.0.get::<Owner>("/owner").await.map(|owner| owner.sender_id)
  }
}
