//! The accounts and the summary of their inboxes (`docs/api.md`, 「Accounts」): the kinds the adapters
//! declare, and each owner's own accounts, listed, added once they sign in, changed, removed, and begun
//! through a provider's page. A route acts for the owner key its request carries, and reaches that owner's
//! accounts alone.

use std::sync::Arc;

use axum::http::{Method, StatusCode};
use axum::response::Response;
use futures::future::join_all;
use serde_json::json;

use crate::accounts::authorizations::{AUTHORIZATION_SPAN, Authorizations, Waiting};
use crate::accounts::{Account, Keeping, account_name};
use crate::adapters::{adapters, by_password, checked_fields, declared};
use crate::announcements::{ACCOUNT_CHANGED, ACCOUNT_CREATED, ACCOUNT_DELETED};
use crate::http::{Answer, Call, Operation, Refused, Route, Standing, answer, inline, nothing, schema};
use crate::routes::requests::{Asked, MailFault, address_shaped, owned_account};
use crate::wire::{
  AccountKind, AccountPatch, Authorization, Fields, ListQuery, NewAccount, NewAuthorization, Page, Summary,
};

const ACCOUNT: (&str, &str) = ("accountName", "The account's name, unique among its owner's accounts.");

/// An account as a member, with its inbox's unread count; an account whose server did not answer is
/// unreachable rather than empty.
async fn member(standing: &Standing, account: &Account) -> crate::wire::Account {
  let unread = match standing.mailboxes.of(account, &standing.context()) {
    Ok(mailbox) => mailbox.inbox_unread().await.ok(),
    Err(_) => None,
  };

  crate::wire::Account {
    name: account.name.clone(),
    kind: account.kind.clone(),
    address: account.address.clone(),
    position: account.position,
    settings: account.settings.clone(),
    unread_count: unread.unwrap_or(0),
    reachable: unread.is_some(),
  }
}

/// Every account of the owner as a member, the accounts asked side by side.
async fn members(standing: &Standing, owner: &str) -> Result<Vec<crate::wire::Account>, Refused> {
  let accounts = standing.accounts.list(owner).await?;

  Ok(join_all(accounts.iter().map(|account| member(standing, account))).await)
}

/// An address as an account keeps it, or a refusal where it is not one.
fn address_of(address: &str) -> Result<String, Asked> {
  let address = address.trim().to_lowercase();

  if address_shaped(&address) {
    Ok(address)
  } else {
    Err(Asked::bad_request("the address is not an address"))
  }
}

/// The name an account is kept under: the one given, else its address's local part.
fn name_of(given: Option<&str>, address: &str) -> Result<String, Asked> {
  account_name(
    given
      .filter(|name| !name.trim().is_empty())
      .unwrap_or_else(|| address.split_once('@').map_or(address, |(local, _)| local)),
  )
}

/// `GET /account-kinds`.
pub struct AccountKinds;

impl Route for AccountKinds {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/account-kinds";
  const OPERATION: Operation = Operation {
    id: "listAccountKinds",
    summary: "The kinds of account the service declares, one for each adapter built into it.",
    tag: "accounts",
    keys: &[],
    query: None,
    body: None,
    status: 200,
    answer: Answer::Json(schema::<Page<AccountKind>>),
    codes: &[],
    owned: false,
  };

  async fn answer(&self, _call: Call) -> Result<Response, Refused> {
    let items: Vec<AccountKind> = adapters().iter().map(|adapter| adapter.kind()).collect();

    Ok(answer(StatusCode::OK, &Page { items, more: false }))
  }
}

crate::route!(AccountKinds);

/// `GET /accounts`.
pub struct Accounts;

impl Route for Accounts {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/accounts";
  const OPERATION: Operation = Operation {
    id: "listAccounts",
    summary: "The owner's mail accounts, in their order.",
    tag: "accounts",
    keys: &[],
    query: Some(inline::<ListQuery>),
    body: None,
    status: 200,
    answer: Answer::Json(schema::<Page<crate::wire::Account>>),
    codes: &[],
    owned: true,
  };

  async fn answer(&self, call: Call) -> Result<Response, Refused> {
    let owner = call.owner()?;
    let query: ListQuery = call.query()?;
    let accounts = members(call.standing(), &owner).await?;

    Ok(answer(StatusCode::OK, &Page::by_position(accounts, |account| account.position, &query)))
  }
}

crate::route!(Accounts);

/// `POST /accounts`: an account added once it signed in, by password from its fields or by the answer of
/// its provider's page.
pub struct CreateAccount;

impl Route for CreateAccount {
  const METHOD: Method = Method::POST;
  const PATH: &'static str = "/accounts";
  const OPERATION: Operation = Operation {
    id: "createAccounts",
    summary: "Add an account of the owner, once it signed in.",
    tag: "accounts",
    keys: &[],
    query: None,
    body: Some(schema::<NewAccount>),
    status: 201,
    answer: Answer::Json(schema::<crate::wire::Account>),
    codes: &["conflict", "unknown-kind", "sign-in-failed", "unknown-authorization", "not-ready"],
    owned: true,
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let owner = call.owner()?;
    let new: NewAccount = call.body().await?;
    let standing = Arc::clone(call.standing());
    let added = async {
      let adapter = declared(&new.kind)?;
      let kind = adapter.kind();

      standing.accounts.secrets()?;

      let keeping = if by_password(&kind) {
        if new.authorization.is_some() {
          return Err(Asked::bad_request("a kind that signs in by password takes no authorization").into());
        }

        let address = address_of(new.address.as_deref().unwrap_or_default())?;
        let (settings, secrets) = checked_fields(
          &kind,
          &new.settings.clone().unwrap_or_default(),
          &new.secrets.clone().unwrap_or_default(),
        )?;
        let name = name_of(new.name.as_deref(), &address)?;

        if standing.accounts.taken(&owner, &name, None).await? {
          return Err(Asked::conflict("the owner holds an account of that name").into());
        }

        adapter.sign_in(&address, &settings, &secrets).await?;

        Keeping {
          name,
          kind: kind.name,
          address,
          settings,
          secrets,
        }
      } else {
        let Some(given) = &new.authorization else {
          return Err(
            Asked::bad_request("a kind that signs in by authorization needs its authorization").into(),
          );
        };

        if new.address.is_some() || new.settings.is_some() || new.secrets.is_some() {
          return Err(Asked::bad_request("a kind that signs in by authorization takes no fields").into());
        }

        let unknown =
          || Asked::rule("unknown-authorization", "no authorization of the owner runs under that id");
        let waiting = standing.authorizations.take(&owner, &kind.name, &given.id).ok_or_else(unknown)?;
        let signed = adapter.finish(&waiting.kept, &given.answer).await?;
        let address = address_of(&signed.address)?;
        let name = name_of(new.name.as_deref(), &address)?;

        Keeping {
          name,
          kind: kind.name,
          address,
          settings: signed.settings,
          secrets: signed.secrets,
        }
      };

      standing.accounts.add(&owner, &keeping).await
    }
    .await?;
    let made = member(&standing, &added).await;

    standing.events.publish(ACCOUNT_CREATED, &owner, Some(&added.name), &made).await;

    Ok(answer(StatusCode::CREATED, &made))
  }
}

crate::route!(CreateAccount);

/// `GET /accounts/{accountName}`.
pub struct ReadAccount;

impl Route for ReadAccount {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/accounts/{accountName}";
  const OPERATION: Operation = Operation {
    id: "readAccountsByAccountName",
    summary: "One account of the owner.",
    tag: "accounts",
    keys: &[ACCOUNT],
    query: None,
    body: None,
    status: 200,
    answer: Answer::Json(schema::<crate::wire::Account>),
    codes: &["not-found"],
    owned: true,
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let account = owned_account(&mut call).await?;

    Ok(answer(StatusCode::OK, &member(call.standing(), &account).await))
  }
}

crate::route!(ReadAccount);

/// `PATCH /accounts/{accountName}`: renamed, moved, or its address, settings or secrets changed, signing in
/// again before a change of those is kept.
pub struct ChangeAccount;

impl Route for ChangeAccount {
  const METHOD: Method = Method::PATCH;
  const PATH: &'static str = "/accounts/{accountName}";
  const OPERATION: Operation = Operation {
    id: "changeAccountsByAccountName",
    summary: "Rename or move an account, or change its address, settings or secrets.",
    tag: "accounts",
    keys: &[ACCOUNT],
    query: None,
    body: Some(schema::<AccountPatch>),
    status: 200,
    answer: Answer::Json(schema::<crate::wire::Account>),
    codes: &["not-found", "conflict", "sign-in-failed", "not-ready"],
    owned: true,
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let patch: AccountPatch = call.body().await?;
    let account = owned_account(&mut call).await?;
    let standing = Arc::clone(call.standing());
    let changed = async {
      let adapter =
        declared(&account.kind).map_err(|_| MailFault::server("the account's kind has no adapter"))?;
      let kind = adapter.kind();
      let mut changed = account.clone();

      if let Some(name) = &patch.name {
        changed.name = account_name(name)?;
      }

      if standing.accounts.taken(&account.owner, &changed.name, Some(account.id)).await? {
        return Err(Asked::conflict("the owner holds an account of that name").into());
      }

      if patch.address.is_some() || patch.settings.is_some() || patch.secrets.is_some() {
        if !by_password(&kind) {
          return Err(
            Asked::bad_request(
              "an account that signs in by authorization changes its name and place alone",
            )
            .into(),
          );
        }

        if let Some(address) = &patch.address {
          changed.address = address_of(address)?;
        }

        let settings = patch.settings.clone().unwrap_or_else(|| account.settings.clone());
        let mut secrets: Fields = account.secrets.clone();

        for (name, value) in patch.secrets.iter().flatten().filter(|(_, value)| !value.trim().is_empty()) {
          secrets.insert(name.clone(), value.clone());
        }

        let (settings, secrets) = checked_fields(&kind, &settings, &secrets)?;

        changed.settings = settings;
        changed.secrets = secrets;
        adapter.sign_in(&changed.address, &changed.settings, &changed.secrets).await?;
      }

      standing.accounts.change(&changed, patch.position).await
    }
    .await?;
    let made = member(&standing, &changed).await;

    standing
      .events
      .publish(ACCOUNT_CHANGED, &changed.owner, Some(&changed.name), &made)
      .await;

    Ok(answer(StatusCode::OK, &made))
  }
}

crate::route!(ChangeAccount);

/// `DELETE /accounts/{accountName}`: the account forgotten with everything the store holds for it.
pub struct DeleteAccount;

impl Route for DeleteAccount {
  const METHOD: Method = Method::DELETE;
  const PATH: &'static str = "/accounts/{accountName}";
  const OPERATION: Operation = Operation {
    id: "deleteAccountsByAccountName",
    summary: "Remove an account of the owner; its mailbox on the server is untouched.",
    tag: "accounts",
    keys: &[ACCOUNT],
    query: None,
    body: None,
    status: 204,
    answer: Answer::Nothing,
    codes: &["not-found"],
    owned: true,
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let account = owned_account(&mut call).await?;
    let standing = call.standing();

    standing.accounts.remove(&account).await?;
    standing.sends.cancel_account(account.id);
    standing.mailboxes.forget(account.id).await;
    standing
      .events
      .publish(ACCOUNT_DELETED, &account.owner, Some(&account.name), &json!({ "name": account.name }))
      .await;

    Ok(nothing())
  }
}

crate::route!(DeleteAccount);

/// `POST /authorizations`: an account of a kind that signs in through its provider's page, begun.
pub struct CreateAuthorization;

impl Route for CreateAuthorization {
  const METHOD: Method = Method::POST;
  const PATH: &'static str = "/authorizations";
  const OPERATION: Operation = Operation {
    id: "createAuthorizations",
    summary: "Begin adding an account through its provider's page.",
    tag: "accounts",
    keys: &[],
    query: None,
    body: Some(schema::<NewAuthorization>),
    status: 201,
    answer: Answer::Json(schema::<Authorization>),
    codes: &["unknown-kind", "no-authorization"],
    owned: true,
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let owner = call.owner()?;
    let new: NewAuthorization = call.body().await?;
    let standing = call.standing();

    if new.return_to.trim().is_empty() {
      return Err(Refused::bad_request("an authorization needs the address to come back to"));
    }

    let adapter = declared(&new.kind).map_err(MailFault::from)?;
    let id = Authorizations::new_id();
    let Some(begun) = adapter.begin(&id, new.return_to.trim()) else {
      return Err(MailFault::from(Asked::rule("no-authorization", "the kind signs in by password")).into());
    };
    let begun = begun?;
    let expires_at = chrono::Utc::now().timestamp_millis() + AUTHORIZATION_SPAN.as_millis() as i64;

    standing.authorizations.hold(
      id.clone(),
      Waiting {
        owner,
        kind: new.kind.clone(),
        kept: begun.kept,
        expires_at,
      },
    );

    Ok(answer(
      StatusCode::CREATED,
      &Authorization {
        id,
        kind: new.kind,
        url: begun.url,
        expires_at,
      },
    ))
  }
}

crate::route!(CreateAuthorization);

/// `GET /summary`.
pub struct SummaryRoute;

impl Route for SummaryRoute {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/summary";
  const OPERATION: Operation = Operation {
    id: "listSummary",
    summary: "Unread counts across the owner's accounts.",
    tag: "summary",
    keys: &[],
    query: None,
    body: None,
    status: 200,
    answer: Answer::Json(schema::<Summary>),
    codes: &[],
    owned: true,
  };

  async fn answer(&self, call: Call) -> Result<Response, Refused> {
    let owner = call.owner()?;
    let accounts = members(call.standing(), &owner).await?;
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
