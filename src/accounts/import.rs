//! The accounts file imported once (`docs/mail.md`, 「The accounts file, imported once」): at start, while no
//! import is recorded, each account the file names is kept as an account of its kind under the administrator's
//! owner key, and everything the store already held under its address is moved under its id, in one
//! transaction. Once the import is recorded the file is never read again; a change of the administrator's key
//! carries the accounts the earlier key held to the new one.

use sqlx::SqlitePool;

use crate::accounts::file::read_accounts_file;
use crate::accounts::{ACCOUNT_TABLES, Accounts, Keeping, keep};
use crate::adapters::imap_smtp;
use crate::routes::requests::MailFault;
use crate::settings::Settings;

/// The name the accounts file's import is recorded under.
pub const SOURCE: &str = "accounts-file";

/// What a start did with the accounts file.
#[derive(Debug, PartialEq, Eq)]
pub enum Imported {
  /// It was imported now: how many accounts it named.
  Now(usize),
  /// It was imported before; the file was not read.
  Before,
  /// It waits, for the reason given: nothing named, no administrator, no key, or a file that does not read.
  Waiting(String),
}

/// The owner key the import was recorded under, where it was made.
async fn recorded(pool: &SqlitePool) -> Result<Option<String>, MailFault> {
  Ok(
    sqlx::query_scalar("SELECT owner FROM imports WHERE source = ?")
      .bind(SOURCE)
      .fetch_optional(pool)
      .await?,
  )
}

/// Carries every account of the earlier administrator's key, and its events, to the key the settings name
/// now. An account whose name the new key already holds stays where it was, said in the log by its name.
async fn follow(pool: &SqlitePool, earlier: &str, now: &str) -> Result<(), MailFault> {
  let mut transaction = pool.begin().await?;
  let staying: Vec<String> = sqlx::query_scalar(
    "SELECT name FROM accounts WHERE owner = ? AND name IN (SELECT name FROM accounts WHERE owner = ?)",
  )
  .bind(earlier)
  .bind(now)
  .fetch_all(&mut *transaction)
  .await?;

  sqlx::query(
    "UPDATE accounts SET owner = ?,
       position = position + (SELECT COALESCE(MAX(position), 0) FROM accounts WHERE owner = ?)
     WHERE owner = ? AND name NOT IN (SELECT name FROM accounts WHERE owner = ?)",
  )
  .bind(now)
  .bind(now)
  .bind(earlier)
  .bind(now)
  .execute(&mut *transaction)
  .await?;
  sqlx::query("UPDATE events SET owner = ? WHERE owner = ?")
    .bind(now)
    .bind(earlier)
    .execute(&mut *transaction)
    .await?;
  sqlx::query("UPDATE imports SET owner = ? WHERE source = ?")
    .bind(now)
    .bind(SOURCE)
    .execute(&mut *transaction)
    .await?;
  transaction.commit().await?;

  for name in staying {
    eprintln!(
      "mail: the account {name} stays with the earlier administrator key; the new one holds that name"
    );
  }

  Ok(())
}

/// Imports the accounts file the settings name, once; or, where it was imported before, carries the imported
/// accounts to the administrator key the settings name now.
pub async fn import_accounts_file(
  pool: &SqlitePool,
  accounts: &Accounts,
  settings: &Settings,
) -> Result<Imported, MailFault> {
  let administrator = settings.accounts.administrator.trim();

  if let Some(earlier) = recorded(pool).await? {
    if !administrator.is_empty() && administrator != earlier {
      follow(pool, &earlier, administrator).await?;
    }

    return Ok(Imported::Before);
  }

  let Some(path) = &settings.accounts.file else {
    return Ok(Imported::Waiting("the settings name no accounts file".to_owned()));
  };

  if administrator.is_empty() {
    return Ok(Imported::Waiting("the settings name no administrator owner key".to_owned()));
  }

  let Ok(secrets) = accounts.secrets() else {
    return Ok(Imported::Waiting("the settings name no key file".to_owned()));
  };
  let config = match read_accounts_file(path) {
    Ok(Some(config)) => config,
    Ok(None) => return Ok(Imported::Waiting(format!("{} does not stand", path.display()))),
    Err(why) => return Ok(Imported::Waiting(why.to_string())),
  };
  let mut transaction = pool.begin().await?;
  let first: Option<i64> = sqlx::query_scalar("SELECT MAX(position) FROM accounts WHERE owner = ?")
    .bind(administrator)
    .fetch_one(&mut *transaction)
    .await?;

  for (index, one) in config.accounts.iter().enumerate() {
    let (settings, secrets_of) = imap_smtp::file_fields(&config.imap, &config.smtp, &one.account);
    let keeping = Keeping {
      name: one.key.clone(),
      kind: imap_smtp::KIND.to_owned(),
      address: one.account.address.clone(),
      settings,
      secrets: secrets_of,
    };
    let position = first.unwrap_or(0) + index as i64 + 1;
    let id = keep(&mut transaction, secrets, administrator, &keeping, position)
      .await
      .map_err(|_| MailFault::server(format!("the account {} could not be kept", one.key)))?;

    for table in ACCOUNT_TABLES {
      sqlx::query(&format!("UPDATE {table} SET account = ? WHERE account = ?"))
        .bind(id.to_string())
        .bind(&one.account.address)
        .execute(&mut *transaction)
        .await?;
    }

    sqlx::query("UPDATE events SET owner = ? WHERE owner IS NULL AND account = ?")
      .bind(administrator)
      .bind(&one.key)
      .execute(&mut *transaction)
      .await?;
  }

  sqlx::query("INSERT INTO imports (source, owner, count, at) VALUES (?, ?, ?, ?)")
    .bind(SOURCE)
    .bind(administrator)
    .bind(config.accounts.len() as i64)
    .bind(chrono::Utc::now().timestamp_millis())
    .execute(&mut *transaction)
    .await?;
  transaction.commit().await?;
  accounts.touch();

  Ok(Imported::Now(config.accounts.len()))
}
