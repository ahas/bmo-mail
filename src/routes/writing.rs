//! Writing (`docs/api.md`, 「Writing」): the addresses to complete from, drafts saved on the server, files
//! attached to them, sends that wait out the undo span, and read receipts.

use std::sync::Arc;

use axum::http::{Method, StatusCode};
use axum::response::Response;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use crate::account_settings::read_settings;
use crate::http::{Answer, Call, Operation, Refused, Route, answer, inline, nothing, schema};
use crate::mail::{AttachmentRef, MailAddress};
use crate::members::address_of;
use crate::routes::requests::{
  Asked, MailContext, MailFault, address_shaped, message_at, one_line, with_account,
};
use crate::wire::{
  Address, AddressQuery, Draft, DraftPatch, Mailbox, NewAttachment, NewDraft, NewReceipt, NewSend, Page,
  Receipt, Send,
};
use crate::writing::addresses::suggest_addresses;
use crate::writing::compose::{forward_draft, new_draft, reply_draft};
use crate::writing::drafts::member;
use crate::writing::send::{SEND_CHANGED, forget_later, queue_send};
use crate::writing::staging::{attachments_fit, stage_file};

const ACCOUNT: (&str, &str) = ("accountName", "The account's name, unique among its owner's accounts.");
const DRAFT: (&str, &str) =
  ("draftId", "The draft's id: the Message-ID it is saved under, without angle brackets.");
const SEND: (&str, &str) = ("sendId", "The send's id.");

/// How many addresses a completion offers where none is asked.
const SUGGESTED: u32 = 10;

/// `GET /accounts/{accountName}/addresses`.
pub struct Addresses;

impl Route for Addresses {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/accounts/{accountName}/addresses";
  const OPERATION: Operation = Operation {
    id: "listAccountsAddressesByAccountName",
    summary: "Addresses written to or heard from, most used first.",
    tag: "accounts",
    keys: &[ACCOUNT],
    query: Some(inline::<AddressQuery>),
    body: None,
    status: 200,
    answer: Answer::Json(schema::<Page<Address>>),
    codes: &["not-found"],
    owned: true,
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let query: AddressQuery = call.query()?;

    with_account(&mut call, |mail| async move {
      let limit = query.limit.unwrap_or(SUGGESTED).max(1);
      let typed = query.q.unwrap_or_default();
      let pool = mail.standing.store.pool();
      let mut items = suggest_addresses(pool, &mail.key(), &typed, query.after.unwrap_or(0), limit).await?;
      let more = items.len() > limit as usize;

      items.truncate(limit as usize);

      Ok(answer(StatusCode::OK, &Page { items, more }))
    })
    .await
  }
}

crate::route!(Addresses);

/// Mailboxes as a draft carries them, each address checked.
fn recipients(list: &[Mailbox]) -> Result<Vec<MailAddress>, Asked> {
  list
    .iter()
    .map(|one| {
      let address = address_of(one);

      if address_shaped(&address.address) {
        Ok(address)
      } else {
        Err(Asked::bad_request(format!("{} is not an address", one.address)))
      }
    })
    .collect()
}

/// The message a draft's start names, read with its bodies; a 422 where the account holds none.
async fn original(mail: &MailContext, id: &str) -> Result<crate::mail::MailMessage, MailFault> {
  let unknown = || Asked::rule("unknown-message", "the account holds no message of that id");
  let at = message_at(id).map_err(|_| unknown())?;

  mail.mailbox.message(&at).await?.ok_or_else(|| unknown().into())
}

/// `POST /accounts/{accountName}/drafts`: a draft started, saved in the drafts folder at once.
pub struct CreateDraft;

impl Route for CreateDraft {
  const METHOD: Method = Method::POST;
  const PATH: &'static str = "/accounts/{accountName}/drafts";
  const OPERATION: Operation = Operation {
    id: "createAccountsDraftsByAccountName",
    summary: "Start a draft, new or answering or carrying a message.",
    tag: "accounts",
    keys: &[ACCOUNT],
    query: None,
    body: Some(schema::<NewDraft>),
    status: 201,
    answer: Answer::Json(schema::<Draft>),
    codes: &["not-found", "unknown-message", "draft-not-placed"],
    owned: true,
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let new: NewDraft = call.body().await?;

    with_account(&mut call, |mail| async move {
      let standing = &mail.standing;
      let from = &mail.account.address;
      let settings = read_settings(standing.store.pool(), &mail.key()).await?;
      let draft = match (&new.reply_to, &new.forward) {
        (Some(_), Some(_)) => {
          return Err(Asked::bad_request("a draft answers a message or carries one, not both").into());
        }
        (Some(id), None) => {
          reply_draft(&original(&mail, id).await?, from, new.reply_all, &settings, &standing.wording)
        }
        (None, Some(id)) => forward_draft(&original(&mail, id).await?, from, &settings, &standing.wording),
        (None, None) => new_draft(from, &settings),
      };
      let saved = mail.mailbox.save_draft(&draft).await?;

      Ok(answer(StatusCode::CREATED, &member(mail.name(), &saved)))
    })
    .await
  }
}

crate::route!(CreateDraft);

/// `GET /accounts/{accountName}/drafts/{draftId}`.
pub struct ReadDraft;

impl Route for ReadDraft {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/accounts/{accountName}/drafts/{draftId}";
  const OPERATION: Operation = Operation {
    id: "readAccountsDraftsByAccountNameDraftId",
    summary: "One draft.",
    tag: "accounts",
    keys: &[ACCOUNT, DRAFT],
    query: None,
    body: None,
    status: 200,
    answer: Answer::Json(schema::<Draft>),
    codes: &["not-found"],
    owned: true,
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let id = call.key("draftId").await?;

    with_account(&mut call, |mail| async move {
      let draft = mail.mailbox.draft(&id).await?;

      Ok(answer(StatusCode::OK, &member(mail.name(), &draft)))
    })
    .await
  }
}

crate::route!(ReadDraft);

/// `PATCH /accounts/{accountName}/drafts/{draftId}`: the fields given replace the draft's own, and the draft
/// is saved anew under the same id.
pub struct ChangeDraft;

impl Route for ChangeDraft {
  const METHOD: Method = Method::PATCH;
  const PATH: &'static str = "/accounts/{accountName}/drafts/{draftId}";
  const OPERATION: Operation = Operation {
    id: "changeAccountsDraftsByAccountNameDraftId",
    summary: "Change a draft, saved again under the same id.",
    tag: "accounts",
    keys: &[ACCOUNT, DRAFT],
    query: None,
    body: Some(schema::<DraftPatch>),
    status: 200,
    answer: Answer::Json(schema::<Draft>),
    codes: &["not-found", "draft-not-placed"],
    owned: true,
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let id = call.key("draftId").await?;
    let patch: DraftPatch = call.body().await?;

    with_account(&mut call, |mail| async move {
      let mut draft = mail.mailbox.draft(&id).await?;

      if let Some(to) = &patch.to {
        draft.to = recipients(to)?;
      }

      if let Some(cc) = &patch.cc {
        draft.cc = recipients(cc)?;
      }

      if let Some(bcc) = &patch.bcc {
        draft.bcc = recipients(bcc)?;
      }

      if let Some(subject) = &patch.subject {
        draft.subject = one_line(subject);
      }

      if let Some(body) = patch.body {
        draft.html = body;
      }

      if let Some(kept) = &patch.attachments {
        draft.attachments.retain(|attachment| match attachment {
          AttachmentRef::Part { part, .. } => kept.iter().any(|one| &one.part_id == part),
          AttachmentRef::Upload { .. } => true,
        });
      }

      if let Some(receipt) = patch.receipt_requested {
        draft.receipt = receipt;
      }

      let saved = mail.mailbox.save_draft(&draft).await?;

      Ok(answer(StatusCode::OK, &member(mail.name(), &saved)))
    })
    .await
  }
}

crate::route!(ChangeDraft);

/// `DELETE /accounts/{accountName}/drafts/{draftId}`.
pub struct DeleteDraft;

impl Route for DeleteDraft {
  const METHOD: Method = Method::DELETE;
  const PATH: &'static str = "/accounts/{accountName}/drafts/{draftId}";
  const OPERATION: Operation = Operation {
    id: "deleteAccountsDraftsByAccountNameDraftId",
    summary: "Throw a draft away.",
    tag: "accounts",
    keys: &[ACCOUNT, DRAFT],
    query: None,
    body: None,
    status: 204,
    answer: Answer::Nothing,
    codes: &["not-found"],
    owned: true,
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let id = call.key("draftId").await?;

    with_account(&mut call, |mail| async move {
      mail.mailbox.discard_draft(&id).await?;

      Ok(nothing())
    })
    .await
  }
}

crate::route!(DeleteDraft);

/// `POST /accounts/{accountName}/drafts/{draftId}/attachments`: a file attached, the draft saved again with
/// it.
pub struct CreateAttachment;

impl Route for CreateAttachment {
  const METHOD: Method = Method::POST;
  const PATH: &'static str = "/accounts/{accountName}/drafts/{draftId}/attachments";
  const OPERATION: Operation = Operation {
    id: "createAccountsDraftsAttachmentsByAccountNameDraftId",
    summary: "Attach a file to a draft.",
    tag: "accounts",
    keys: &[ACCOUNT, DRAFT],
    query: None,
    body: Some(schema::<NewAttachment>),
    status: 201,
    answer: Answer::Json(schema::<Draft>),
    codes: &["not-found", "too-large", "draft-not-placed"],
    owned: true,
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let id = call.key("draftId").await?;
    let file: NewAttachment = call.body().await?;

    with_account(&mut call, |mail| async move {
      let bytes = STANDARD
        .decode(file.content.trim())
        .map_err(|why| Asked::bad_request(format!("the content is not base64: {why}")))?;
      let mut draft = mail.mailbox.draft(&id).await?;
      let mut sizes: Vec<u64> = draft
        .attachments
        .iter()
        .map(|attachment| match attachment {
          AttachmentRef::Upload { size, .. } | AttachmentRef::Part { size, .. } => *size,
        })
        .collect();

      sizes.push(bytes.len() as u64);

      let limit = mail.mailbox.sending_limit().await;

      if !attachments_fit(&sizes, limit) {
        return Err(
          Asked::too_large("the file would take the draft past the size the server takes").into(),
        );
      }

      let staging = &mail.standing.settings.staging.dir;
      let name = if file.name.trim().is_empty() { "attachment" } else { file.name.trim() };
      let staged = stage_file(staging, name, &file.media_type, &bytes).map_err(MailFault::server)?;

      draft.attachments.push(AttachmentRef::Upload {
        id: staged.id,
        filename: staged.filename,
        kind: staged.kind,
        size: staged.size,
      });

      let saved = mail.mailbox.save_draft(&draft).await?;

      Ok(answer(StatusCode::CREATED, &member(mail.name(), &saved)))
    })
    .await
  }
}

crate::route!(CreateAttachment);

/// `POST /accounts/{accountName}/sends`: a draft sent once the undo span has run, at once, or at a time
/// within the span.
pub struct CreateSend;

impl Route for CreateSend {
  const METHOD: Method = Method::POST;
  const PATH: &'static str = "/accounts/{accountName}/sends";
  const OPERATION: Operation = Operation {
    id: "createAccountsSendsByAccountName",
    summary: "Send a draft, at once or after the undo span.",
    tag: "accounts",
    keys: &[ACCOUNT],
    query: None,
    body: Some(schema::<NewSend>),
    status: 201,
    answer: Answer::Json(schema::<Send>),
    codes: &["not-found", "no-recipients", "send-too-late"],
    owned: true,
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let new: NewSend = call.body().await?;

    with_account(&mut call, |mail| async move {
      let standing = Arc::clone(&mail.standing);
      let settings = read_settings(standing.store.pool(), &mail.key()).await?;
      let now = chrono::Utc::now().timestamp_millis();
      let latest = now + settings.undo_send_ms;
      let send_at = match new.send_at {
        None => latest,
        Some(at) if at <= now => now,
        Some(at) if at > latest => {
          return Err(Asked::rule("send-too-late", "a send waits no longer than the undo span").into());
        }
        Some(at) => at,
      };
      let send =
        queue_send(standing, Arc::clone(&mail.mailbox), &mail.account, &new.draft_id, send_at).await?;
      let owner = &mail.account.owner;

      mail.standing.events.publish(SEND_CHANGED, owner, Some(mail.name()), &send).await;

      Ok(answer(StatusCode::CREATED, &send))
    })
    .await
  }
}

crate::route!(CreateSend);

/// `GET /accounts/{accountName}/sends/{sendId}`.
pub struct ReadSend;

impl Route for ReadSend {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/accounts/{accountName}/sends/{sendId}";
  const OPERATION: Operation = Operation {
    id: "readAccountsSendsByAccountNameSendId",
    summary: "A send as it stands.",
    tag: "accounts",
    keys: &[ACCOUNT, SEND],
    query: None,
    body: None,
    status: 200,
    answer: Answer::Json(schema::<Send>),
    codes: &["not-found"],
    owned: true,
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let id = call.key("sendId").await?;

    with_account(&mut call, |mail| async move {
      let Some(send) = mail.standing.sends.send(mail.account.id, &id) else {
        return Err(Asked::not_found("no send has that id").into());
      };

      Ok(answer(StatusCode::OK, &send))
    })
    .await
  }
}

crate::route!(ReadSend);

/// `DELETE /accounts/{accountName}/sends/{sendId}`: undo send, while the send still waits.
pub struct CancelSend;

impl Route for CancelSend {
  const METHOD: Method = Method::DELETE;
  const PATH: &'static str = "/accounts/{accountName}/sends/{sendId}";
  const OPERATION: Operation = Operation {
    id: "deleteAccountsSendsByAccountNameSendId",
    summary: "Take a send back while it waits; its draft stays.",
    tag: "accounts",
    keys: &[ACCOUNT, SEND],
    query: None,
    body: None,
    status: 204,
    answer: Answer::Nothing,
    codes: &["not-found", "conflict"],
    owned: true,
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let id = call.key("sendId").await?;

    with_account(&mut call, |mail| async move {
      let Some(cancelled) = mail.standing.sends.cancel(mail.account.id, &id) else {
        return Err(Asked::not_found("no send has that id").into());
      };
      let send = cancelled?;
      let owner = &mail.account.owner;

      mail.standing.events.publish(SEND_CHANGED, owner, Some(mail.name()), &send).await;
      forget_later(Arc::clone(&mail.standing), id);

      Ok(nothing())
    })
    .await
  }
}

crate::route!(CancelSend);

/// `POST /accounts/{accountName}/receipts`: the read receipt a message asks for, sent.
pub struct CreateReceipt;

impl Route for CreateReceipt {
  const METHOD: Method = Method::POST;
  const PATH: &'static str = "/accounts/{accountName}/receipts";
  const OPERATION: Operation = Operation {
    id: "createAccountsReceiptsByAccountName",
    summary: "Send a read receipt for a message.",
    tag: "accounts",
    keys: &[ACCOUNT],
    query: None,
    body: Some(schema::<NewReceipt>),
    status: 201,
    answer: Answer::Json(schema::<Receipt>),
    codes: &["not-found", "no-receipt-asked"],
    owned: true,
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let new: NewReceipt = call.body().await?;

    with_account(&mut call, |mail| async move {
      let at = message_at(&new.message_id)?;

      mail.mailbox.send_receipt(&at, &mail.standing.wording).await?;

      Ok(answer(
        StatusCode::CREATED,
        &Receipt {
          message_id: new.message_id,
          sent: true,
        },
      ))
    })
    .await
  }
}

crate::route!(CreateReceipt);
