//! An account's settings (`docs/mail.md`, 「Where each piece of state lives」): the signature a new message
//! opens with, whether a read receipt is asked by default, whether a message's outside pictures are shown as
//! it opens, and how long a send waits before it goes.

use sqlx::SqlitePool;

use crate::mail::MailSettings;
use crate::routes::requests::MailFault;

/// How long a send waits where the account never said.
pub const UNDO_SEND_MS: i64 = 10_000;
/// The longest a send may be made to wait.
pub const MOST_UNDO_SEND_MS: i64 = 30_000;

/// The account's settings; an account that has never saved any has none of them on and the usual undo span.
pub async fn read_settings(pool: &SqlitePool, account: &str) -> Result<MailSettings, MailFault> {
  let row: Option<(String, i64, i64, i64)> = sqlx::query_as(
    "SELECT signature, receipt_default, show_images, undo_send_ms FROM settings WHERE account = ?",
  )
  .bind(account)
  .fetch_optional(pool)
  .await?;

  Ok(match row {
    Some((signature, receipt_default, show_images, undo_send_ms)) => MailSettings {
      signature,
      receipt_default: receipt_default == 1,
      show_images: show_images == 1,
      undo_send_ms,
    },
    None => MailSettings {
      signature: String::new(),
      receipt_default: false,
      show_images: false,
      undo_send_ms: UNDO_SEND_MS,
    },
  })
}

/// Keeps the account's settings.
pub async fn write_settings(
  pool: &SqlitePool,
  account: &str,
  settings: &MailSettings,
) -> Result<(), MailFault> {
  sqlx::query(
    "INSERT INTO settings (account, signature, receipt_default, show_images, undo_send_ms) VALUES (?, ?, ?, ?, ?)
     ON CONFLICT (account) DO UPDATE SET signature = excluded.signature, receipt_default = excluded.receipt_default,
       show_images = excluded.show_images, undo_send_ms = excluded.undo_send_ms",
  )
  .bind(account)
  .bind(&settings.signature)
  .bind(i64::from(settings.receipt_default))
  .bind(i64::from(settings.show_images))
  .bind(settings.undo_send_ms)
  .execute(pool)
  .await?;

  Ok(())
}
