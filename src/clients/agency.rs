//! Agency, as the hosted tool asks it (`docs/mail.md`, 「A person's sent mail, read on their own machine」):
//! the account a caller acts for, and the node agency's record names for an account.

use serde::Deserialize;
use serde_json::Value;

use super::{Failure, Reach, account_id};
use crate::client::segment;
use crate::settings::Upstream;

/// The unit's name in a sentence.
const NAME: &str = "agency";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Owned {
  #[serde(default)]
  owner_account: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RoleAgent {
  name: String,
  #[serde(default)]
  owner_account: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersonNodeRecord {
  owner_account: Value,
  node: String,
  #[serde(default)]
  shared: bool,
}

#[derive(Deserialize)]
struct Items<T> {
  items: Vec<T>,
}

/// The node agency's record names for an account.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersonNode {
  pub node: String,
  /// Whether the record shares the node with other accounts.
  pub shared: bool,
}

/// Agency's control routes the tool reads.
pub struct Agency(Reach);

impl Agency {
  pub fn new(upstream: &Upstream, http: reqwest::Client) -> Result<Self, Failure> {
    Reach::of(NAME, upstream, http).map(Self)
  }

  /// The account `agent` acts for: the owner of the supervisor of that name, else of the role agent of that
  /// name; none where neither names one.
  pub async fn account_of(&self, agent: &str) -> Result<Option<String>, Failure> {
    match self.0.get::<Owned>(&format!("/supervisors/{}", segment(agent))).await {
      Ok(supervisor) => return Ok(account_id(&supervisor.owner_account)),
      Err(Failure::Refused { status: 404, .. }) => {}
      Err(failure) => return Err(failure),
    }

    let agents: Items<RoleAgent> = self.0.get("/role-agents").await?;

    Ok(
      agents
        .items
        .iter()
        .find(|role_agent| role_agent.name == agent)
        .and_then(|role_agent| account_id(&role_agent.owner_account)),
    )
  }

  /// The node agency's record names for `account`, compared as the id written out; none where no record
  /// names one.
  pub async fn person_node(&self, account: &str) -> Result<Option<PersonNode>, Failure> {
    let records: Items<PersonNodeRecord> = self.0.get("/person-nodes").await?;

    Ok(
      records
        .items
        .into_iter()
        .find(|record| account_id(&record.owner_account).as_deref() == Some(account))
        .map(|record| PersonNode {
          node: record.node,
          shared: record.shared,
        }),
    )
  }
}
