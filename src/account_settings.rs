//! An account's settings (`docs/mail.md`, 「Where each piece of state lives」): the signature a new message
//! opens with, whether a read receipt is asked by default, and whether a message's outside pictures are
//! shown as it opens.

use sqlx::SqlitePool;

use crate::routes::requests::MailFault;
use crate::wire::MailSettings;

/// The account's settings; an account that has never saved any has none of them on.
pub async fn read_settings(pool: &SqlitePool, account: &str) -> Result<MailSettings, MailFault> {
  let row: Option<(String, i64, i64)> =
    sqlx::query_as("SELECT signature, receipt_default, show_images FROM settings WHERE account = ?")
      .bind(account)
      .fetch_optional(pool)
      .await?;

  Ok(match row {
    Some((signature, receipt_default, show_images)) => MailSettings {
      signature,
      receipt_default: receipt_default == 1,
      show_images: show_images == 1,
    },
    None => MailSettings {
      signature: String::new(),
      receipt_default: false,
      show_images: false,
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
    "INSERT INTO settings (account, signature, receipt_default, show_images) VALUES (?, ?, ?, ?)
     ON CONFLICT (account) DO UPDATE SET signature = excluded.signature, receipt_default = excluded.receipt_default,
       show_images = excluded.show_images",
  )
  .bind(account)
  .bind(&settings.signature)
  .bind(i64::from(settings.receipt_default))
  .bind(i64::from(settings.show_images))
  .execute(pool)
  .await?;

  Ok(())
}
