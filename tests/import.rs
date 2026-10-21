//! The old archive's seven mail tables moved into the store (TODO #1923): the renames, the sender logos kept
//! as bytes, the refusal of a store already holding rows, the dry run, and the count-and-sample compare.

use std::path::{Path, PathBuf};

use bmo_mail::import;
use bmo_mail::store::Store;

fn scratch(name: &str) -> PathBuf {
  let folder = std::env::temp_dir().join(format!("bmo-mail-import-{}-{name}", std::process::id()));
  let _ = std::fs::remove_dir_all(&folder);
  std::fs::create_dir_all(&folder).expect("the scratch folder is made");

  folder
}

const TABLES: [&str; 7] = [
  "web_mail_addresses",
  "web_mail_avatars",
  "web_mail_labels",
  "web_mail_marks",
  "web_mail_rules",
  "web_mail_settings",
  "web_mail_spam_senders",
];

fn export(folder: &Path, rows: [&[&str]; 7]) {
  let keys = [
    "[\"account\",\"address\"]",
    "[\"domain\"]",
    "[\"account\",\"id\"]",
    "[\"account\",\"folder\"]",
    "[\"id\"]",
    "[\"account\"]",
    "[\"account\",\"address\"]",
  ];
  let mut tables = Vec::new();
  for (index, table) in TABLES.iter().enumerate() {
    tables.push(format!(
      r#"{{"source":"{table}","file":"{table}.ndjson","count":{},"key":{}}}"#,
      rows[index].len(),
      keys[index]
    ));
    std::fs::write(folder.join(format!("{table}.ndjson")), rows[index].join("\n") + "\n")
      .expect("the rows are written");
  }
  let said = format!(
    r#"{{"snapshot":"1:2:","readAt":"2026-10-21T17:00:00+09:00","tables":[{}]}}"#,
    tables.join(",")
  );
  std::fs::write(folder.join("manifest.json"), said).expect("the manifest is written");
}

const ADDRESS: &str = r#"{"account":"dev@ahas.io","address":"one@example.com","name":"One","last_used":1791866256,"uses":2,"rowid":1,"user_id":1}"#;
const AVATAR: &str = r##"{"domain":"example.com","type":"image/svg+xml","bytes":"\\x3c737667","looked_up_at":1791866256,"rowid":1}"##;
const MARK: &str = r#"{"account":"dev@ahas.io","folder":"INBOX","uid_validity":"1640519300","last_uid":1232,"rowid":1,"user_id":1}"#;
const SETTINGS: &str = r#"{"account":"dev@ahas.io","signature":"-- ","receipt_default":0,"show_images":1,"rowid":1,"user_id":1}"#;

fn full(folder: &Path) {
  export(folder, [&[ADDRESS], &[AVATAR], &[], &[MARK], &[], &[SETTINGS], &[]]);
}

#[tokio::test]
async fn the_seven_tables_are_renamed_moved_and_compared() {
  let folder = scratch("samples");
  full(&folder);

  let store = folder.join("mail.db");
  let report = import::run(&store, &folder, false).await.expect("the import runs");
  assert_eq!(
    report.tables.iter().map(|table| table.written).collect::<Vec<_>>(),
    [1, 1, 0, 1, 0, 1, 0]
  );
  assert_eq!(report.sample.checked, 4);
  assert_eq!(report.sample.failures, Vec::<String>::new());

  let opened = Store::open(&store).await.expect("the store opens");
  let pool = opened.pool();
  let address: (String, String, i64) =
    sqlx::query_as("SELECT account, name, uses FROM addresses WHERE address = 'one@example.com'")
      .fetch_one(pool)
      .await
      .expect("the address is read");
  assert_eq!(address, ("dev@ahas.io".to_owned(), "One".to_owned(), 2));
  let logo: Vec<u8> = sqlx::query_scalar("SELECT bytes FROM avatars WHERE domain = 'example.com'")
    .fetch_one(pool)
    .await
    .expect("the logo is read");
  assert_eq!(logo, b"<svg");
  let last: i64 = sqlx::query_scalar("SELECT last_uid FROM marks WHERE account = 'dev@ahas.io'")
    .fetch_one(pool)
    .await
    .expect("the mark is read");
  assert_eq!(last, 1232);
  let images: i64 = sqlx::query_scalar("SELECT show_images FROM settings WHERE account = 'dev@ahas.io'")
    .fetch_one(pool)
    .await
    .expect("the settings are read");
  assert_eq!(images, 1);
}

#[tokio::test]
async fn a_dry_run_writes_nothing() {
  let folder = scratch("dry");
  full(&folder);

  let store = folder.join("mail.db");
  import::run(&store, &folder, true).await.expect("the dry run runs");

  let opened = Store::open(&store).await.expect("the store opens");
  let held: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM addresses")
    .fetch_one(opened.pool())
    .await
    .expect("the count is read");
  assert_eq!(held, 0);
}

#[tokio::test]
async fn a_store_holding_rows_is_refused() {
  let folder = scratch("refuse");
  full(&folder);

  let store = folder.join("mail.db");
  import::run(&store, &folder, false).await.expect("the first import runs");
  let refused = import::run(&store, &folder, false).await.expect_err("the second is refused");
  assert!(refused.contains("already holds rows"), "{refused}");
  assert!(refused.contains("addresses holds 1"), "{refused}");
}
