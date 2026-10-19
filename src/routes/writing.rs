//! Writing (`docs/api.md`, 「Writing」).

use std::sync::Arc;

use axum::extract::{FromRequest, Multipart};
use axum::http::Method;
use axum::response::Response;
use serde_json::{Value, json};

use crate::account_settings::read_settings;
use crate::http::{Call, Refused, Route};
use crate::imap::messages::read_message;
use crate::routes::requests::{MailFault, configured, draft_of, ref_of, with_mail_account};
use crate::writing::addresses::suggest_addresses;
use crate::writing::compose::{forward_draft, new_draft, reply_draft};
use crate::writing::drafts::{discard_draft, open_draft, save_draft};
use crate::writing::receipts::answer_receipt;
use crate::writing::send::{SEND_HOLD, queue_send};
use crate::writing::staging::{attachments_fit, stage_file};

/// `GET /api/mail/{account}/addresses`: completion for an address field, from the addresses the account
/// has used and heard from.
pub struct Addresses;

impl Route for Addresses {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/api/mail/{account}/addresses";

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let key = call.path_param("account").await?;
    let query = call.query();
    let standing = call.standing().clone();

    with_mail_account(&standing, key.as_deref(), |mail| async move {
      let typed = query.get("q").and_then(Value::as_str).unwrap_or("");
      let addresses = suggest_addresses(mail.standing.store.pool(), &mail.account.address, typed).await?;

      Ok(configured(json!({ "addresses": addresses })))
    })
    .await
  }
}

crate::route!(Addresses);

/// `GET /api/mail/{account}/draft`: a saved draft, reopened in the composer.
pub struct OpenDraft;

impl Route for OpenDraft {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/api/mail/{account}/draft";

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let key = call.path_param("account").await?;
    let query = call.query();
    let standing = call.standing().clone();

    with_mail_account(&standing, key.as_deref(), |mail| async move {
      let draft = open_draft(&mail.session, &ref_of(&query)?).await?;

      Ok(configured(json!({ "draft": draft })))
    })
    .await
  }
}

crate::route!(OpenDraft);

/// `POST /api/mail/{account}/draft`: the draft saved, replacing the copy it was saved as before; answers
/// where it now is.
pub struct SaveDraft;

impl Route for SaveDraft {
  const METHOD: Method = Method::POST;
  const PATH: &'static str = "/api/mail/{account}/draft";

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let key = call.path_param("account").await?;
    let body = call.body().await;
    let standing = call.standing().clone();

    with_mail_account(&standing, key.as_deref(), |mail| async move {
      let draft = draft_of(&body, &mail.account)?;
      let saved = save_draft(&mail.session, &mail.standing.settings.staging.dir, &draft).await?;

      Ok(configured(json!({ "saved": saved })))
    })
    .await
  }
}

crate::route!(SaveDraft);

/// `DELETE /api/mail/{account}/draft`: a draft thrown away.
pub struct DiscardDraft;

impl Route for DiscardDraft {
  const METHOD: Method = Method::DELETE;
  const PATH: &'static str = "/api/mail/{account}/draft";

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let key = call.path_param("account").await?;
    let body = call.body().await;
    let standing = call.standing().clone();

    with_mail_account(&standing, key.as_deref(), |mail| async move {
      discard_draft(&mail.session, &ref_of(&body)?).await?;

      Ok(configured(json!({})))
    })
    .await
  }
}

crate::route!(DiscardDraft);

/// `POST /api/mail/{account}/draft/start`: what a composer opens with: a new message, a reply, a reply to
/// all, or a forward of a message, each with the account's signature (`docs/mail.md`, 「Writing」).
pub struct StartDraft;

impl Route for StartDraft {
  const METHOD: Method = Method::POST;
  const PATH: &'static str = "/api/mail/{account}/draft/start";

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let key = call.path_param("account").await?;
    let body = call.body().await;
    let standing = call.standing().clone();

    with_mail_account(&standing, key.as_deref(), |mail| async move {
      let settings = read_settings(mail.standing.store.pool(), &mail.account.address).await?;
      let from = &mail.account.address;
      let kind = body.get("kind").and_then(Value::as_str);

      if kind.is_none() || kind == Some("new") {
        return Ok(configured(json!({ "draft": new_draft(from, &settings) })));
      }

      let Some(original) = read_message(&mail.session, &ref_of(&body)?).await? else {
        return Err(MailFault::asked("message_not_found", "the message is not on the server"));
      };
      let wording = &mail.standing.wording;
      let draft = match kind {
        Some("reply") => reply_draft(&original, from, false, &settings, wording),
        Some("reply-all") => reply_draft(&original, from, true, &settings, wording),
        Some("forward") => forward_draft(&original, from, &settings, wording),
        _ => return Err(MailFault::asked("bad_compose_mode", "the kind is none a composer opens as")),
      };

      Ok(configured(json!({ "draft": draft })))
    })
    .await
  }
}

crate::route!(StartDraft);

/// One field of an upload's form.
struct FormField {
  name: String,
  filename: Option<String>,
  kind: Option<String>,
  bytes: Vec<u8>,
}

/// The fields of a multipart form, none where the body is not one.
async fn form_fields(call: Call) -> Vec<FormField> {
  let Ok(mut form) = Multipart::from_request(call.request, &()).await else {
    return Vec::new();
  };
  let mut fields = Vec::new();

  while let Ok(Some(field)) = form.next_field().await {
    let name = field.name().unwrap_or("").to_owned();
    let filename = field.file_name().map(str::to_owned);
    let kind = field.content_type().map(str::to_owned);
    let Ok(bytes) = field.bytes().await else {
      break;
    };

    fields.push(FormField {
      name,
      filename,
      kind,
      bytes: bytes.to_vec(),
    });
  }

  fields
}

/// `POST /api/mail/{account}/upload`: files attached while a message is written, put in the staging folder
/// until the draft is saved. They are refused whole when the message would no longer fit what the mail
/// server takes, counting the attachments the message already carries (the form's `attached`, their sizes).
pub struct Upload;

impl Route for Upload {
  const METHOD: Method = Method::POST;
  const PATH: &'static str = "/api/mail/{account}/upload";

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let key = call.path_param("account").await?;
    let standing = call.standing().clone();
    let fields = form_fields(call).await;

    with_mail_account(&standing, key.as_deref(), |mail| async move {
      let files: Vec<&FormField> =
        fields.iter().filter(|field| field.name == "file" && field.filename.is_some()).collect();
      let held: Vec<u64> = fields
        .iter()
        .find(|field| field.name == "attached")
        .and_then(|field| serde_json::from_slice::<Vec<Value>>(&field.bytes).ok())
        .map(|sizes| sizes.iter().filter_map(Value::as_f64).map(|size| size.max(0.0) as u64).collect())
        .unwrap_or_default();

      if files.is_empty() {
        return Err(MailFault::asked("no_upload", "an upload carries no file"));
      }

      let sizes: Vec<u64> =
        held.iter().copied().chain(files.iter().map(|file| file.bytes.len() as u64)).collect();
      let limit = mail.standing.sends.submission_limit(&mail.config.smtp).await;

      if !attachments_fit(&sizes, limit) {
        return Err(MailFault::asked(
          "upload_too_large",
          "the files would not fit the size the server takes",
        ));
      }

      let staging = &mail.standing.settings.staging.dir;
      let staged = files
        .iter()
        .map(|file| {
          let kind = file.kind.as_deref().unwrap_or("application/octet-stream");

          stage_file(staging, file.filename.as_deref().unwrap_or(""), kind, &file.bytes)
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(MailFault::server)?;

      Ok(configured(json!({ "files": staged })))
    })
    .await
  }
}

crate::route!(Upload);

/// `POST /api/mail/{account}/send`: the draft saved and the message held for the hold's length, while undo
/// send can still take it back (`docs/mail.md`, 「Writing」).
pub struct Send;

impl Route for Send {
  const METHOD: Method = Method::POST;
  const PATH: &'static str = "/api/mail/{account}/send";

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let key = call.path_param("account").await?;
    let body = call.body().await;
    let standing = call.standing().clone();

    with_mail_account(&standing, key.as_deref(), |mail| async move {
      let draft = draft_of(&body, &mail.account)?;
      let smtp = mail.config.smtp.clone();
      let (id, saved) =
        queue_send(Arc::clone(&mail.standing), Arc::clone(&mail.session), smtp, draft).await?;

      Ok(configured(json!({ "id": id, "draft": saved, "holdMs": SEND_HOLD.as_millis() })))
    })
    .await
  }
}

crate::route!(Send);

/// `GET /api/mail/{account}/send/{id}`: where a send stands: held, sent, taken back or failed; null once it
/// has long ended.
pub struct SendStanding;

impl Route for SendStanding {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/api/mail/{account}/send/{id}";

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let mut params = call.path_params().await?;
    let key = params.remove("account");
    let id = params.remove("id").unwrap_or_default();
    let standing = call.standing().clone();

    with_mail_account(&standing, key.as_deref(), |mail| async move {
      Ok(configured(json!({ "state": mail.standing.sends.state(&id) })))
    })
    .await
  }
}

crate::route!(SendStanding);

/// `DELETE /api/mail/{account}/send/{id}`: undo send: answers the draft to reopen, or null when the message
/// has already gone.
pub struct UndoSend;

impl Route for UndoSend {
  const METHOD: Method = Method::DELETE;
  const PATH: &'static str = "/api/mail/{account}/send/{id}";

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let mut params = call.path_params().await?;
    let key = params.remove("account");
    let id = params.remove("id").unwrap_or_default();
    let standing = call.standing().clone();

    with_mail_account(&standing, key.as_deref(), |mail| async move {
      Ok(configured(json!({ "draft": mail.standing.sends.cancel(&id) })))
    })
    .await
  }
}

crate::route!(UndoSend);

/// `POST /api/mail/{account}/receipt`: send or don't send, on a message's read-receipt bar. Nothing is sent
/// without that press.
pub struct Receipt;

impl Route for Receipt {
  const METHOD: Method = Method::POST;
  const PATH: &'static str = "/api/mail/{account}/receipt";

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let key = call.path_param("account").await?;
    let body = call.body().await;
    let standing = call.standing().clone();

    with_mail_account(&standing, key.as_deref(), |mail| async move {
      let at = ref_of(&body)?;
      let send = matches!(body.get("send"), Some(Value::Bool(true)));

      answer_receipt(&mail.session, &mail.config.smtp, &mail.standing.wording, &at, send).await?;

      Ok(configured(json!({})))
    })
    .await
  }
}

crate::route!(Receipt);
