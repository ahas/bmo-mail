//! The store: one SQLite file, opened once as the service starts, holding the accounts with their secrets
//! sealed, the labels, the filter rules, the accounts' settings, the always-spam senders, the watcher's marks,
//! the known addresses, the sender logos, the event window and the record of the accounts file's import
//! (`docs/api.md`, 「The store」). Each module that keeps rows asks the pool itself.

use std::ffi::{c_int, c_void};
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use libsqlite3_sys::{SQLITE_OK, sqlite3_busy_handler};
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};

/// How long a statement that met another's lock sleeps before it asks for the lock again.
const BUSY_PAUSE: Duration = Duration::from_millis(10);
/// The span sqlx takes for how long a statement waits for a connection of the pool. It has no wait without
/// one, so it is given one longer than any run of the service: the wait ends when a connection comes back.
const UNBOUNDED: Duration = Duration::from_secs(100 * 365 * 24 * 60 * 60);

/// SQLite's busy handler on every connection of the store: a statement that meets another's lock sleeps a
/// moment and asks again, for as long as the lock is held (`docs/api.md`, 「The store」).
extern "C" fn keep_waiting(_: *mut c_void, _asked: c_int) -> c_int {
  std::thread::sleep(BUSY_PAUSE);
  1
}

/// The store's migrations, from the 1.0.0 baseline (`docs/api.md`, 「The store」).
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// The record of migrations an install from before 1.0.0 holds once every migration of its last release ran,
/// each by its version and description. Its schema is the baseline's.
pub const BEFORE: [(i64, &str); 6] = [
  (1, "mail"),
  (2, "events"),
  (3, "accounts"),
  (4, "arrivals"),
  (5, "pop3"),
  (6, "servers"),
];

/// The store, open.
#[derive(Clone, Debug)]
pub struct Store {
  pool: SqlitePool,
}

impl Store {
  /// The store at `path`, made where it does not stand yet, its folder with it, and migrated.
  pub async fn open(path: &Path) -> Result<Self, StoreError> {
    if let Some(folder) = path.parent().filter(|folder| !folder.as_os_str().is_empty()) {
      std::fs::create_dir_all(folder)
        .map_err(|why| StoreError::Folder(folder.display().to_string(), why))?;
    }

    let options = SqliteConnectOptions::from_str("sqlite:")?
      .filename(path)
      .create_if_missing(true)
      .journal_mode(SqliteJournalMode::Wal);
    let pool = SqlitePoolOptions::new()
      .acquire_timeout(UNBOUNDED)
      .after_connect(|connection, _| {
        Box::pin(async move {
          let mut handle = connection.lock_handle().await?;
          // SAFETY: the handle is the connection's own, held locked for the call, and the handler is a
          // plain function that touches no data.
          let set = unsafe {
            sqlite3_busy_handler(handle.as_raw_handle().as_ptr(), Some(keep_waiting), std::ptr::null_mut())
          };

          if set == SQLITE_OK {
            Ok(())
          } else {
            Err(sqlx::Error::Protocol(format!("the store's busy handler could not be set: {set}")))
          }
        })
      })
      .connect_with(options)
      .await?;

    adopt(&pool).await?;
    MIGRATOR.run(&pool).await?;

    Ok(Self { pool })
  }

  /// The pool the statements run on.
  pub fn pool(&self) -> &SqlitePool {
    &self.pool
  }
}

/// A record naming every migration of the last release before 1.0.0, and nothing else, rewritten to name the
/// baseline alone, touching no table and no row; one naming only part of them refused.
async fn adopt(pool: &SqlitePool) -> Result<(), StoreError> {
  let mut tx = pool.begin().await?;
  let recorded: bool = sqlx::query_scalar(
    "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations')",
  )
  .fetch_one(&mut *tx)
  .await?;
  if !recorded {
    return Ok(());
  }
  let held: Vec<(i64, String, bool)> =
    sqlx::query_as("SELECT version, description, success FROM _sqlx_migrations ORDER BY version")
      .fetch_all(&mut *tx)
      .await?;
  let before = |(version, description, success): &(i64, String, bool)| {
    *success && BEFORE.iter().any(|(old, named)| old == version && named == description)
  };
  if held.is_empty() || !held.iter().all(before) {
    return Ok(());
  }
  if held.len() != BEFORE.len() {
    return Err(StoreError::Before(held.last().map_or(0, |(version, _, _)| *version)));
  }
  let baseline = MIGRATOR.iter().next().expect("the baseline migration is built in");
  sqlx::query("DELETE FROM _sqlx_migrations").execute(&mut *tx).await?;
  sqlx::query(
    "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) \
     VALUES ($1, $2, TRUE, $3, 0)",
  )
  .bind(baseline.version)
  .bind(baseline.description.as_ref())
  .bind(baseline.checksum.as_ref())
  .execute(&mut *tx)
  .await?;
  tx.commit().await?;
  Ok(())
}

/// Why the store could not be opened.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
  #[error("the store's folder {0} could not be made: {1}")]
  Folder(String, std::io::Error),
  #[error("the store could not be opened: {0}")]
  Open(#[from] sqlx::Error),
  #[error("the store could not be migrated: {0}")]
  Migrate(#[from] sqlx::migrate::MigrateError),
  #[error(
    "the store holds the migrations of a release before 1.0.0 only through version {0}; that release's last \
     migration runs first"
  )]
  Before(i64),
}
