//! An account's labels (`docs/api.md`, 「Labels」).

use axum::http::{Method, StatusCode};
use axum::response::Response;

use crate::http::{Answer, Call, Operation, Refused, Route, answer, inline, nothing, schema};
use crate::labels::{create_label, forget_label, list_labels, update_label};
use crate::routes::requests::{Asked, colour_of, with_account};
use crate::wire::{Label, LabelPatch, ListQuery, NewLabel, Page};

const ACCOUNT: (&str, &str) = ("accountName", "The account's name, unique among its owner's accounts.");
const LABEL: (&str, &str) = ("labelId", "The label's id.");

/// `GET /accounts/{accountName}/labels`.
pub struct Labels;

impl Route for Labels {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/accounts/{accountName}/labels";
  const OPERATION: Operation = Operation {
    id: "listAccountsLabelsByAccountName",
    summary: "An account's labels in their order.",
    tag: "accounts",
    keys: &[ACCOUNT],
    query: Some(inline::<ListQuery>),
    body: None,
    status: 200,
    answer: Answer::Json(schema::<Page<Label>>),
    codes: &["not-found"],
    owned: true,
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let query: ListQuery = call.query()?;

    with_account(&mut call, |mail| async move {
      let labels = list_labels(mail.standing.store.pool(), &mail.key()).await?;

      Ok(answer(StatusCode::OK, &Page::by_position(labels, |label| label.position, &query)))
    })
    .await
  }
}

crate::route!(Labels);

/// `POST /accounts/{accountName}/labels`.
pub struct CreateLabel;

impl Route for CreateLabel {
  const METHOD: Method = Method::POST;
  const PATH: &'static str = "/accounts/{accountName}/labels";
  const OPERATION: Operation = Operation {
    id: "createAccountsLabelsByAccountName",
    summary: "Make a label, placed after the others.",
    tag: "accounts",
    keys: &[ACCOUNT],
    query: None,
    body: Some(schema::<NewLabel>),
    status: 201,
    answer: Answer::Json(schema::<Label>),
    codes: &["not-found", "conflict", "too-many-labels"],
    owned: true,
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let new: NewLabel = call.body().await?;

    with_account(&mut call, |mail| async move {
      let colour = colour_of(&new.colour)?;
      let label = create_label(mail.standing.store.pool(), &mail.key(), &new.name, colour).await?;

      Ok(answer(StatusCode::CREATED, &label))
    })
    .await
  }
}

crate::route!(CreateLabel);

/// `PATCH /accounts/{accountName}/labels/{labelId}`.
pub struct ChangeLabel;

impl Route for ChangeLabel {
  const METHOD: Method = Method::PATCH;
  const PATH: &'static str = "/accounts/{accountName}/labels/{labelId}";
  const OPERATION: Operation = Operation {
    id: "changeAccountsLabelsByAccountNameLabelId",
    summary: "Rename, recolour or move a label.",
    tag: "accounts",
    keys: &[ACCOUNT, LABEL],
    query: None,
    body: Some(schema::<LabelPatch>),
    status: 200,
    answer: Answer::Json(schema::<Label>),
    codes: &["not-found", "conflict"],
    owned: true,
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let id = call.key("labelId").await?;
    let patch: LabelPatch = call.body().await?;

    with_account(&mut call, |mail| async move {
      let colour = patch.colour.as_deref().map(colour_of).transpose()?;
      let pool = mail.standing.store.pool();
      let label =
        update_label(pool, &mail.key(), &id, patch.name.as_deref(), colour, patch.position).await?;

      Ok(answer(StatusCode::OK, &label))
    })
    .await
  }
}

crate::route!(ChangeLabel);

/// `DELETE /accounts/{accountName}/labels/{labelId}`.
pub struct DeleteLabel;

impl Route for DeleteLabel {
  const METHOD: Method = Method::DELETE;
  const PATH: &'static str = "/accounts/{accountName}/labels/{labelId}";
  const OPERATION: Operation = Operation {
    id: "deleteAccountsLabelsByAccountNameLabelId",
    summary: "Remove a label, its keyword taken off every message that carried it.",
    tag: "accounts",
    keys: &[ACCOUNT, LABEL],
    query: None,
    body: None,
    status: 204,
    answer: Answer::Nothing,
    codes: &["not-found"],
    owned: true,
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let id = call.key("labelId").await?;

    with_account(&mut call, |mail| async move {
      let pool = mail.standing.store.pool();

      if !list_labels(pool, &mail.key()).await?.iter().any(|label| label.id == id) {
        return Err(Asked::not_found("no label of the account has that id").into());
      }

      mail.mailbox.drop_label(&id).await?;
      forget_label(pool, &mail.key(), &id).await?;

      Ok(nothing())
    })
    .await
  }
}

crate::route!(DeleteLabel);
