//! Filters, spam senders and settings (`docs/api.md`, 「Filters, spam senders and settings」).

use axum::http::Method;
use axum::response::Response;
use serde_json::{Value, json};

use crate::account_settings::{read_settings, write_settings};
use crate::http::{Call, Refused, Route};
use crate::routes::requests::{Asked, MailFault, configured, strings_of, with_mail_account};
use crate::rules::{
  add_spam_senders, create_rule, delete_rule, list_rules, list_spam_senders, order_rules,
  remove_spam_sender, run_rules_over_folder, spam_header_rules, update_rule,
};
use crate::wire::MailSettings;

/// How long a signature may be, in the characters a browser counts. It is markup — a few lines under a
/// message with a picture carried in them — so the room it needs is the markup's, not the words'.
const SIGNATURE_LIMIT: usize = 200_000;

/// `GET /api/mail/{account}/rules`: the account's filter rules in their order, and the ready-made
/// spam-header rules the settings offer.
pub struct Rules;

impl Route for Rules {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/api/mail/{account}/rules";

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let key = call.path_param("account").await?;
    let standing = call.standing().clone();

    with_mail_account(&standing, key.as_deref(), |mail| async move {
      let rules = list_rules(mail.standing.store.pool(), &mail.account.address).await?;

      Ok(configured(json!({ "rules": rules, "presets": spam_header_rules() })))
    })
    .await
  }
}

crate::route!(Rules);

/// `POST /api/mail/{account}/rules`: a new rule, placed after the others.
pub struct CreateRule;

impl Route for CreateRule {
  const METHOD: Method = Method::POST;
  const PATH: &'static str = "/api/mail/{account}/rules";

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let key = call.path_param("account").await?;
    let body = call.body().await;
    let standing = call.standing().clone();

    with_mail_account(&standing, key.as_deref(), |mail| async move {
      let rule = create_rule(mail.standing.store.pool(), &mail.account.address, &body).await?;

      Ok(configured(json!({ "rule": rule })))
    })
    .await
  }
}

crate::route!(CreateRule);

/// A rule's id from the path: a positive whole number.
fn rule_id(value: Option<&str>) -> Option<i64> {
  value.and_then(|id| id.trim().parse::<i64>().ok()).filter(|id| *id > 0)
}

/// `PUT /api/mail/{account}/rules/{id}`: a rule written again: its conditions, its actions, on or off.
pub struct ChangeRule;

impl Route for ChangeRule {
  const METHOD: Method = Method::PUT;
  const PATH: &'static str = "/api/mail/{account}/rules/{id}";

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let mut params = call.path_params().await?;
    let key = params.remove("account");
    let id = rule_id(params.get("id").map(String::as_str));
    let body = call.body().await;
    let standing = call.standing().clone();

    with_mail_account(&standing, key.as_deref(), |mail| async move {
      let Some(id) = id else {
        return Err(MailFault::asked("rule_not_found", "no rule of the account has that id"));
      };

      update_rule(mail.standing.store.pool(), &mail.account.address, id, &body).await?;

      Ok(configured(json!({})))
    })
    .await
  }
}

crate::route!(ChangeRule);

/// `DELETE /api/mail/{account}/rules/{id}`: a rule deleted; an id that names none deletes nothing.
pub struct DeleteRule;

impl Route for DeleteRule {
  const METHOD: Method = Method::DELETE;
  const PATH: &'static str = "/api/mail/{account}/rules/{id}";

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let mut params = call.path_params().await?;
    let key = params.remove("account");
    let id = rule_id(params.get("id").map(String::as_str));
    let standing = call.standing().clone();

    with_mail_account(&standing, key.as_deref(), |mail| async move {
      if let Some(id) = id {
        delete_rule(mail.standing.store.pool(), &mail.account.address, id).await?;
      }

      Ok(configured(json!({})))
    })
    .await
  }
}

crate::route!(DeleteRule);

/// `POST /api/mail/{account}/rules/order`: the rules put in the order given.
pub struct OrderRules;

impl Route for OrderRules {
  const METHOD: Method = Method::POST;
  const PATH: &'static str = "/api/mail/{account}/rules/order";

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let key = call.path_param("account").await?;
    let body = call.body().await;
    let standing = call.standing().clone();

    with_mail_account(&standing, key.as_deref(), |mail| async move {
      let ids: Vec<i64> = body
        .get("ids")
        .and_then(Value::as_array)
        .map_or_else(Vec::new, |ids| ids.iter().filter_map(Value::as_i64).collect());

      order_rules(mail.standing.store.pool(), &mail.account.address, &ids).await?;

      Ok(configured(json!({})))
    })
    .await
  }
}

crate::route!(OrderRules);

/// `POST /api/mail/{account}/rules/run`: the rules run over one folder, for the mail that came before them.
pub struct RunRules;

impl Route for RunRules {
  const METHOD: Method = Method::POST;
  const PATH: &'static str = "/api/mail/{account}/rules/run";

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let key = call.path_param("account").await?;
    let body = call.body().await;
    let standing = call.standing().clone();

    with_mail_account(&standing, key.as_deref(), |mail| async move {
      let folder = body
        .get("folder")
        .and_then(Value::as_str)
        .filter(|folder| !folder.is_empty())
        .unwrap_or("INBOX");
      let (kept, moved) =
        run_rules_over_folder(mail.standing.store.pool(), &mail.session, &mail.account.address, folder)
          .await?;

      Ok(configured(json!({ "kept": kept, "moved": moved })))
    })
    .await
  }
}

crate::route!(RunRules);

/// `POST /api/mail/{account}/spam-senders`: senders whose mail always goes to spam.
pub struct AddSpamSenders;

impl Route for AddSpamSenders {
  const METHOD: Method = Method::POST;
  const PATH: &'static str = "/api/mail/{account}/spam-senders";

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let key = call.path_param("account").await?;
    let body = call.body().await;
    let standing = call.standing().clone();

    with_mail_account(&standing, key.as_deref(), |mail| async move {
      let pool = mail.standing.store.pool();

      add_spam_senders(pool, &mail.account.address, &strings_of(body.get("add"))).await?;

      Ok(configured(
        json!({ "spamSenders": list_spam_senders(pool, &mail.account.address).await? }),
      ))
    })
    .await
  }
}

crate::route!(AddSpamSenders);

/// `DELETE /api/mail/{account}/spam-senders`: a sender taken off the always-spam list.
pub struct RemoveSpamSender;

impl Route for RemoveSpamSender {
  const METHOD: Method = Method::DELETE;
  const PATH: &'static str = "/api/mail/{account}/spam-senders";

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let key = call.path_param("account").await?;
    let body = call.body().await;
    let standing = call.standing().clone();

    with_mail_account(&standing, key.as_deref(), |mail| async move {
      let pool = mail.standing.store.pool();
      let address = body.get("address").and_then(Value::as_str).unwrap_or("");

      remove_spam_sender(pool, &mail.account.address, address).await?;

      Ok(configured(
        json!({ "spamSenders": list_spam_senders(pool, &mail.account.address).await? }),
      ))
    })
    .await
  }
}

crate::route!(RemoveSpamSender);

/// `GET /api/mail/{account}/settings`: the account's signature, whether a read receipt is asked by default,
/// whether outside pictures are shown, and its always-spam senders.
pub struct Settings;

impl Route for Settings {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/api/mail/{account}/settings";

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let key = call.path_param("account").await?;
    let standing = call.standing().clone();

    with_mail_account(&standing, key.as_deref(), |mail| async move {
      let pool = mail.standing.store.pool();
      let settings = read_settings(pool, &mail.account.address).await?;
      let spam_senders = list_spam_senders(pool, &mail.account.address).await?;

      Ok(configured(json!({
        "signature": settings.signature,
        "receiptDefault": settings.receipt_default,
        "showImages": settings.show_images,
        "spamSenders": spam_senders,
      })))
    })
    .await
  }
}

crate::route!(Settings);

/// `POST /api/mail/{account}/settings`: the signature, the read-receipt default, whether outside pictures
/// are shown, or any of them.
pub struct SaveSettings;

impl Route for SaveSettings {
  const METHOD: Method = Method::POST;
  const PATH: &'static str = "/api/mail/{account}/settings";

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let key = call.path_param("account").await?;
    let body = call.body().await;
    let standing = call.standing().clone();

    with_mail_account(&standing, key.as_deref(), |mail| async move {
      let pool = mail.standing.store.pool();
      let held = read_settings(pool, &mail.account.address).await?;
      let signature = body.get("signature").and_then(Value::as_str).map_or(held.signature, str::to_owned);

      if signature.encode_utf16().count() > SIGNATURE_LIMIT {
        return Err(
          Asked::new("signature_too_long", "the signature is past the limit")
            .with("n", SIGNATURE_LIMIT)
            .into(),
        );
      }

      let next = MailSettings {
        signature,
        receipt_default: body
          .get("receiptDefault")
          .and_then(Value::as_bool)
          .unwrap_or(held.receipt_default),
        show_images: body.get("showImages").and_then(Value::as_bool).unwrap_or(held.show_images),
      };

      write_settings(pool, &mail.account.address, &next).await?;

      Ok(configured(json!(next)))
    })
    .await
  }
}

crate::route!(SaveSettings);
