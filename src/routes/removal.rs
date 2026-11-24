//! The platform administrator's removal of an owner or an organisation unit for good (`docs/api.md`, 「Removing an
//! owner」): what the service keeps of it, counted and forgotten.

use axum::http::{Method, StatusCode};
use axum::response::Response;
use serde_json::json;

use crate::announcements::ACCOUNT_DELETED;
use crate::http::{Answer, Call, Guard, Operation, Refused, Route, Standing, answer, inline};
use crate::servers::own_holder;
use crate::wire::{OwnerHeld, RemovalCounts, Removed, UnitHeld, UnitRemovalQuery};

const OWNER: (&str, &str) =
  ("owner", "The owner's key as the account service names it: positive decimal digits.");
const UNIT: (&str, &str) = ("unit", "The organisation unit's id as the account service names it.");

/// A key the path names, refused unless it is positive decimal digits.
async fn key(call: &mut Call, name: &str) -> Result<String, Refused> {
  let given = call.key(name).await?;
  let digits = !given.is_empty() && given.len() <= 20 && given.bytes().all(|byte| byte.is_ascii_digit());

  if digits && given.bytes().any(|byte| byte != b'0') {
    Ok(given)
  } else {
    Err(Refused::bad_request(format!("{given:?} is no owner's or unit's key")))
  }
}

/// The link a unit's servers stand under.
fn unit_link(unit: &str) -> String {
  format!("unit:{unit}")
}

async fn owner_held(standing: &Standing, owner: &str) -> Result<OwnerHeld, Refused> {
  let accounts = standing.accounts.held_by_owner(owner).await?;
  let servers = standing.servers.held_with_use(&own_holder(owner)).await?;

  Ok(OwnerHeld {
    accounts: i64::try_from(accounts.len()).unwrap_or(i64::MAX),
    servers: i64::try_from(servers.len()).unwrap_or(i64::MAX),
  })
}

async fn unit_held(standing: &Standing, unit: &str) -> Result<UnitHeld, Refused> {
  let servers = standing.servers.held_with_use(&unit_link(unit)).await?;
  let in_use = servers.iter().filter(|(_, uses)| *uses > 0).count();

  Ok(UnitHeld {
    servers: i64::try_from(servers.len() - in_use).unwrap_or(i64::MAX),
    servers_in_use: i64::try_from(in_use).unwrap_or(i64::MAX),
  })
}

/// `GET /owners/{owner}/removal`: what an owner's removal would take now.
pub struct CountOwnerRemoval;

impl Route for CountOwnerRemoval {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/owners/{owner}/removal";
  const OPERATION: Operation = Operation {
    id: "getOwnersByOwnerRemoval",
    summary: "What removing the owner for good would take now; nothing changes.",
    tag: "removal",
    keys: &[OWNER],
    query: None,
    body: None,
    status: 200,
    answer: Answer::Json(inline::<RemovalCounts<OwnerHeld>>),
    codes: &[],
    guard: Guard::Administrator,
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let owner = key(&mut call, "owner").await?;
    let counts = owner_held(&call.standing().clone(), &owner).await?;

    Ok(answer(StatusCode::OK, &RemovalCounts { counts }))
  }
}

crate::route!(CountOwnerRemoval);

/// `DELETE /owners/{owner}`: every account and own server of the owner forgotten.
pub struct RemoveOwner;

impl Route for RemoveOwner {
  const METHOD: Method = Method::DELETE;
  const PATH: &'static str = "/owners/{owner}";
  const OPERATION: Operation = Operation {
    id: "deleteOwnersByOwner",
    summary: "Remove the owner for good: every account and own server the owner key holds.",
    tag: "removal",
    keys: &[OWNER],
    query: None,
    body: None,
    status: 200,
    answer: Answer::Json(inline::<Removed<OwnerHeld>>),
    codes: &[],
    guard: Guard::Administrator,
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let owner = key(&mut call, "owner").await?;
    let standing = call.standing().clone();
    let mut removed = OwnerHeld::default();

    for (id, name) in standing.accounts.held_by_owner(&owner).await? {
      standing.accounts.forget_id(id).await?;
      standing.sends.cancel_account(id);
      standing.mailboxes.forget(id).await;
      standing
        .events
        .publish(ACCOUNT_DELETED, &owner, Some(&name), &json!({ "name": name }))
        .await;
      removed.accounts += 1;
    }

    removed.servers = standing.servers.forget_unused(&own_holder(&owner)).await?;
    standing
      .events
      .forget_owner(&owner)
      .await
      .map_err(|why| Refused::failed(format!("the event window could not be cleared: {why}")))?;

    Ok(answer(StatusCode::OK, &Removed { removed }))
  }
}

crate::route!(RemoveOwner);

/// `GET /org-units/{unit}/removal`: what a unit's removal would take now.
pub struct CountUnitRemoval;

impl Route for CountUnitRemoval {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/org-units/{unit}/removal";
  const OPERATION: Operation = Operation {
    id: "getOrgUnitsByUnitRemoval",
    summary: "What removing the unit for good would take now; nothing changes.",
    tag: "removal",
    keys: &[UNIT],
    query: Some(inline::<UnitRemovalQuery>),
    body: None,
    status: 200,
    answer: Answer::Json(inline::<RemovalCounts<UnitHeld>>),
    codes: &[],
    guard: Guard::Administrator,
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    call.query::<UnitRemovalQuery>()?;
    let unit = key(&mut call, "unit").await?;
    let counts = unit_held(&call.standing().clone(), &unit).await?;

    Ok(answer(StatusCode::OK, &RemovalCounts { counts }))
  }
}

crate::route!(CountUnitRemoval);

/// `DELETE /org-units/{unit}`: the unit's servers no account stands on forgotten.
pub struct RemoveUnit;

impl Route for RemoveUnit {
  const METHOD: Method = Method::DELETE;
  const PATH: &'static str = "/org-units/{unit}";
  const OPERATION: Operation = Operation {
    id: "deleteOrgUnitsByUnit",
    summary: "Remove what the unit's link holds for good: its servers no account stands on.",
    tag: "removal",
    keys: &[UNIT],
    query: Some(inline::<UnitRemovalQuery>),
    body: None,
    status: 200,
    answer: Answer::Json(inline::<Removed<UnitHeld>>),
    codes: &[],
    guard: Guard::Administrator,
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    call.query::<UnitRemovalQuery>()?;
    let unit = key(&mut call, "unit").await?;
    let before = unit_held(&call.standing().clone(), &unit).await?;
    let standing = call.standing().clone();
    let servers = standing.servers.forget_unused(&unit_link(&unit)).await?;
    let removed = UnitHeld {
      servers,
      servers_in_use: before.servers_in_use,
    };

    Ok(answer(StatusCode::OK, &Removed { removed }))
  }
}

crate::route!(RemoveUnit);
