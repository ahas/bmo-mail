//! The addresses an account has written to or heard from (`docs/mail.md`, 「Where each piece of state
//! lives」), which the address fields complete from.

use sqlx::SqlitePool;

use crate::mail::MailAddress;
use crate::routes::requests::MailFault;
use crate::wire::Address;

/// Records each address as used now under the account's key: its name kept where one is given, its use
/// counted. The account's own address, `own`, is never recorded.
pub async fn record_addresses(
  pool: &SqlitePool,
  account: &str,
  own: &str,
  addresses: &[MailAddress],
  at: i64,
) -> Result<(), MailFault> {
  let mut transaction = pool.begin().await?;

  for one in addresses {
    let address = one.address.trim().to_lowercase();

    if address.is_empty() || address == own.to_lowercase() {
      continue;
    }

    sqlx::query(
      "INSERT INTO addresses (account, address, name, last_used, uses) VALUES (?, ?, ?, ?, 1)
       ON CONFLICT (account, address) DO UPDATE SET
         name = CASE WHEN excluded.name <> '' THEN excluded.name ELSE addresses.name END,
         last_used = excluded.last_used,
         uses = addresses.uses + 1",
    )
    .bind(account)
    .bind(&address)
    .bind(one.name.trim())
    .bind(at)
    .execute(&mut *transaction)
    .await?;
  }

  transaction.commit().await?;

  Ok(())
}

/// The known addresses whose address or name holds the typed text, the most used and most recent first,
/// after the place `after` names, at most `limit` and one more to tell whether more stand past them.
pub async fn suggest_addresses(
  pool: &SqlitePool,
  account: &str,
  typed: &str,
  after: i64,
  limit: u32,
) -> Result<Vec<Address>, MailFault> {
  let escaped: String = typed
    .trim()
    .to_lowercase()
    .chars()
    .flat_map(|c| if matches!(c, '%' | '_' | '\\') { vec!['\\', c] } else { vec![c] })
    .collect();
  let like = format!("%{escaped}%");
  let rows: Vec<(String, String, i64, i64)> = sqlx::query_as(
    "SELECT name, address, uses, last_used FROM addresses
     WHERE account = ? AND (address LIKE ? ESCAPE '\\' OR lower(name) LIKE ? ESCAPE '\\')
     ORDER BY uses DESC, last_used DESC, address LIMIT ? OFFSET ?",
  )
  .bind(account)
  .bind(&like)
  .bind(&like)
  .bind(i64::from(limit) + 1)
  .bind(after.max(0))
  .fetch_all(pool)
  .await?;

  Ok(
    rows
      .into_iter()
      .enumerate()
      .map(|(index, (name, address, uses, last_used))| Address {
        address,
        name: Some(name).filter(|name| !name.is_empty()),
        used_count: uses,
        last_used_at: last_used,
        position: after.max(0) + index as i64 + 1,
      })
      .collect(),
  )
}
