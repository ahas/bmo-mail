//! The accounts (`docs/mail.md`, 「Accounts and credentials」): each one an owner's, of the kind whose adapter
//! reaches its mailbox, kept in the store with its secrets sealed (`crate::secrets`). An owner key reaches its
//! own accounts alone: every read here is under the owner it names. Everything the store holds under an account
//! is keyed by the account's id, so a rename moves nothing and a removal takes it all.

pub mod authorizations;
pub mod file;
pub mod import;

use std::fmt;

use sqlx::{SqliteConnection, SqlitePool};
use tokio::sync::Notify;

use crate::routes::requests::{Asked, MailFault};
use crate::secrets::Secrets;
use crate::wire::Fields;

/// The tables whose rows are an account's, by its id written as text in `account`.
pub const ACCOUNT_TABLES: [&str; 6] = ["labels", "rules", "settings", "spam_senders", "marks", "addresses"];

/// The longest a name may be.
const NAME_LONGEST: usize = 64;

/// An account as the service holds it, its secrets opened.
#[derive(Clone, PartialEq, Eq)]
pub struct Account {
  pub id: i64,
  /// The owner key it stands under.
  pub owner: String,
  /// Unique among the owner's accounts, in lower case.
  pub name: String,
  /// The adapter's name.
  pub kind: String,
  pub address: String,
  /// Its place in its owner's list, from 1.
  pub position: i64,
  pub settings: Fields,
  pub secrets: Fields,
}

// An account written into a log names its id, name, kind and address, never a secret.
impl fmt::Debug for Account {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter
      .debug_struct("Account")
      .field("id", &self.id)
      .field("name", &self.name)
      .field("kind", &self.kind)
      .field("address", &self.address)
      .finish_non_exhaustive()
  }
}

impl Account {
  /// The key the store holds the account's rows under.
  pub fn store_key(&self) -> String {
    self.id.to_string()
  }
}

/// An account to keep: everything but its id and its place.
#[derive(Clone)]
pub struct Keeping {
  pub name: String,
  pub kind: String,
  pub address: String,
  pub settings: Fields,
  pub secrets: Fields,
}

/// A name as an account is kept under, or why it is not one: 1 to 64 characters with no slash, no space and
/// nothing unprintable, kept in lower case.
pub fn account_name(name: &str) -> Result<String, Asked> {
  let name = name.trim().to_lowercase();
  let shaped = !name.is_empty()
    && name.chars().count() <= NAME_LONGEST
    && !name.chars().any(|c| c == '/' || c.is_whitespace() || c.is_control());

  if shaped {
    Ok(name)
  } else {
    Err(Asked::bad_request("an account's name is 1 to 64 characters with no slash or space"))
  }
}

/// A row of `accounts`, its secret still sealed.
type Row = (i64, String, String, String, String, i64, String, Vec<u8>);

const COLUMNS: &str = "id, owner, name, kind, address, position, settings, secret";

/// The accounts in the store, and the word that they changed.
pub struct Accounts {
  pool: SqlitePool,
  secrets: Option<Secrets>,
  changed: Notify,
}

impl fmt::Debug for Accounts {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter
      .debug_struct("Accounts")
      .field("sealing", &self.secrets.is_some())
      .finish_non_exhaustive()
  }
}

fn fields(text: &str) -> Result<Fields, MailFault> {
  serde_json::from_str(text).map_err(|_| MailFault::server("an account's fields in the store do not read"))
}

fn written(fields: &Fields) -> String {
  serde_json::to_string(fields).unwrap_or_else(|_| "{}".to_owned())
}

/// The secrets of an account sealed under its id.
pub fn sealed(secrets: &Secrets, id: i64, fields: &Fields) -> Result<Vec<u8>, MailFault> {
  secrets.seal(id, written(fields).as_bytes()).map_err(MailFault::server)
}

/// Keeps an account on a connection already in a transaction, placed at `position`, and answers its id. Its
/// secrets are sealed under that id once the row stands.
pub async fn keep(
  connection: &mut SqliteConnection,
  secrets: &Secrets,
  owner: &str,
  account: &Keeping,
  position: i64,
) -> Result<i64, MailFault> {
  let id: i64 = sqlx::query_scalar(
    "INSERT INTO accounts (owner, name, kind, address, position, settings, secret, created_at)
     VALUES (?, ?, ?, ?, ?, ?, X'', ?) RETURNING id",
  )
  .bind(owner)
  .bind(&account.name)
  .bind(&account.kind)
  .bind(&account.address)
  .bind(position)
  .bind(written(&account.settings))
  .bind(chrono::Utc::now().timestamp_millis())
  .fetch_one(&mut *connection)
  .await?;

  sqlx::query("UPDATE accounts SET secret = ? WHERE id = ?")
    .bind(sealed(secrets, id, &account.secrets)?)
    .bind(id)
    .execute(&mut *connection)
    .await?;

  Ok(id)
}

/// Whether the owner holds an account of that name, another than `but`.
async fn name_taken(
  connection: &mut SqliteConnection,
  owner: &str,
  name: &str,
  but: Option<i64>,
) -> Result<bool, MailFault> {
  let found: Option<i64> = sqlx::query_scalar("SELECT id FROM accounts WHERE owner = ? AND name = ?")
    .bind(owner)
    .bind(name)
    .fetch_optional(&mut *connection)
    .await?;

  Ok(found.is_some_and(|id| Some(id) != but))
}

/// Numbers the owner's accounts from 1 in the order given.
async fn renumber(connection: &mut SqliteConnection, ids: &[i64]) -> Result<(), MailFault> {
  for (index, id) in ids.iter().enumerate() {
    sqlx::query("UPDATE accounts SET position = ? WHERE id = ?")
      .bind(index as i64 + 1)
      .bind(id)
      .execute(&mut *connection)
      .await?;
  }

  Ok(())
}

impl Accounts {
  pub fn new(pool: SqlitePool, secrets: Option<Secrets>) -> Self {
    Self {
      pool,
      secrets,
      changed: Notify::new(),
    }
  }

  /// The key secrets are sealed with, or a refusal as not ready where the service has none.
  pub fn secrets(&self) -> Result<&Secrets, MailFault> {
    self.secrets.as_ref().ok_or_else(|| {
      Asked::not_ready("the service has no key file, so it keeps no account's secrets").into()
    })
  }

  /// Whether the service has a key to seal secrets with.
  pub fn sealing(&self) -> bool {
    self.secrets.is_some()
  }

  /// Resolves the next time an account was added, changed or removed.
  pub async fn changed(&self) {
    self.changed.notified().await;
  }

  /// Says that an account was added, changed or removed, so the watchers follow at once.
  pub fn touch(&self) {
    self.changed.notify_one();
  }

  /// How many accounts the store holds, every owner's together.
  pub async fn count(&self) -> Result<i64, MailFault> {
    Ok(sqlx::query_scalar("SELECT COUNT(*) FROM accounts").fetch_one(&self.pool).await?)
  }

  fn opened(&self, row: Row) -> Result<Account, MailFault> {
    let (id, owner, name, kind, address, position, settings, secret) = row;
    let secrets = self.secrets()?;
    let opened = secrets.open(id, &secret).map_err(MailFault::server)?;
    let secrets =
      fields(&String::from_utf8(opened).map_err(|_| MailFault::server("a secret does not read"))?)?;

    Ok(Account {
      id,
      owner,
      name,
      kind,
      address,
      position,
      settings: fields(&settings)?,
      secrets,
    })
  }

  /// The owner's accounts, in their order. An owner whose service keeps no key holds none.
  pub async fn list(&self, owner: &str) -> Result<Vec<Account>, MailFault> {
    if !self.sealing() {
      return Ok(Vec::new());
    }

    let rows: Vec<Row> =
      sqlx::query_as(&format!("SELECT {COLUMNS} FROM accounts WHERE owner = ? ORDER BY position, id"))
        .bind(owner)
        .fetch_all(&self.pool)
        .await?;

    rows.into_iter().map(|row| self.opened(row)).collect()
  }

  /// Every account of every owner, for the watchers.
  pub async fn all(&self) -> Result<Vec<Account>, MailFault> {
    if !self.sealing() {
      return Ok(Vec::new());
    }

    let rows: Vec<Row> =
      sqlx::query_as(&format!("SELECT {COLUMNS} FROM accounts ORDER BY owner, position"))
        .fetch_all(&self.pool)
        .await?;

    rows.into_iter().map(|row| self.opened(row)).collect()
  }

  /// The owner's account a name names, matched without regard to case.
  pub async fn find(&self, owner: &str, name: &str) -> Result<Option<Account>, MailFault> {
    if !self.sealing() {
      return Ok(None);
    }

    let row: Option<Row> =
      sqlx::query_as(&format!("SELECT {COLUMNS} FROM accounts WHERE owner = ? AND name = ?"))
        .bind(owner)
        .bind(name.trim().to_lowercase())
        .fetch_optional(&self.pool)
        .await?;

    row.map(|row| self.opened(row)).transpose()
  }

  /// Keeps a new account of the owner after the owner's others; a name the owner holds is a conflict.
  pub async fn add(&self, owner: &str, account: &Keeping) -> Result<Account, MailFault> {
    let secrets = self.secrets()?;
    let mut transaction = self.pool.begin().await?;

    if name_taken(&mut transaction, owner, &account.name, None).await? {
      return Err(Asked::conflict("the owner holds an account of that name").into());
    }

    let last: Option<i64> = sqlx::query_scalar("SELECT MAX(position) FROM accounts WHERE owner = ?")
      .bind(owner)
      .fetch_one(&mut *transaction)
      .await?;
    let id = keep(&mut transaction, secrets, owner, account, last.unwrap_or(0) + 1).await?;

    transaction.commit().await?;
    self.touch();

    let kept = self.find(owner, &account.name).await?;

    kept
      .filter(|kept| kept.id == id)
      .ok_or_else(|| MailFault::server("the kept account could not be read"))
  }

  /// Whether the owner holds an account of that name, another than `but`.
  pub async fn taken(&self, owner: &str, name: &str, but: Option<i64>) -> Result<bool, MailFault> {
    let mut connection = self.pool.acquire().await?;

    name_taken(&mut connection, owner, name, but).await
  }

  /// Writes an account's name, kind, address, settings and secrets as they now stand, and moves it to
  /// `position` among its owner's others where one is given.
  pub async fn change(&self, account: &Account, position: Option<i64>) -> Result<Account, MailFault> {
    let secrets = self.secrets()?;
    let mut transaction = self.pool.begin().await?;

    if name_taken(&mut transaction, &account.owner, &account.name, Some(account.id)).await? {
      return Err(Asked::conflict("the owner holds an account of that name").into());
    }

    sqlx::query(
      "UPDATE accounts SET name = ?, kind = ?, address = ?, settings = ?, secret = ? WHERE id = ?",
    )
    .bind(&account.name)
    .bind(&account.kind)
    .bind(&account.address)
    .bind(written(&account.settings))
    .bind(sealed(secrets, account.id, &account.secrets)?)
    .bind(account.id)
    .execute(&mut *transaction)
    .await?;

    if let Some(position) = position {
      let mut ids: Vec<i64> =
        sqlx::query_scalar("SELECT id FROM accounts WHERE owner = ? ORDER BY position, id")
          .bind(&account.owner)
          .fetch_all(&mut *transaction)
          .await?;

      ids.retain(|id| *id != account.id);

      let place = usize::try_from(position.saturating_sub(1)).unwrap_or(0).min(ids.len());

      ids.insert(place, account.id);
      renumber(&mut transaction, &ids).await?;
    }

    transaction.commit().await?;
    self.touch();

    self
      .find(&account.owner, &account.name)
      .await?
      .ok_or_else(|| MailFault::server("the changed account could not be read"))
  }

  /// Forgets an account and every row the store holds under it, and numbers its owner's others again.
  pub async fn remove(&self, account: &Account) -> Result<(), MailFault> {
    let mut transaction = self.pool.begin().await?;

    sqlx::query("DELETE FROM accounts WHERE id = ?")
      .bind(account.id)
      .execute(&mut *transaction)
      .await?;

    for table in ACCOUNT_TABLES {
      sqlx::query(&format!("DELETE FROM {table} WHERE account = ?"))
        .bind(account.store_key())
        .execute(&mut *transaction)
        .await?;
    }

    let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM accounts WHERE owner = ? ORDER BY position, id")
      .bind(&account.owner)
      .fetch_all(&mut *transaction)
      .await?;

    renumber(&mut transaction, &ids).await?;
    transaction.commit().await?;
    self.touch();

    Ok(())
  }
}
