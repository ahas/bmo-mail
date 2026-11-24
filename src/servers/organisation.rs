//! Who offers which server (`docs/mail.md`, 「Mail servers」): the account service's delegation chain, asked for the
//! kind `mail.servers` this service declares. A link's value is the list of the servers it holds, written here
//! alone and in the name of the person changing them, so the chain judges who may change a link and whether a lock
//! above withholds it. Nothing it answers is kept: every list and every write asks again.

use std::collections::HashMap;

use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::clients::Failure;
use crate::clients::accounts::{Accounts, Kind};
use crate::http::{FORBIDDEN, NOT_FOUND, UPSTREAM_FAILED};
use crate::routes::requests::{Asked, MailFault};
use crate::servers::PLATFORM;
use crate::settings::Settings;

/// The chain's kind.
pub const KIND: &str = "mail.servers";
/// The code the chain refuses a write under a lock above with.
pub const LOCKED: &str = "locked";

/// The kind as this service declares it.
fn kind() -> Kind {
  Kind {
    key: KIND,
    declared_by: "mail",
    description: "The mail servers a link offers the people under it",
    shape: "json",
    choices: Vec::new(),
    mode: "layered",
    rules: false,
    levels: vec!["platform", "unit"],
    unit_permission: "manage",
    person_writer: "heads",
  }
}

/// A link of a person's chain that may offer servers, with the name of its company or unit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offering {
  pub link: String,
  /// None for the platform's.
  pub name: Option<String>,
}

/// The account service as the servers ask it, where the settings name one.
pub struct Organisation {
  accounts: Option<Accounts>,
}

impl std::fmt::Debug for Organisation {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter.debug_struct("Organisation").field("named", &self.accounts.is_some()).finish()
  }
}

/// A refusal of the account service, as a route answers it.
fn refused(failure: Failure) -> MailFault {
  let asked = |status: StatusCode, code: &'static str, error: String| {
    MailFault::Asked(Asked {
      status,
      code,
      error,
    })
  };

  match failure {
    Failure::Refused {
      status: 403,
      answer,
      ..
    } => asked(
      StatusCode::FORBIDDEN,
      FORBIDDEN,
      sentence(answer, "the account service refused the change"),
    ),
    Failure::Refused {
      status: 409,
      answer: Some(answer),
      ..
    } if answer.code == LOCKED => asked(StatusCode::CONFLICT, LOCKED, answer.error),
    Failure::Refused {
      status: 404,
      answer,
      ..
    } => asked(StatusCode::NOT_FOUND, NOT_FOUND, sentence(answer, "the link does not stand")),
    other => {
      eprintln!("mail: the account service did not answer: {other}");

      asked(
        StatusCode::BAD_GATEWAY,
        UPSTREAM_FAILED,
        "the account service did not answer".to_owned(),
      )
    }
  }
}

fn sentence(answer: Option<crate::wire::WireError>, otherwise: &str) -> String {
  answer.map_or_else(|| otherwise.to_owned(), |answer| answer.error)
}

/// Whether a failure is the chain not knowing the kind yet.
fn undeclared(failure: &Failure) -> bool {
  matches!(failure, Failure::Refused { status: 404, answer: Some(answer), .. } if answer.code == "no-such-kind")
}

impl Organisation {
  /// The account service the settings name, or none.
  pub fn new(settings: &Settings) -> Self {
    Self {
      accounts: Accounts::new(&settings.account_service, reqwest::Client::new()).ok(),
    }
  }

  /// Whether the settings name an account service.
  pub fn named(&self) -> bool {
    self.accounts.is_some()
  }

  fn accounts(&self) -> Result<&Accounts, MailFault> {
    self.accounts.as_ref().ok_or_else(|| {
      Asked::not_ready("the settings name no account service, so no server of a link stands").into()
    })
  }

  /// Declares the kind, as the service stands; a failure is said in the log and the next read declares again.
  pub async fn declare(&self) {
    if let Some(accounts) = &self.accounts
      && let Err(why) = accounts.declare(&kind()).await
    {
      eprintln!("mail: the account service did not take the kind {KIND}: {why}");
    }
  }

  /// Declares the kind where a call found the chain not knowing it, and says whether the call is to be asked
  /// once more.
  async fn redeclared<T>(accounts: &Accounts, done: &Result<T, Failure>) -> Result<bool, MailFault> {
    match done {
      Err(failure) if undeclared(failure) => {
        accounts.declare(&kind()).await.map_err(refused)?;

        Ok(true)
      }
      _ => Ok(false),
    }
  }

  /// The links that offer an owner servers, their organisation's first — the company's and then each unit's
  /// down to their own, cut at the link whose lock cuts the chain — then the platform's. A person the account
  /// service does not know reaches the platform's alone, and a service that names no account service none.
  pub async fn offering(&self, owner: &str) -> Result<Vec<Offering>, MailFault> {
    let Some(accounts) = &self.accounts else {
      return Ok(Vec::new());
    };
    let mut resolved = accounts.resolve(KIND, owner).await;

    if Self::redeclared(accounts, &resolved).await? {
      resolved = accounts.resolve(KIND, owner).await;
    }

    let resolved = resolved.map_err(refused)?;
    let Some(resolved) = resolved else {
      return Ok(vec![Offering {
        link: PLATFORM.to_owned(),
        name: None,
      }]);
    };
    let names: HashMap<String, String> = accounts
      .unit_of(owner)
      .await
      .map_err(refused)?
      .map(|unit| {
        unit
          .path
          .into_iter()
          .filter_map(|one| Some((format!("unit:{}", crate::clients::account_id(&one.id)?), one.name)))
          .collect()
      })
      .unwrap_or_default();
    let cut = resolved
      .locked_by
      .as_ref()
      .and_then(|lock| resolved.path.iter().position(|link| link == lock))
      .map_or(resolved.path.len(), |at| at + 1);
    let read = &resolved.path[..cut];
    let units = read.iter().filter(|link| link.starts_with("unit:")).map(|link| Offering {
      link: link.clone(),
      name: names.get(link).cloned(),
    });
    let platform = read.iter().filter(|link| *link == PLATFORM).map(|link| Offering {
      link: link.clone(),
      name: None,
    });

    Ok(units.chain(platform).collect())
  }

  /// Whether the owner may change a link's servers now; a refusal where they may not read its entry.
  pub async fn writable(&self, link: &str, owner: &str) -> Result<bool, MailFault> {
    let accounts = self.accounts()?;
    let mut entry = accounts.entry(KIND, link, owner).await;

    if Self::redeclared(accounts, &entry).await? {
      entry = accounts.entry(KIND, link, owner).await;
    }

    entry.map(|entry| entry.writable).map_err(refused)
  }

  /// Writes the list of a link's servers in the owner's name.
  pub async fn write(&self, link: &str, owner: &str, ids: &[i64]) -> Result<(), MailFault> {
    let value: Value = json!(ids);

    let accounts = self.accounts()?;
    let mut written = accounts.write(KIND, link, owner, &value).await;

    if Self::redeclared(accounts, &written).await? {
      written = accounts.write(KIND, link, owner, &value).await;
    }

    written.map_err(refused)
  }
}
