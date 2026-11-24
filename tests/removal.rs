//! The platform administrator's removal of an owner or a unit for good (`docs/api.md`, 「Removing an owner」): the
//! count changes nothing and names what the removal then takes; the owner's accounts and own servers go, another
//! owner's stay, a unit's server an account still stands on stays, a second removal takes nothing, and a console
//! call for anyone but the administrator is refused.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bmo_mail::client::MailClient;
use bmo_mail::http::{Standing, router};
use bmo_mail::secrets::Secrets;
use bmo_mail::settings::Settings;
use bmo_mail::store::Store;
use bmo_mail::wire::{NewServer, Protocol, Role};
use bmo_mail::wording::Wording;
use serde_json::{Value, json};
use sqlx::SqlitePool;
use sqlx::sqlite::SqliteConnectOptions;

const TOKEN: &str = "test-token";
const CONSOLE: &str = "console-token";
/// The owner removed, and one whose things stay.
const GOING: &str = "7";
const STAYING: &str = "8";

fn scratch(name: &str) -> PathBuf {
  let folder = std::env::temp_dir().join(format!("bmo-mail-removal-{}-{name}", std::process::id()));
  let _ = std::fs::remove_dir_all(&folder);
  std::fs::create_dir_all(&folder).expect("the scratch folder is made");
  folder
}

/// The service answering on a port of its own, and its store's file.
async fn service(folder: &Path) -> (String, PathBuf) {
  let environment = HashMap::from([
    ("BMO_MAIL_SERVER__TOKEN".to_owned(), TOKEN.to_owned()),
    ("BMO_MAIL_CONSOLE__TOKEN".to_owned(), CONSOLE.to_owned()),
  ]);
  let settings = Settings::read(folder, Some(environment)).expect("the settings read");
  let path = settings.store.path.clone();
  let store = Store::open(&settings.store.path).await.expect("the store opens");
  let wording = Wording::load(&settings).expect("the words load");
  let standing = Arc::new(Standing::new(settings, store, wording, Some(Secrets::new(&[7; 32]))));
  let app = router(standing).expect("the routes stand");
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("the service listens");
  let address = format!("http://{}", listener.local_addr().expect("the address is known"));
  tokio::spawn(async move { axum::serve(listener, app).await });
  (address, path)
}

fn own_server(port: u16) -> NewServer {
  NewServer {
    link: None,
    protocol: Protocol::Imap,
    name: None,
    host: "127.0.0.1".to_owned(),
    port: Some(port),
    security: None,
    tls_name: None,
    auth: None,
  }
}

/// An account's row and one row beside it, written straight into the store: the routes that make one sign in.
async fn account_row(pool: &SqlitePool, owner: &str, name: &str, server: Option<i64>) -> i64 {
  let id: i64 = sqlx::query_scalar(
    "INSERT INTO accounts (owner, name, kind, address, position, settings, secret, created_at, incoming_server) \
     VALUES (?, ?, 'imap', ?, 1, '{}', x'00', 0, ?) RETURNING id",
  )
  .bind(owner)
  .bind(name)
  .bind(format!("{name}@example.test"))
  .bind(server)
  .fetch_one(pool)
  .await
  .expect("the account's row is written");
  sqlx::query("INSERT INTO addresses (account, address, name, last_used, uses) VALUES (?, 'a@example.test', '', 0, 1)")
    .bind(id.to_string())
    .execute(pool)
    .await
    .expect("an address is kept");
  id
}

async fn called(
  address: &str,
  method: reqwest::Method,
  path: &str,
  token: &str,
  role: Option<&str>,
) -> (u16, Value) {
  let mut request = reqwest::Client::new().request(method, format!("{address}{path}")).bearer_auth(token);
  if let Some(role) = role {
    request = request.header("Owner-Role", role).header("Owner-Key", "1");
  }
  let answer = request.send().await.expect("the service answers");
  let status = answer.status().as_u16();
  (status, answer.json().await.unwrap_or(Value::Null))
}

#[tokio::test]
async fn an_owners_accounts_and_own_servers_go_and_nothing_else_does() {
  let folder = scratch("owner");
  let (address, path) = service(&folder).await;
  let going = MailClient::new(&address, TOKEN).acting_for(GOING, Role::Member);
  let staying = MailClient::new(&address, TOKEN).acting_for(STAYING, Role::Member);
  going.create_server(&own_server(993)).await.unwrap();
  going.create_server(&own_server(994)).await.unwrap();
  staying.create_server(&own_server(995)).await.unwrap();
  let pool = SqlitePool::connect_with(SqliteConnectOptions::new().filename(&path)).await.unwrap();
  account_row(&pool, GOING, "one", None).await;
  account_row(&pool, GOING, "two", None).await;
  account_row(&pool, STAYING, "one", None).await;

  let owner = format!("/owners/{GOING}");
  let (status, counted) = called(
    &address,
    reqwest::Method::GET,
    &format!("{owner}/removal"),
    CONSOLE,
    Some("administrator"),
  )
  .await;
  assert_eq!(status, 200, "{counted}");
  assert_eq!(counted, json!({ "counts": { "accounts": 2, "servers": 2 } }));

  let (status, removed) =
    called(&address, reqwest::Method::DELETE, &owner, CONSOLE, Some("administrator")).await;
  assert_eq!(status, 200, "{removed}");
  assert_eq!(removed["removed"], counted["counts"]);
  let left: Vec<(String, String)> =
    sqlx::query_as("SELECT owner, name FROM accounts").fetch_all(&pool).await.unwrap();
  assert_eq!(left, [(STAYING.to_owned(), "one".to_owned())]);
  let addresses: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM addresses").fetch_one(&pool).await.unwrap();
  assert_eq!(addresses, 1, "the rows under the owner's accounts went with them");
  let holders: Vec<String> =
    sqlx::query_scalar("SELECT holder FROM servers").fetch_all(&pool).await.unwrap();
  assert_eq!(holders, [format!("owner:{STAYING}")]);

  let (status, again) = called(&address, reqwest::Method::DELETE, &owner, TOKEN, None).await;
  assert_eq!((status, again), (200, json!({ "removed": { "accounts": 0, "servers": 0 } })));
}

#[tokio::test]
async fn a_units_servers_go_but_one_an_account_stands_on_stays() {
  let folder = scratch("unit");
  let (address, path) = service(&folder).await;
  let pool = SqlitePool::connect_with(SqliteConnectOptions::new().filename(&path)).await.unwrap();
  let mut ids = Vec::new();
  for port in [993, 994] {
    let id: i64 = sqlx::query_scalar(
      "INSERT INTO servers (holder, protocol, name, host, port, security, auth, created_at) \
       VALUES ('unit:13', 'imap', 'unit', '127.0.0.1', ?, 'tls', 'password', 0) RETURNING id",
    )
    .bind(port)
    .fetch_one(&pool)
    .await
    .expect("the unit's server is kept");
    ids.push(id);
  }
  account_row(&pool, STAYING, "one", Some(ids[0])).await;

  let (status, counted) =
    called(&address, reqwest::Method::GET, "/org-units/13/removal?company=3", TOKEN, None).await;
  assert_eq!((status, &counted), (200, &json!({ "counts": { "servers": 1, "servers-in-use": 1 } })));
  let (status, removed) =
    called(&address, reqwest::Method::DELETE, "/org-units/13?company=3", TOKEN, None).await;
  assert_eq!((status, &removed["removed"]), (200, &counted["counts"]));
  let left: Vec<i64> = sqlx::query_scalar("SELECT id FROM servers").fetch_all(&pool).await.unwrap();
  assert_eq!(left, [ids[0]], "the server an account stands on stays");
}

#[tokio::test]
async fn a_removal_is_the_administrators_alone_and_names_a_key() {
  let folder = scratch("refused");
  let (address, _) = service(&folder).await;
  let (status, _) = called(&address, reqwest::Method::DELETE, "/owners/7", CONSOLE, Some("member")).await;
  assert_eq!(status, 403);
  let (status, _) = called(&address, reqwest::Method::DELETE, "/owners/seven", TOKEN, None).await;
  assert_eq!(status, 400);
  let (status, _) = called(&address, reqwest::Method::DELETE, "/owners/7", "wrong", None).await;
  assert_eq!(status, 401);
}
