//! A POP3 account's drafts (`docs/mail.md`, 「A POP3 account」): its server keeps none, so each stands in the
//! store as the whole message it was last saved as, under the `Message-ID` it keeps across every save, in the
//! drafts folder of the service's own. A save replaces the kept message in one write.

use sqlx::SqlitePool;

use crate::imap::messages::{heads, message_of};
use crate::mail::MailMessage;
use crate::pop3::DRAFTS;
use crate::pop3::mime::read_whole;
use crate::routes::requests::MailFault;

/// The flags a draft carries until a request changes them.
pub const DRAFT_FLAGS: [&str; 2] = ["\\Draft", "\\Seen"];

/// A draft as the store keeps it.
#[derive(Clone, Debug)]
pub struct DraftRow {
  pub id: u32,
  /// Its `Message-ID`, without angle brackets.
  pub message_id: String,
  pub message: Vec<u8>,
  pub flags: Vec<String>,
  pub changed_at: i64,
}

type DraftColumns = (i64, String, Vec<u8>, String, i64);

fn draft_of((id, message_id, message, flags, changed_at): DraftColumns) -> Option<DraftRow> {
  Some(DraftRow {
    id: u32::try_from(id).ok()?,
    message_id,
    message,
    flags: serde_json::from_str(&flags).unwrap_or_default(),
    changed_at,
  })
}

const DRAFT_COLUMNS: &str = "id, message_id, message, flags, changed_at";

/// Every draft of the account, oldest first.
pub async fn draft_rows(pool: &SqlitePool, account: &str) -> Result<Vec<DraftRow>, MailFault> {
  let found: Vec<DraftColumns> =
    sqlx::query_as(&format!("SELECT {DRAFT_COLUMNS} FROM drafts WHERE account = ? ORDER BY id"))
      .bind(account)
      .fetch_all(pool)
      .await?;

  Ok(found.into_iter().filter_map(draft_of).collect())
}

/// One draft, by its number.
pub async fn draft_row(pool: &SqlitePool, account: &str, id: u32) -> Result<Option<DraftRow>, MailFault> {
  let found: Option<DraftColumns> =
    sqlx::query_as(&format!("SELECT {DRAFT_COLUMNS} FROM drafts WHERE account = ? AND id = ?"))
      .bind(account)
      .bind(i64::from(id))
      .fetch_optional(pool)
      .await?;

  Ok(found.and_then(draft_of))
}

/// One draft, by its `Message-ID` without angle brackets.
pub async fn draft_row_named(
  pool: &SqlitePool,
  account: &str,
  message_id: &str,
) -> Result<Option<DraftRow>, MailFault> {
  let found: Option<DraftColumns> =
    sqlx::query_as(&format!("SELECT {DRAFT_COLUMNS} FROM drafts WHERE account = ? AND message_id = ?"))
      .bind(account)
      .bind(message_id)
      .fetch_optional(pool)
      .await?;

  Ok(found.and_then(draft_of))
}

/// Keeps a draft's message under its `Message-ID`, replacing the one kept before in the same write; answers its
/// number, which stays across every save.
pub async fn keep_draft(
  pool: &SqlitePool,
  account: &str,
  message_id: &str,
  message: &[u8],
  changed_at: i64,
) -> Result<u32, MailFault> {
  let flags = serde_json::to_string(&DRAFT_FLAGS).map_err(MailFault::server)?;
  let id: i64 = sqlx::query_scalar(
    "INSERT INTO drafts (account, message_id, message, flags, changed_at) VALUES (?, ?, ?, ?, ?)
     ON CONFLICT (account, message_id) DO UPDATE SET message = excluded.message, changed_at = excluded.changed_at
     RETURNING id",
  )
  .bind(account)
  .bind(message_id)
  .bind(message)
  .bind(flags)
  .bind(changed_at)
  .fetch_one(pool)
  .await?;

  u32::try_from(id).map_err(MailFault::server)
}

/// Forgets a draft.
pub async fn forget_draft(pool: &SqlitePool, account: &str, id: u32) -> Result<(), MailFault> {
  sqlx::query("DELETE FROM drafts WHERE account = ? AND id = ?")
    .bind(account)
    .bind(i64::from(id))
    .execute(pool)
    .await?;

  Ok(())
}

/// Keeps the flags a draft carries.
pub async fn keep_draft_flags(
  pool: &SqlitePool,
  account: &str,
  id: u32,
  flags: &[String],
) -> Result<(), MailFault> {
  sqlx::query("UPDATE drafts SET flags = ? WHERE account = ? AND id = ?")
    .bind(serde_json::to_string(flags).map_err(MailFault::server)?)
    .bind(account)
    .bind(i64::from(id))
    .execute(pool)
    .await?;

  Ok(())
}

/// A draft as a message of the drafts folder, with its bodies where they are asked for.
pub fn draft_message(row: &DraftRow, with_bodies: bool) -> MailMessage {
  let whole = read_whole(&row.message);
  let mut message =
    message_of(DRAFTS, row.id, &row.flags, heads(Some(&row.message)), whole.parts, row.changed_at);

  message.listed.snippet = whole.snippet;

  if with_bodies {
    message.text = whole.text;
    message.html = whole.html;
  }

  message
}
