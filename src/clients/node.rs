//! The node service, as the hosted tool asks it: one run on a person's own node, made as the caller `bmo-mail`
//! for that person's account (the node service's `docs/api.md`, 「Runs」).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{Failure, Reach};
use crate::client::segment;
use crate::settings::Upstream;

/// The unit's name in a sentence.
const NAME: &str = "the node service";

/// A command to run on a node for an account.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunAsk<'a> {
  /// The account the run is for, its id written out.
  pub account: &'a str,
  pub command: &'a str,
  /// The shell the node's host runs it in.
  pub shell: &'a str,
  /// Variables laid over the agent's environment.
  pub env: BTreeMap<&'a str, String>,
}

/// A run's end, as the node service answers it.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
  /// The command's exit code; none where the hub reports none.
  pub exit_code: Option<i64>,
  pub stdout: String,
  pub stderr: String,
  /// Whether more came on standard output than its last 64 KiB answered.
  #[serde(default)]
  pub stdout_cut: bool,
}

/// The node service's runs route, reached with the caller's own token.
pub struct NodeService(Reach);

impl NodeService {
  pub fn new(upstream: &Upstream, http: reqwest::Client) -> Result<Self, Failure> {
    Reach::of(NAME, upstream, http).map(Self)
  }

  /// Runs `ask` on `node` and answers its end.
  pub async fn run(&self, node: &str, ask: &RunAsk<'_>) -> Result<Run, Failure> {
    self.0.post(&format!("/nodes/{}/runs", segment(node)), ask).await
  }
}
