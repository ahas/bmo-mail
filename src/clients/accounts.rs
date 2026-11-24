//! The account service (bmo-web's `docs/boundary.md`), as this repository asks it: for the hosted tool, the
//! account a sender is linked to (`GET /senders/{sender}/user`); for the service, its mail servers' delegation
//! chain — the kind `mail.servers` declared, resolved for a person, a link's entry read and its value written in a
//! person's name — and the names of the units on a person's path (`GET /users/{user}/unit`).

use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Failure, Reach};
use crate::settings::Upstream;

/// The unit's name in a sentence.
const NAME: &str = "the account service";
/// The unit this service declares its chain's kinds as.
const DECLARED_BY: &str = "mail";

#[derive(Deserialize)]
struct Linked {
  user: Value,
}

/// A chain's kind as this service declares it.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Kind {
  pub key: &'static str,
  pub declared_by: &'static str,
  pub description: &'static str,
  pub shape: &'static str,
  pub choices: Vec<String>,
  pub mode: &'static str,
  pub rules: bool,
  pub levels: Vec<&'static str>,
  pub unit_permission: &'static str,
  pub person_writer: &'static str,
}

/// What the chain resolves for a person: every link of their chain, top first, and the link whose lock cuts it.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Resolved {
  #[serde(default)]
  pub path: Vec<String>,
  #[serde(default)]
  pub locked_by: Option<String>,
}

/// A link's entry, as the person asking reads it.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
  /// Whether the person may change the link now.
  pub writable: bool,
}

/// A unit as the account service names it.
#[derive(Clone, Debug, Deserialize)]
pub struct UnitRef {
  pub id: Value,
  pub name: String,
}

/// A person's unit, with the units from their company down to it.
#[derive(Clone, Debug, Deserialize)]
pub struct UserUnit {
  #[serde(default)]
  pub path: Vec<UnitRef>,
}

/// The account service's sender links and delegation chain.
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

  /// Declares a kind of the chain.
  pub async fn declare(&self, kind: &Kind) -> Result<(), Failure> {
    self
      .0
      .call::<Value>(Method::PUT, &format!("/chain/kinds/{}", kind.key), &[], Some(&json!(kind)))
      .await
      .map(drop)
  }

  /// What the chain resolves for a person; none where the person does not stand.
  pub async fn resolve(&self, key: &str, person: &str) -> Result<Option<Resolved>, Failure> {
    let path = format!("/chain/kinds/{key}/resolve?person={}", encoded(person));

    match self.0.get::<Resolved>(&path).await {
      Ok(resolved) => Ok(Some(resolved)),
      Err(Failure::Refused {
        status: 404,
        answer,
        ..
      }) if answer.as_ref().is_none_or(|answer| answer.code != "no-such-kind") => Ok(None),
      Err(failure) => Err(failure),
    }
  }

  /// A link's entry as the person `owner` names reads it.
  pub async fn entry(&self, key: &str, link: &str, owner: &str) -> Result<Entry, Failure> {
    let path = format!("/chain/kinds/{key}/links/{}", encoded(link));

    self.0.call(Method::GET, &path, &[("owner-key", owner)], None).await
  }

  /// Writes a link's value in the name of the person `owner` names, as the unit that declared the kind.
  pub async fn write(&self, key: &str, link: &str, owner: &str, value: &Value) -> Result<(), Failure> {
    let path = format!("/chain/kinds/{key}/links/{}/value", encoded(link));
    let headers = [("owner-key", owner), ("declared-by", DECLARED_BY)];

    self
      .0
      .call::<Value>(Method::PUT, &path, &headers, Some(&json!({ "value": value })))
      .await
      .map(drop)
  }

  /// A person's unit and the units above it; none where the person does not stand.
  pub async fn unit_of(&self, user: &str) -> Result<Option<UserUnit>, Failure> {
    match self.0.get::<UserUnit>(&format!("/users/{}/unit", encoded(user))).await {
      Ok(unit) => Ok(Some(unit)),
      Err(Failure::Refused { status: 404, .. }) => Ok(None),
      Err(failure) => Err(failure),
    }
  }
}

/// A key written into a path or a query, percent-encoded.
fn encoded(key: &str) -> String {
  percent_encoding::utf8_percent_encode(key, percent_encoding::NON_ALPHANUMERIC).to_string()
}
