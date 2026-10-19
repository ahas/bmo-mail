//! Changing messages (`docs/api.md`, 「Changing messages」).

use axum::http::Method;
use axum::response::Response;
use serde_json::{Value, json};

use crate::http::{Call, Refused, Route};
use crate::imap::messages::{
  change_flags, delete_forever, move_messages, move_to_role, report_not_spam, report_spam,
};
use crate::routes::requests::{MailFault, configured, refs_of, strings_of, with_mail_account};
use crate::wire::FolderRole;

/// `POST /api/mail/{account}/flags`: read, unread, star, a label's keyword on or off, on every message
/// chosen, in whichever folders.
pub struct Flags;

impl Route for Flags {
  const METHOD: Method = Method::POST;
  const PATH: &'static str = "/api/mail/{account}/flags";

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let key = call.path_param("account").await?;
    let body = call.body().await;
    let standing = call.standing().clone();

    with_mail_account(&standing, key.as_deref(), |mail| async move {
      let refs = refs_of(body.get("refs"))?;

      change_flags(&mail.session, &refs, &strings_of(body.get("add")), &strings_of(body.get("remove")))
        .await?;

      Ok(configured(json!({})))
    })
    .await
  }
}

crate::route!(Flags);

/// `POST /api/mail/{account}/move`: the chosen messages moved to trash, to spam as spam reported, back to
/// the inbox as not spam, or to a folder by its path.
pub struct Move;

impl Route for Move {
  const METHOD: Method = Method::POST;
  const PATH: &'static str = "/api/mail/{account}/move";

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let key = call.path_param("account").await?;
    let body = call.body().await;
    let standing = call.standing().clone();

    with_mail_account(&standing, key.as_deref(), |mail| async move {
      let refs = refs_of(body.get("refs"))?;
      let folder = body.get("folder").and_then(Value::as_str).filter(|folder| !folder.is_empty());

      match (body.get("to").and_then(Value::as_str), folder) {
        (Some("trash"), _) => move_to_role(&mail.session, &refs, FolderRole::Trash).await.map(drop)?,
        (Some("spam"), _) => report_spam(&mail.session, &refs).await?,
        (Some("not-spam"), _) => report_not_spam(&mail.session, &refs).await?,
        (Some("folder"), Some(folder)) => move_messages(&mail.session, &refs, folder).await?,
        _ => return Err(MailFault::asked("bad_move_target", "the move names no place mail can go")),
      }

      Ok(configured(json!({})))
    })
    .await
  }
}

crate::route!(Move);

/// `POST /api/mail/{account}/delete`: deletion for good, which only mail in spam and trash allows.
pub struct Delete;

impl Route for Delete {
  const METHOD: Method = Method::POST;
  const PATH: &'static str = "/api/mail/{account}/delete";

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let key = call.path_param("account").await?;
    let body = call.body().await;
    let standing = call.standing().clone();

    with_mail_account(&standing, key.as_deref(), |mail| async move {
      delete_forever(&mail.session, &refs_of(body.get("refs"))?).await?;

      Ok(configured(json!({})))
    })
    .await
  }
}

crate::route!(Delete);
