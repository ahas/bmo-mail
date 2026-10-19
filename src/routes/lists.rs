//! Lists and messages (`docs/api.md`, 「Lists and messages」).

use axum::http::Method;
use axum::response::Response;
use serde_json::{Value, json};

use crate::http::{Call, Refused, Route};
use crate::imap::messages::{
  NameOrder, find_inline_part, folder_states, open_part, read_message, read_thread,
};
use crate::imap::views::{mark_view_read, view_page};
use crate::labels::list_labels;
use crate::routes::downloads::{Disposition, part_answer};
use crate::routes::requests::{
  MailFault, configured, offset_of, ref_of, sort_of, view_of, with_mail_account,
};

/// `GET /api/mail/{account}/folders`: one account's folders with their unread and total counts, and its
/// labels, for the mail nav.
pub struct Folders;

impl Route for Folders {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/api/mail/{account}/folders";

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let key = call.path_param("account").await?;
    let standing = call.standing().clone();

    with_mail_account(&standing, key.as_deref(), |mail| async move {
      let folders = folder_states(&mail.session).await?;
      let labels = list_labels(mail.standing.store.pool(), &mail.account.address).await?;

      Ok(configured(json!({ "folders": folders, "labels": labels })))
    })
    .await
  }
}

crate::route!(Folders);

/// `GET /api/mail/{account}/list`: one page of a list: a folder, starred, a label or a search
/// (`docs/mail.md`, 「Reading」 and 「Search」).
pub struct List;

impl Route for List {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/api/mail/{account}/list";

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let key = call.path_param("account").await?;
    let query = call.query();
    let standing = call.standing().clone();

    with_mail_account(&standing, key.as_deref(), |mail| async move {
      let view = view_of(&query)?;
      let order = NameOrder::new(&mail.standing.settings.wording.language);
      let page = view_page(
        mail.standing.store.pool(),
        &mail.session,
        &mail.account.address,
        &view,
        offset_of(query.get("offset")),
        sort_of(&query),
        &order,
      )
      .await?;

      Ok(configured(json!(page)))
    })
    .await
  }
}

crate::route!(List);

/// `GET /api/mail/{account}/thread`: every message of one conversation, oldest first, with its bodies.
/// Reading it marks nothing read; the screen says so itself.
pub struct Thread;

impl Route for Thread {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/api/mail/{account}/thread";

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let key = call.path_param("account").await?;
    let query = call.query();
    let standing = call.standing().clone();

    with_mail_account(&standing, key.as_deref(), |mail| async move {
      let conversation = query.get("key").and_then(Value::as_str).unwrap_or("");
      let folder = query.get("folder").and_then(Value::as_str).unwrap_or("INBOX");
      let messages = read_thread(&mail.session, conversation, folder).await?;

      Ok(configured(json!({ "messages": messages })))
    })
    .await
  }
}

crate::route!(Thread);

/// `GET /api/mail/{account}/message`: one message with its bodies.
pub struct Message;

impl Route for Message {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/api/mail/{account}/message";

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let key = call.path_param("account").await?;
    let query = call.query();
    let standing = call.standing().clone();

    with_mail_account(&standing, key.as_deref(), |mail| async move {
      let at = ref_of(&query)?;
      let Some(message) = read_message(&mail.session, &at).await? else {
        return Err(MailFault::asked("message_not_found", "the message is not on the server"));
      };

      Ok(configured(json!({ "message": message })))
    })
    .await
  }
}

crate::route!(Message);

/// `GET /api/mail/{account}/part`: an attachment, from the mail server as a download: never rendered in the
/// console's own origin, never sniffed into another type, and sandboxed if a browser opens it anyway.
pub struct Part;

impl Route for Part {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/api/mail/{account}/part";

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let key = call.path_param("account").await?;
    let query = call.query();
    let standing = call.standing().clone();

    with_mail_account(&standing, key.as_deref(), |mail| async move {
      let at = ref_of(&query)?;
      let part = query.get("part").and_then(Value::as_str).unwrap_or("");
      let opened = open_part(&mail.session, &at, part).await?;

      Ok(part_answer(
        opened.bytes,
        opened.kind.as_deref(),
        opened.filename.as_deref(),
        Disposition::Attachment,
      ))
    })
    .await
  }
}

crate::route!(Part);

/// `GET /api/mail/{account}/cid`: an image an HTML body points at by `cid:`, which the body's frame loads
/// from here. Only an image is served this way; anything else a `cid:` names is not shown inline at all.
pub struct Cid;

impl Route for Cid {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/api/mail/{account}/cid";

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let key = call.path_param("account").await?;
    let query = call.query();
    let standing = call.standing().clone();

    with_mail_account(&standing, key.as_deref(), |mail| async move {
      let at = ref_of(&query)?;
      let cid = query.get("cid").and_then(Value::as_str).unwrap_or("");
      let Some(part) = find_inline_part(&mail.session, &at, cid)
        .await?
        .filter(|part| part.kind.starts_with("image/"))
      else {
        return Err(MailFault::asked("inline_image_not_found", "no image of the message has that cid"));
      };
      let opened = open_part(&mail.session, &at, &part.part).await?;

      Ok(part_answer(
        opened.bytes,
        Some(&part.kind),
        part.filename.as_deref(),
        Disposition::Inline,
      ))
    })
    .await
  }
}

crate::route!(Cid);

/// `POST /api/mail/{account}/read-all`: every unread message the folder, the label or the search holds,
/// marked read where it lies, not only the page the list had read (`docs/mail.md`, 「Reading」).
pub struct ReadAll;

impl Route for ReadAll {
  const METHOD: Method = Method::POST;
  const PATH: &'static str = "/api/mail/{account}/read-all";

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let key = call.path_param("account").await?;
    let body = call.body().await;
    let standing = call.standing().clone();

    with_mail_account(&standing, key.as_deref(), |mail| async move {
      let view = view_of(&body)?;
      let marked =
        mark_view_read(mail.standing.store.pool(), &mail.session, &mail.account.address, &view).await?;

      Ok(configured(json!({ "marked": marked })))
    })
    .await
  }
}

crate::route!(ReadAll);
