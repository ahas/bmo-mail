//! The watchers' rows in the store (`docs/mail.md`, 「Connections and the watcher」): each folder's high-water
//! mark, the UID validity it was taken under and the highest UID handled, and the arrivals still owed their
//! `message.created`. Every adapter's watcher keeps them the same way.

use sqlx::SqlitePool;

use crate::routes::requests::MailFault;

/// The high-water mark of a folder: the UID validity it was taken under and the highest UID handled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MailMark {
  pub validity: String,
  pub last_uid: u32,
}

pub async fn read_mark(
  pool: &SqlitePool,
  account: &str,
  folder: &str,
) -> Result<Option<MailMark>, MailFault> {
  let row: Option<(String, i64)> =
    sqlx::query_as("SELECT uid_validity, last_uid FROM marks WHERE account = ? AND folder = ?")
      .bind(account)
      .bind(folder)
      .fetch_optional(pool)
      .await?;

  Ok(row.map(|(validity, last_uid)| MailMark {
    validity,
    last_uid: u32::try_from(last_uid).unwrap_or(0),
  }))
}

pub async fn write_mark(
  pool: &SqlitePool,
  account: &str,
  folder: &str,
  mark: &MailMark,
) -> Result<(), MailFault> {
  sqlx::query(
    "INSERT INTO marks (account, folder, uid_validity, last_uid) VALUES (?, ?, ?, ?)
     ON CONFLICT (account, folder) DO UPDATE SET uid_validity = excluded.uid_validity, last_uid = excluded.last_uid",
  )
  .bind(account)
  .bind(folder)
  .bind(&mark.validity)
  .bind(i64::from(mark.last_uid))
  .execute(pool)
  .await?;

  Ok(())
}

/// Keeps a message as an arrival owed its `message.created`.
pub async fn owe(
  pool: &SqlitePool,
  account: &str,
  folder: &str,
  validity: &str,
  uid: u32,
) -> Result<(), MailFault> {
  sqlx::query("INSERT OR IGNORE INTO arrivals (account, folder, uid_validity, uid) VALUES (?, ?, ?, ?)")
    .bind(account)
    .bind(folder)
    .bind(validity)
    .bind(i64::from(uid))
    .execute(pool)
    .await?;

  Ok(())
}

/// The arrivals still owed their event under the folder's UID validity, oldest first; those of another
/// validity, a folder the server rebuilt, are let go.
pub async fn owed(
  pool: &SqlitePool,
  account: &str,
  folder: &str,
  validity: &str,
) -> Result<Vec<u32>, MailFault> {
  sqlx::query("DELETE FROM arrivals WHERE account = ? AND folder = ? AND uid_validity <> ?")
    .bind(account)
    .bind(folder)
    .bind(validity)
    .execute(pool)
    .await?;

  let uids: Vec<i64> = sqlx::query_scalar(
    "SELECT uid FROM arrivals WHERE account = ? AND folder = ? AND uid_validity = ? ORDER BY uid",
  )
  .bind(account)
  .bind(folder)
  .bind(validity)
  .fetch_all(pool)
  .await?;

  Ok(uids.into_iter().filter_map(|uid| u32::try_from(uid).ok()).collect())
}

/// Lets an arrival go once its event is said.
pub async fn settled(
  pool: &SqlitePool,
  account: &str,
  folder: &str,
  validity: &str,
  uid: u32,
) -> Result<(), MailFault> {
  sqlx::query("DELETE FROM arrivals WHERE account = ? AND folder = ? AND uid_validity = ? AND uid = ?")
    .bind(account)
    .bind(folder)
    .bind(validity)
    .bind(i64::from(uid))
    .execute(pool)
    .await?;

  Ok(())
}
