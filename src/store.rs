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

    sqlx::migrate!("./migrations").run(&pool).await?;

    Ok(Self { pool })
  }

  /// The pool the statements run on.
  pub fn pool(&self) -> &SqlitePool {
    &self.pool
  }
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
}
