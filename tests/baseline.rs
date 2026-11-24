//! The store's starting point (docs/api.md, 「The store」): a new file takes the 1.0.0 baseline whole, an
//! install holding the record of the last release before 1.0.0 is taken as it stands, and one holding part of
//! it is refused.

use bmo_mail::store::{BEFORE, Store};

const SCHEMA: &str = include_str!("../migrations/0001_baseline.sql");

// The record's table as the migrator makes it.
const RECORD: &str = "CREATE TABLE _sqlx_migrations (version BIGINT PRIMARY KEY, description TEXT NOT NULL, \
  installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP, success BOOLEAN NOT NULL, checksum BLOB NOT NULL, \
  execution_time BIGINT NOT NULL);";

// A folder of the test's own, made empty.
fn scratch(name: &str) -> std::path::PathBuf {
  let folder = std::env::temp_dir().join(format!("bmo-mail-baseline-{}-{name}", std::process::id()));
  let _ = std::fs::remove_dir_all(&folder);
  std::fs::create_dir_all(&folder).expect("the scratch folder is made");
  folder
}

async fn pool(path: &std::path::Path) -> sqlx::SqlitePool {
  sqlx::SqlitePool::connect(&format!("sqlite://{}?mode=rwc", path.display()))
    .await
    .expect("the file opens")
}

// Every object of the file but the record's, as SQLite keeps it.
async fn shape(pool: &sqlx::SqlitePool) -> Vec<(String, String)> {
  sqlx::query_as(
    "SELECT name, COALESCE(sql, '') FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' \
     AND tbl_name <> '_sqlx_migrations' ORDER BY type, name",
  )
  .fetch_all(pool)
  .await
  .expect("the objects")
}

async fn recorded(pool: &sqlx::SqlitePool) -> Vec<(i64, String)> {
  sqlx::query_as("SELECT version, description FROM _sqlx_migrations ORDER BY version")
    .fetch_all(pool)
    .await
    .expect("the record")
}

// A file holding the baseline's schema and an import, recorded as an install from before 1.0.0 that ran the
// first `through` migrations of its last release.
async fn installed(path: &std::path::Path, through: usize) {
  let pool = pool(path).await;
  sqlx::raw_sql(SCHEMA).execute(&pool).await.expect("the schema");
  sqlx::raw_sql(RECORD).execute(&pool).await.expect("the record");
  for (version, description) in &BEFORE[..through] {
    sqlx::query(
      "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) \
       VALUES ($1, $2, TRUE, X'00', 1)",
    )
    .bind(version)
    .bind(description)
    .execute(&pool)
    .await
    .expect("a record");
  }
  sqlx::query("INSERT INTO imports (source, owner, count, at) VALUES ('source-a', 'owner-a', 1, 1)")
    .execute(&pool)
    .await
    .expect("an import");
  pool.close().await;
}

#[tokio::test]
async fn a_new_file_takes_the_baseline_whole() {
  let folder = scratch("new");
  drop(Store::open(&folder.join("opened.db")).await.expect("the store opens"));
  let opened = pool(&folder.join("opened.db")).await;
  let laid = pool(&folder.join("laid.db")).await;
  sqlx::raw_sql(SCHEMA).execute(&laid).await.expect("the schema");

  assert_eq!(shape(&opened).await, shape(&laid).await);
  assert_eq!(recorded(&opened).await, [(1, "baseline".to_owned())]);
}

#[tokio::test]
async fn an_install_at_the_last_release_before_the_baseline_only_has_its_record_rewritten() {
  let path = scratch("rewritten").join("mail.db");
  installed(&path, BEFORE.len()).await;
  let before = shape(&pool(&path).await).await;

  drop(Store::open(&path).await.expect("the store opens"));
  let opened = pool(&path).await;
  assert_eq!(recorded(&opened).await, [(1, "baseline".to_owned())]);
  assert_eq!(shape(&opened).await, before);
  let owners: Vec<String> =
    sqlx::query_scalar("SELECT owner FROM imports").fetch_all(&opened).await.unwrap();
  assert_eq!(owners, ["owner-a"]);

  // A second opening finds the baseline recorded and changes nothing.
  drop(Store::open(&path).await.expect("the store opens again"));
  assert_eq!(recorded(&opened).await, [(1, "baseline".to_owned())]);
  assert_eq!(shape(&opened).await, before);
}

#[tokio::test]
async fn an_install_short_of_the_last_release_before_the_baseline_is_refused() {
  let path = scratch("refused").join("mail.db");
  installed(&path, 2).await;

  let refused = Store::open(&path).await.expect_err("a store short of the last release is refused");
  assert!(refused.to_string().contains("through version 2"), "{refused}");
  assert_eq!(recorded(&pool(&path).await).await.len(), 2);
}
