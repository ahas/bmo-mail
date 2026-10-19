//! The addresses an account has written to or heard from (`docs/mail.md`, 「Where each piece of state
//! lives」), which the address fields complete from.

use sqlx::SqlitePool;

use crate::routes::requests::MailFault;
use crate::wire::MailAddress;

/// How many completions an address field is offered.
const SUGGESTED: i64 = 8;

/// Records each address as used now: its name kept where one is given, its use counted. The account's own
/// address is never recorded.
pub async fn record_addresses(
  pool: &SqlitePool,
  account: &str,
  addresses: &[MailAddress],
  at: i64,
) -> Result<(), MailFault> {
  let mut transaction = pool.begin().await?;

  for one in addresses {
    let address = one.address.trim().to_lowercase();

    if address.is_empty() || address == account {
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

/// The known addresses whose address or name holds the typed text, the most used and most recent first.
pub async fn suggest_addresses(
  pool: &SqlitePool,
  account: &str,
  typed: &str,
) -> Result<Vec<MailAddress>, MailFault> {
  let escaped: String = typed
    .trim()
    .to_lowercase()
    .chars()
    .flat_map(|c| if matches!(c, '%' | '_' | '\\') { vec!['\\', c] } else { vec![c] })
    .collect();
  let like = format!("%{escaped}%");
  let rows: Vec<(String, String)> = sqlx::query_as(
    "SELECT name, address FROM addresses
     WHERE account = ? AND (address LIKE ? ESCAPE '\\' OR lower(name) LIKE ? ESCAPE '\\')
     ORDER BY uses DESC, last_used DESC LIMIT ?",
  )
  .bind(account)
  .bind(&like)
  .bind(&like)
  .bind(SUGGESTED)
  .fetch_all(pool)
  .await?;

  Ok(rows.into_iter().map(|(name, address)| MailAddress { name, address }).collect())
}
