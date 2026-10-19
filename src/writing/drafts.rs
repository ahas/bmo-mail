//! Drafts (`docs/mail.md`, 「Writing」): a draft is a message in the drafts folder. A save writes the new
//! copy first and deletes the one it replaces afterwards, so a save that fails midway never loses the older
//! copy.

use std::path::Path;

use mail_parser::{Address, HeaderName, MessageParser, MimeHeaders};

use crate::imap::conversation::id_key;
use crate::imap::folders::ensure_role_folder;
use crate::imap::messages::{open_part, read_message};
use crate::imap::session::MailSession;
use crate::imap::{append_landing, expunge, flag_name, store_flags, uid_fetch};
use crate::routes::requests::{Asked, MailFault};
use crate::wire::{AttachmentRef, Draft, DraftKind, FolderRole, MailAddress, MailRef};
use crate::writing::compose::{
  DRAFT_KIND_HEADER, DRAFT_ORIGINAL_HEADER, Purpose, ResolvedAttachment, build_message, data_images,
  escape_html,
};
use crate::writing::staging::read_staged;

/// Every attachment's bytes: a staged upload from the staging folder, a part from the server.
pub async fn resolve_attachments(
  session: &MailSession,
  staging: &Path,
  refs: &[AttachmentRef],
) -> Result<Vec<ResolvedAttachment>, MailFault> {
  let mut resolved = Vec::new();

  for at in refs {
    match at {
      AttachmentRef::Upload { id, filename, .. } => {
        let Some(staged) = read_staged(staging, id) else {
          return Err(
            Asked::new("upload_gone", "a staged file is no longer there")
              .with("name", filename)
              .into(),
          );
        };

        resolved.push(ResolvedAttachment {
          filename: staged.filename,
          kind: staged.kind,
          content: staged.content,
        });
      }
      AttachmentRef::Part {
        folder,
        uid,
        part,
        filename,
        kind,
        ..
      } => {
        let opened = open_part(
          session,
          &MailRef {
            folder: folder.clone(),
            uid: *uid,
          },
          part,
        )
        .await?;

        resolved.push(ResolvedAttachment {
          filename: filename.clone(),
          kind: kind.clone(),
          content: opened.bytes,
        });
      }
    }
  }

  Ok(resolved)
}

/// Saves the draft and answers where it now is. The draft it replaces, if any, is deleted only after the
/// new copy stands.
pub async fn save_draft(
  session: &MailSession,
  staging: &Path,
  draft: &Draft,
) -> Result<MailRef, MailFault> {
  let attachments = resolve_attachments(session, staging, &draft.attachments).await?;
  let raw = build_message(draft, &attachments, Purpose::Draft, None, chrono::Utc::now().timestamp())
    .map_err(MailFault::server)?;
  let folder = ensure_role_folder(session, FolderRole::Drafts).await?;
  let mut connection = session.connection().await?;
  let appended = append_landing(connection.session(), &folder, "\\Draft \\Seen", None, &raw).await;
  let landed = connection.settle(appended)?;
  let Some(uid) = landed.and_then(|landed| landed.uids.first().copied()) else {
    return Err(MailFault::asked("no_draft_uid", "the server did not say where it put the saved draft"));
  };

  if let Some(saved) = &draft.saved {
    discard_draft(session, saved).await?;
  }

  Ok(MailRef { folder, uid })
}

/// Deletes a draft for good. A draft is the only message the service deletes outside trash and spam, and
/// only one flagged as a draft.
pub async fn discard_draft(session: &MailSession, at: &MailRef) -> Result<(), MailFault> {
  let mut connection = session.folder(&at.folder).await?;
  let done = async {
    let found = uid_fetch(connection.session(), &[at.uid], "(UID FLAGS)").await?;
    let is_draft = found
      .first()
      .is_some_and(|fetch| fetch.flags().any(|flag| flag_name(&flag) == "\\Draft"));

    if is_draft {
      let uidplus =
        connection.session().capabilities().await.map_err(MailFault::server)?.has_str("UIDPLUS");

      store_flags(connection.session(), &[at.uid], "+", &["\\Deleted".to_owned()]).await?;
      expunge(connection.session(), &[at.uid], uidplus).await?;
    }

    Ok(())
  }
  .await;

  connection.settle(done)
}

fn addresses(address: Option<&Address<'_>>) -> Vec<MailAddress> {
  address
    .map(|address| {
      address
        .iter()
        .filter_map(|one| {
          let mail = one.address().filter(|mail| !mail.is_empty())?;

          Some(MailAddress {
            name: one.name().unwrap_or("").to_owned(),
            address: mail.to_lowercase(),
          })
        })
        .collect()
    })
    .unwrap_or_default()
}

/// A saved draft, back in the shape the composer writes in: its headers and bodies parsed from the message
/// itself, its attachments as parts of it on the server.
pub async fn open_draft(session: &MailSession, at: &MailRef) -> Result<Draft, MailFault> {
  let mut connection = session.folder(&at.folder).await?;
  let done = uid_fetch(connection.session(), &[at.uid], "(UID BODY.PEEK[])").await;
  let source = connection.settle(done)?.first().and_then(|fetch| fetch.body().map(<[u8]>::to_vec));
  let message = read_message(session, at).await?;
  let (Some(source), Some(message)) = (source, message) else {
    return Err(MailFault::asked("draft_not_found", "the draft is not on the server"));
  };
  let parser = MessageParser::new()
    .with_mime_headers()
    .with_address_headers()
    .with_date_headers()
    .with_message_ids()
    .header_text(HeaderName::Subject)
    .header_text(HeaderName::Other(DRAFT_KIND_HEADER.into()))
    .header_text(HeaderName::Other(DRAFT_ORIGINAL_HEADER.into()))
    .header_raw(HeaderName::DispositionNotificationTo);
  let Some(parsed) = parser.parse(&source) else {
    return Err(MailFault::asked("draft_not_found", "the draft is not on the server"));
  };
  let text_header = |name: &str| {
    parsed
      .header(HeaderName::Other(name.to_owned().into()))
      .and_then(|value| value.as_text())
      .map(str::to_owned)
  };
  let kind = match text_header(DRAFT_KIND_HEADER).as_deref() {
    Some("reply") => DraftKind::Reply,
    Some("forward") => DraftKind::Forward,
    _ => DraftKind::New,
  };
  let html = parsed
    .body_html(0)
    .map(|html| html.into_owned())
    .unwrap_or_else(|| escape_html(parsed.body_text(0).as_deref().unwrap_or("")).replace('\n', "<br>"));
  let html = data_images(&html, |cid| {
    parsed.parts.iter().find_map(|part| {
      let named = part.content_id()?.trim_start_matches('<').trim_end_matches('>');
      let kind = part.content_type().filter(|kind| kind.ctype().eq_ignore_ascii_case("image"))?;
      let kind = format!("image/{}", kind.subtype()?).to_lowercase();
      (named == cid && !part.contents().is_empty()).then(|| (kind, part.contents().to_vec()))
    })
  });
  let ids = |value: &mail_parser::HeaderValue<'_>| -> Vec<String> {
    value
      .as_text_list()
      .map(|ids| ids.iter().map(|id| id_key(id)).collect())
      .unwrap_or_default()
  };

  Ok(Draft {
    from: parsed
      .from()
      .and_then(|from| from.first())
      .and_then(|one| one.address())
      .map(str::to_lowercase)
      .unwrap_or_else(|| session.account.address.clone()),
    to: addresses(parsed.to()),
    cc: addresses(parsed.cc()),
    bcc: addresses(parsed.bcc()),
    subject: parsed.subject().unwrap_or("").to_owned(),
    html,
    attachments: message
      .attachments
      .iter()
      .map(|part| AttachmentRef::Part {
        folder: at.folder.clone(),
        uid: at.uid,
        part: part.part.clone(),
        filename: part.filename.clone().unwrap_or_else(|| "attachment".to_owned()),
        kind: part.kind.clone(),
        size: part.decoded_size(),
      })
      .collect(),
    receipt: parsed.header(HeaderName::DispositionNotificationTo).is_some(),
    kind,
    original: text_header(DRAFT_ORIGINAL_HEADER).map(|id| id_key(&id)),
    in_reply_to: ids(parsed.in_reply_to()).into_iter().next(),
    references: ids(parsed.references()),
    saved: Some(at.clone()),
  })
}
