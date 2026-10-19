//! Labels (`docs/api.md`, 「Labels」).

use axum::http::Method;
use axum::response::Response;
use serde_json::{Value, json};

use crate::http::{Call, Refused, Route};
use crate::labels::{create_label, delete_label, update_label};
use crate::routes::requests::{color_of, configured, with_mail_account};

/// A field read as text the way the screen's old server read it: a string as it is, nothing as empty, and
/// any other value as its own text.
fn text_of(value: Option<&Value>) -> String {
  match value {
    None | Some(Value::Null) => String::new(),
    Some(Value::String(text)) => text.clone(),
    Some(other) => other.to_string(),
  }
}

/// `POST /api/mail/{account}/labels`: a new label, placed after the others.
pub struct CreateLabel;

impl Route for CreateLabel {
  const METHOD: Method = Method::POST;
  const PATH: &'static str = "/api/mail/{account}/labels";

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let key = call.path_param("account").await?;
    let body = call.body().await;
    let standing = call.standing().clone();

    with_mail_account(&standing, key.as_deref(), |mail| async move {
      let color = color_of(body.get("color"))?;
      let label =
        create_label(mail.standing.store.pool(), &mail.account.address, &text_of(body.get("name")), color)
          .await?;

      Ok(configured(json!({ "label": label })))
    })
    .await
  }
}

crate::route!(CreateLabel);

/// `PATCH /api/mail/{account}/labels/{id}`: a label renamed or given another colour.
pub struct ChangeLabel;

impl Route for ChangeLabel {
  const METHOD: Method = Method::PATCH;
  const PATH: &'static str = "/api/mail/{account}/labels/{id}";

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let mut params = call.path_params().await?;
    let key = params.remove("account");
    let id = params.remove("id").unwrap_or_default();
    let body = call.body().await;
    let standing = call.standing().clone();

    with_mail_account(&standing, key.as_deref(), |mail| async move {
      let name = body.get("name").and_then(Value::as_str);
      let color = match body.get("color") {
        None => None,
        Some(color) => Some(color_of(Some(color))?),
      };

      update_label(mail.standing.store.pool(), &mail.account.address, &id, name, color).await?;

      Ok(configured(json!({})))
    })
    .await
  }
}

crate::route!(ChangeLabel);

/// `DELETE /api/mail/{account}/labels/{id}`: a label deleted, its keyword taken off every message that
/// carried it.
pub struct DeleteLabel;

impl Route for DeleteLabel {
  const METHOD: Method = Method::DELETE;
  const PATH: &'static str = "/api/mail/{account}/labels/{id}";

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let mut params = call.path_params().await?;
    let key = params.remove("account");
    let id = params.remove("id").unwrap_or_default();
    let standing = call.standing().clone();

    with_mail_account(&standing, key.as_deref(), |mail| async move {
      delete_label(mail.standing.store.pool(), &mail.session, &mail.account.address, &id).await?;

      Ok(configured(json!({})))
    })
    .await
  }
}

crate::route!(DeleteLabel);
