//! Drafts (`docs/mail.md`, 「Writing」): a draft is a message in the drafts folder, named by the `Message-ID`
//! it keeps across every save. A save writes the new copy first and deletes the one it replaces afterwards,
//! so a save that fails midway never loses the older copy.

use std::path::Path;

use mail_parser::{Address, HeaderName, MessageParser, MimeHeaders};

use crate::imap::conversation::id_key;
use crate::imap::folders::{ensure_role_folder, folders_of, role_folders};
use crate::imap::messages::{open_part, read_message};
use crate::imap::search::Search;
use crate::imap::session::MailSession;
use crate::imap::{append_landing, expunge, flag_name, store_flags, uid_fetch, uid_search};
use crate::mail::{AttachmentRef, Draft, DraftKind, FolderRole, MailAddress, MailRef};
use crate::members::{bare_id, mailbox};
use crate::routes::requests::{Asked, MailFault};
use crate::wire;
use crate::writing::compose::{
  DRAFT_KIND_HEADER, DRAFT_ORIGINAL_HEADER, Purpose, ResolvedAttachment, build_message, data_images,
  escape_html, new_message_id,
};
use crate::writing::staging::read_staged;

/// Every attachment's bytes: a staged file from the staging folder, a part from the server.
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
          return Err(MailFault::server(format!("the staged file {filename} is no longer there")));
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
        let at = MailRef {
          folder: folder.clone(),
          uid: *uid,
        };
        let Some(opened) = open_part(session, &at, part).await? else {
          return Err(MailFault::server(format!("the attachment {filename} is no longer on the server")));
        };

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

/// Where the draft a `Message-ID` names stands in the drafts folders, the newest copy where there are two.
pub async fn find_draft(session: &MailSession, draft_id: &str) -> Result<Option<MailRef>, MailFault> {
  let wanted = bare_id(draft_id);

  if wanted.is_empty() {
    return Ok(None);
  }

  let search = Search::Header("message-id".to_owned(), wanted);

  for folder in role_folders(&folders_of(session).await?, FolderRole::Drafts) {
    let mut connection = session.folder(&folder.path).await?;
    let done = uid_search(connection.session(), &search.command()).await;
    let found = connection.settle(done)?;

    if let Some(uid) = found.into_iter().max() {
      return Ok(Some(MailRef {
        folder: folder.path,
        uid,
      }));
    }
  }

  Ok(None)
}

/// Saves the draft under the `Message-ID` it keeps, and answers where it now is. The draft it replaces, if
/// any, is deleted only after the new copy stands.
pub async fn save_draft(
  session: &MailSession,
  staging: &Path,
  draft: &Draft,
) -> Result<MailRef, MailFault> {
  let message_id = draft.message_id.clone().unwrap_or_else(|| format!("<{}>", new_message_id(&draft.from)));
  let attachments = resolve_attachments(session, staging, &draft.attachments).await?;
  let raw =
    build_message(draft, &attachments, Purpose::Draft, Some(&message_id), chrono::Utc::now().timestamp())
      .map_err(MailFault::server)?;
  let folder = ensure_role_folder(session, FolderRole::Drafts).await?;
  let mut connection = session.connection().await?;
  let appended = append_landing(connection.session(), &folder, "\\Draft \\Seen", None, &raw).await;
  let landed = connection.settle(appended)?;
  let uid = match landed.and_then(|landed| landed.uids.first().copied()) {
    Some(uid) => uid,
    None => {
      let mut connection = session.folder(&folder).await?;
      let search = Search::Header("message-id".to_owned(), bare_id(&message_id));
      let done = uid_search(connection.session(), &search.command()).await;
      let found = connection.settle(done)?;
      let replaced = draft.saved.as_ref().filter(|saved| saved.folder == folder).map(|saved| saved.uid);

      found.into_iter().filter(|uid| Some(*uid) != replaced).max().ok_or_else(|| {
        Asked::rule("draft-not-placed", "the server did not say where it put the saved draft")
      })?
    }
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

/// A saved draft, back in the shape it is written in: its headers and bodies parsed from the message itself,
/// its attachments as parts of it on the server. None where it is not there.
pub async fn open_draft(session: &MailSession, at: &MailRef) -> Result<Option<Draft>, MailFault> {
  let mut connection = session.folder(&at.folder).await?;
  let done = uid_fetch(connection.session(), &[at.uid], "(UID BODY.PEEK[])").await;
  let source = connection.settle(done)?.first().and_then(|fetch| fetch.body().map(<[u8]>::to_vec));
  let message = read_message(session, at).await?;
  let (Some(source), Some(message)) = (source, message) else {
    return Ok(None);
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
    return Ok(None);
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

  Ok(Some(Draft {
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
    message_id: parsed.message_id().map(|id| format!("<{}>", bare_id(id))),
    saved: Some(at.clone()),
    changed_at: message.listed.date,
  }))
}

/// The draft a `Message-ID` names, opened, or a 404.
pub async fn draft_named(session: &MailSession, draft_id: &str) -> Result<Draft, MailFault> {
  let Some(at) = find_draft(session, draft_id).await? else {
    return Err(Asked::not_found("no draft has that id").into());
  };

  open_draft(session, &at)
    .await?
    .ok_or_else(|| Asked::not_found("no draft has that id").into())
}

/// Saves a draft and opens it again where it now stands, so its attachments name the parts of the copy that
/// stands.
pub async fn save_and_open(
  session: &MailSession,
  staging: &Path,
  draft: &Draft,
) -> Result<Draft, MailFault> {
  let at = save_draft(session, staging, draft).await?;

  open_draft(session, &at)
    .await?
    .ok_or_else(|| MailFault::server("the saved draft could not be read back"))
}

/// A draft as a member, under the account it is written from.
pub fn member(account: &str, draft: &Draft) -> wire::Draft {
  let people = |list: &[MailAddress]| list.iter().map(mailbox).collect();

  wire::Draft {
    id: draft.message_id.as_deref().map(bare_id).unwrap_or_default(),
    account_name: account.to_owned(),
    kind: draft.kind,
    from: wire::Mailbox {
      name: None,
      address: draft.from.clone(),
    },
    to: people(&draft.to),
    cc: people(&draft.cc),
    bcc: people(&draft.bcc),
    subject: draft.subject.clone(),
    body: draft.html.clone(),
    attachments: draft
      .attachments
      .iter()
      .filter_map(|attachment| match attachment {
        AttachmentRef::Part {
          part,
          filename,
          kind,
          size,
          ..
        } => Some(wire::DraftAttachment {
          part_id: part.clone(),
          name: filename.clone(),
          media_type: kind.clone(),
          size: *size,
        }),
        AttachmentRef::Upload { .. } => None,
      })
      .collect(),
    receipt_requested: draft.receipt,
    in_reply_to: draft.in_reply_to.as_deref().map(bare_id),
    references: draft.references.iter().map(|id| bare_id(id)).collect(),
    original_id: draft.original.as_deref().map(bare_id),
    changed_at: draft.changed_at,
  }
}
