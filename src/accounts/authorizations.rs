//! Authorizations under way (`docs/api.md`, 「Accounts」): an account of a kind that signs in through its
//! provider's page is added in two steps, and what the adapter needs between them waits here, for the owner
//! who started it alone, for ten minutes. Nothing of it is kept across a restart: an authorization a restart
//! lost is started again.

use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;
use std::time::Duration;

use rand::RngCore;

use crate::wire::Fields;

/// How long an authorization waits for the browser to come back.
pub const AUTHORIZATION_SPAN: Duration = Duration::from_secs(10 * 60);

/// An authorization waiting for its answer.
#[derive(Clone)]
pub struct Waiting {
  pub owner: String,
  pub kind: String,
  /// What the adapter keeps between the two steps, the provider's code verifier among it: never answered.
  pub kept: Fields,
  pub expires_at: i64,
}

/// The authorizations waiting.
#[derive(Default)]
pub struct Authorizations {
  waiting: Mutex<HashMap<String, Waiting>>,
}

impl fmt::Debug for Authorizations {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.debug_struct("Authorizations").finish_non_exhaustive()
  }
}

impl Authorizations {
  fn waiting(&self) -> std::sync::MutexGuard<'_, HashMap<String, Waiting>> {
    self.waiting.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
  }

  /// A fresh id, random enough to stand as the provider's state.
  pub fn new_id() -> String {
    let mut random = [0u8; 18];

    rand::rng().fill_bytes(&mut random);
    random.iter().map(|byte| format!("{byte:02x}")).collect()
  }

  /// Holds an authorization under its id, letting go of every one that has run out.
  pub fn hold(&self, id: String, waiting: Waiting) {
    let now = chrono::Utc::now().timestamp_millis();
    let mut held = self.waiting();

    held.retain(|_, one| one.expires_at > now);
    held.insert(id, waiting);
  }

  /// Takes the owner's authorization of that kind out, where it is the owner's, of that kind and still runs;
  /// it answers once.
  pub fn take(&self, owner: &str, kind: &str, id: &str) -> Option<Waiting> {
    let now = chrono::Utc::now().timestamp_millis();
    let mut held = self.waiting();
    let fits = held
      .get(id)
      .is_some_and(|one| one.owner == owner && one.kind == kind && one.expires_at > now);

    if fits { held.remove(id) } else { None }
  }
}
