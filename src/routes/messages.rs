//! Messages and threads (`docs/api.md`, 「Messages and threads」): a page of them, one with its bodies, its
//! flags, labels and folder changed, its deletion, a part's bytes, and the conversation it belongs to.

use axum::http::{Method, StatusCode};
use axum::response::Response;

use crate::announcements::{MESSAGE_CHANGED, folder_now, message_gone};
use crate::http::{Answer, Call, Operation, Refused, Route, answer, inline, nothing, schema};
use crate::keys::thread_key;
use crate::labels::{label_id_of, label_keyword, list_labels};
use crate::members::message;
use crate::routes::downloads::part_answer;
use crate::routes::requests::{Asked, message_at, with_account};
use crate::wire::{Message, MessagePatch, MessageQuery, Page, Thread, ThreadQuery};

const ACCOUNT: (&str, &str) = ("accountName", "The account's name, unique among its owner's accounts.");
const MESSAGE: (&str, &str) = ("messageId", "The message's id, opaque.");

/// `GET /accounts/{accountName}/messages`.
pub struct Messages;

impl Route for Messages {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/accounts/{accountName}/messages";
  const OPERATION: Operation = Operation {
    id: "listAccountsMessagesByAccountName",
    summary: "Messages of a folder or matching words, newest first, without their bodies.",
    tag: "accounts",
    keys: &[ACCOUNT],
    query: Some(inline::<MessageQuery>),
    body: None,
    status: 200,
    answer: Answer::Json(schema::<Page<Message>>),
    codes: &["not-found"],
    owned: true,
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let query: MessageQuery = call.query()?;

    with_account(&mut call, |mail| async move {
      let (read, more) = mail.mailbox.page(&query).await?;
      let items = read.into_iter().map(|one| message(mail.name(), one)).collect();

      Ok(answer(StatusCode::OK, &Page { items, more }))
    })
    .await
  }
}

crate::route!(Messages);

/// `GET /accounts/{accountName}/messages/{messageId}`.
pub struct ReadMessage;

impl Route for ReadMessage {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/accounts/{accountName}/messages/{messageId}";
  const OPERATION: Operation = Operation {
    id: "readAccountsMessagesByAccountNameMessageId",
    summary: "One message with its bodies.",
    tag: "accounts",
    keys: &[ACCOUNT, MESSAGE],
    query: None,
    body: None,
    status: 200,
    answer: Answer::Json(schema::<Message>),
    codes: &["not-found"],
    owned: true,
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let id = call.key("messageId").await?;

    with_account(&mut call, |mail| async move {
      let at = message_at(&id)?;
      let Some(read) = mail.mailbox.message(&at).await? else {
        return Err(Asked::not_found("no message has that id").into());
      };

      Ok(answer(StatusCode::OK, &message(mail.name(), read)))
    })
    .await
  }
}

crate::route!(ReadMessage);

/// `PATCH /accounts/{accountName}/messages/{messageId}`: the whole of the flags and labels it should carry,
/// then the folder it should be in.
pub struct ChangeMessage;

impl Route for ChangeMessage {
  const METHOD: Method = Method::PATCH;
  const PATH: &'static str = "/accounts/{accountName}/messages/{messageId}";
  const OPERATION: Operation = Operation {
    id: "changeAccountsMessagesByAccountNameMessageId",
    summary: "Change a message's flags or labels, or move it to a folder.",
    tag: "accounts",
    keys: &[ACCOUNT, MESSAGE],
    query: None,
    body: Some(schema::<MessagePatch>),
    status: 200,
    answer: Answer::Json(schema::<Message>),
    codes: &["not-found", "flag-not-settable", "unknown-label", "unknown-folder"],
    owned: true,
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let id = call.key("messageId").await?;
    let patch: MessagePatch = call.body().await?;

    with_account(&mut call, |mail| async move {
      let at = message_at(&id)?;
      let labels = list_labels(mail.standing.store.pool(), &mail.key()).await?;

      if let Some(unknown) = patch.label_ids.iter().find(|id| !labels.iter().any(|label| &label.id == *id))
      {
        return Err(Asked::rule("unknown-label", format!("the account holds no label {unknown}")).into());
      }

      let mut wanted: Vec<String> =
        patch.flags.iter().filter(|flag| label_id_of(flag).is_none()).cloned().collect();

      wanted.extend(patch.label_ids.iter().map(|id| label_keyword(id)));

      let destination = patch.folder_name.filter(|folder| *folder != at.folder);

      if let Some(folder) = &destination
        && !mail.mailbox.folders().await?.iter().any(|one| &one.path == folder)
      {
        return Err(Asked::rule("unknown-folder", "the mailbox holds no folder of that name").into());
      }

      if !mail.mailbox.set_flags(&at, &wanted).await? {
        return Err(Asked::not_found("no message has that id").into());
      }

      let now = match &destination {
        None => Some(at.clone()),
        Some(folder) => {
          let read = mail.mailbox.row(&at).await?;
          let header_id = read.as_ref().and_then(|read| read.listed.message_id.clone());
          let landed = mail.mailbox.move_message(&at, header_id.as_deref(), folder).await?;

          message_gone(&mail.standing, &mail.account, &at).await;
          folder_now(&mail.standing, &*mail.mailbox, &mail.account, folder).await;
          landed
        }
      };

      folder_now(&mail.standing, &*mail.mailbox, &mail.account, &at.folder).await;

      let Some(now) = now else {
        return Ok(nothing());
      };
      let Some(read) = mail.mailbox.row(&now).await? else {
        return Ok(nothing());
      };
      let changed = message(mail.name(), read);

      let owner = &mail.account.owner;

      mail.standing.events.publish(MESSAGE_CHANGED, owner, Some(mail.name()), &changed).await;

      Ok(answer(StatusCode::OK, &changed))
    })
    .await
  }
}

crate::route!(ChangeMessage);

/// `DELETE /accounts/{accountName}/messages/{messageId}`: deletion for good, from spam or trash alone.
pub struct DeleteMessage;

impl Route for DeleteMessage {
  const METHOD: Method = Method::DELETE;
  const PATH: &'static str = "/accounts/{accountName}/messages/{messageId}";
  const OPERATION: Operation = Operation {
    id: "deleteAccountsMessagesByAccountNameMessageId",
    summary: "Delete a message for good; only a message in spam or trash.",
    tag: "accounts",
    keys: &[ACCOUNT, MESSAGE],
    query: None,
    body: None,
    status: 204,
    answer: Answer::Nothing,
    codes: &["not-found", "outside-trash"],
    owned: true,
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let id = call.key("messageId").await?;

    with_account(&mut call, |mail| async move {
      let at = message_at(&id)?;

      mail.mailbox.delete_forever(&at).await?;
      message_gone(&mail.standing, &mail.account, &at).await;
      folder_now(&mail.standing, &*mail.mailbox, &mail.account, &at.folder).await;

      Ok(nothing())
    })
    .await
  }
}

crate::route!(DeleteMessage);

/// `GET /accounts/{accountName}/messages/{messageId}/parts/{partId}/content`: a part's bytes, by its number
/// as an attachment, by its content id inline.
pub struct PartContent;

impl Route for PartContent {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/accounts/{accountName}/messages/{messageId}/parts/{partId}/content";
  const OPERATION: Operation = Operation {
    id: "listAccountsMessagesPartsContentByAccountNameMessageIdPartId",
    summary: "A part's bytes, found by its number or its content id.",
    tag: "accounts",
    keys: &[ACCOUNT, MESSAGE, ("partId", "The part's number in the message, or its content id.")],
    query: None,
    body: None,
    status: 200,
    answer: Answer::Bytes,
    codes: &["not-found"],
    owned: true,
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let id = call.key("messageId").await?;
    let part = call.key("partId").await?;

    with_account(&mut call, |mail| async move {
      let at = message_at(&id)?;
      let Some((opened, disposition)) = mail.mailbox.part(&at, &part).await? else {
        return Err(Asked::not_found("no part of the message has that id").into());
      };

      Ok(part_answer(
        opened.bytes,
        opened.kind.as_deref(),
        opened.filename.as_deref(),
        disposition,
      ))
    })
    .await
  }
}

crate::route!(PartContent);

/// `GET /accounts/{accountName}/threads/{threadId}`.
pub struct ReadThread;

impl Route for ReadThread {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/accounts/{accountName}/threads/{threadId}";
  const OPERATION: Operation = Operation {
    id: "readAccountsThreadsByAccountNameThreadId",
    summary: "One thread and its messages, each with its bodies.",
    tag: "accounts",
    keys: &[ACCOUNT, ("threadId", "The conversation's id, opaque.")],
    query: Some(inline::<ThreadQuery>),
    body: None,
    status: 200,
    answer: Answer::Json(schema::<Thread>),
    codes: &["not-found"],
    owned: true,
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let id = call.key("threadId").await?;
    let query: ThreadQuery = call.query()?;

    with_account(&mut call, |mail| async move {
      let Some(key) = thread_key(&id) else {
        return Err(Asked::not_found("no thread has that id").into());
      };
      let read = mail.mailbox.thread(&key, query.folder_name.as_deref()).await?;
      let Some(first) = read.first() else {
        return Err(Asked::not_found("no thread has that id").into());
      };
      let subject = first.listed.subject.clone();
      let messages = read.into_iter().map(|one| message(mail.name(), one)).collect();

      Ok(answer(
        StatusCode::OK,
        &Thread {
          id,
          account_name: mail.name().to_owned(),
          subject,
          messages,
        },
      ))
    })
    .await
  }
}

crate::route!(ReadThread);
