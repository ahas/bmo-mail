//! The accounts (`docs/mail.md`, 「Accounts and credentials」; `docs/api.md`, 「Accounts」): their routes, one
//! owner key never reaching another's, their secrets sealed at rest, and the accounts file imported once into
//! the administrator's accounts with everything the store held for each address.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bmo_mail::accounts::import::{Imported, import_accounts_file};
use bmo_mail::accounts::{Account, Keeping};
use bmo_mail::client::{ClientFailure, MailClient};
use bmo_mail::http::{Standing, router};
use bmo_mail::secrets::{Secrets, SecretsError};
use bmo_mail::settings::Settings;
use bmo_mail::store::Store;
use bmo_mail::wire::{AccountPatch, EventQuery, Fields, ListQuery, NewAccount, NewAuthorization};
use bmo_mail::wording::Wording;

const TOKEN: &str = "test-token";
const OWNER: &str = "owner-one";
const OTHER: &str = "owner-two";
const KEY: [u8; 32] = [7; 32];

/// Chooses the one crypto provider every TLS client of the service runs on, as the binary does at start.
fn provider() {
  let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
}

fn scratch(name: &str) -> PathBuf {
  provider();

  let folder = std::env::temp_dir().join(format!("bmo-mail-accounts-{}-{name}", std::process::id()));
  let _ = std::fs::remove_dir_all(&folder);
  std::fs::create_dir_all(&folder).expect("the scratch folder is made");

  folder
}

/// The settings of a service in `folder`: its token, and the given keys besides, as the environment sets them.
fn settings(folder: &Path, keys: &[(&str, String)]) -> Settings {
  let mut environment = HashMap::from([("BMO_MAIL_SERVER__TOKEN".to_owned(), TOKEN.to_owned())]);

  environment.extend(keys.iter().map(|(key, value)| ((*key).to_owned(), value.clone())));

  Settings::read(folder, Some(environment)).expect("the settings read")
}

/// A port on this machine nothing listens on, so a sign-in there is refused at once.
fn closed_port() -> u16 {
  let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port is taken");

  listener.local_addr().expect("the port is known").port()
}

fn fields(pairs: &[(&str, &str)]) -> Fields {
  pairs.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect()
}

/// An `imap-smtp` account's fields at a server that does not answer.
fn unanswered() -> (Fields, Fields) {
  let port = closed_port().to_string();
  let settings = fields(&[
    ("imapHost", "127.0.0.1"),
    ("imapPort", &port),
    ("imapTlsName", "-"),
    ("smtpHost", "127.0.0.1"),
    ("smtpPort", &port),
    ("smtpTlsName", "-"),
  ]);

  (settings, fields(&[("password", "not-a-real-one")]))
}

/// A service answering on a port of its own, and its standing.
async fn service(folder: &Path, sealing: bool) -> (Arc<Standing>, String) {
  let settings = settings(folder, &[]);
  let store = Store::open(&settings.store.path).await.expect("the store opens");
  let wording = Wording::load(&settings).expect("the words load");
  let secrets = sealing.then(|| Secrets::new(&KEY));
  let standing = Arc::new(Standing::new(settings, store, wording, secrets));
  let app = router(Arc::clone(&standing)).expect("the routes stand");
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("the service listens");
  let address = format!("http://{}", listener.local_addr().expect("the address is known"));

  tokio::spawn(async move { axum::serve(listener, app).await });

  (standing, address)
}

/// The status and code a refused call answered.
fn refused(failure: ClientFailure) -> (u16, String) {
  match failure {
    ClientFailure::Answered { status, answer, .. } => {
      (status, answer.map(|one| one.code).unwrap_or_default())
    }
    other => panic!("the call was not answered: {other}"),
  }
}

async fn keep(standing: &Standing, owner: &str, name: &str) -> Account {
  let (settings, secrets) = unanswered();

  standing
    .accounts
    .add(
      owner,
      &Keeping {
        name: name.to_owned(),
        kind: "imap-smtp".to_owned(),
        address: format!("{name}@example.com"),
        settings,
        secrets,
      },
    )
    .await
    .expect("the account is kept")
}

#[test]
fn a_secret_opens_for_its_own_account_under_its_own_key_alone() {
  let secrets = Secrets::new(&KEY);
  let sealed = secrets.seal(7, b"the password").expect("the secret seals");

  assert!(!sealed.windows(12).any(|window| window == b"the password"));
  assert_eq!(secrets.open(7, &sealed).expect("the secret opens"), b"the password");
  assert_eq!(secrets.open(8, &sealed), Err(SecretsError::Open));
  assert_eq!(Secrets::new(&[9; 32]).open(7, &sealed), Err(SecretsError::Open));
  assert_ne!(secrets.seal(7, b"the password").expect("the secret seals again"), sealed);

  let text = base64_of(&KEY);

  assert!(Secrets::from_text(&format!("{text}\n"), "key").is_ok());
  assert!(matches!(Secrets::from_text("c2hvcnQ=", "key"), Err(SecretsError::Shape(_))));
}

fn base64_of(bytes: &[u8]) -> String {
  use base64::Engine as _;

  base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[tokio::test]
async fn the_routes_add_change_and_remove_an_owners_accounts() {
  let folder = scratch("routes");
  let (standing, address) = service(&folder, true).await;
  let anybody = MailClient::new(&address, TOKEN);
  let one = MailClient::new(&address, TOKEN).acting_for(OWNER);
  let kinds = anybody.account_kinds().await.expect("the kinds answer");

  assert_eq!(kinds.items.iter().map(|kind| kind.name.as_str()).collect::<Vec<_>>(), ["imap-smtp"]);
  assert_eq!(
    refused(anybody.accounts(&ListQuery::default()).await.unwrap_err()),
    (400, "bad-request".into())
  );

  let (settings, secrets) = unanswered();
  let adding = |kind: &str, name: Option<&str>| NewAccount {
    kind: kind.to_owned(),
    name: name.map(str::to_owned),
    address: Some("first@example.com".to_owned()),
    settings: Some(settings.clone()),
    secrets: Some(secrets.clone()),
    authorization: None,
  };

  assert_eq!(
    refused(one.create_account(&adding("gmail", None)).await.unwrap_err()),
    (422, "unknown-kind".into())
  );
  assert_eq!(
    refused(one.create_account(&adding("imap-smtp", None)).await.unwrap_err()),
    (422, "sign-in-failed".into())
  );

  let mut no_password = adding("imap-smtp", None);

  no_password.secrets = None;
  assert_eq!(
    refused(one.create_account(&no_password).await.unwrap_err()),
    (400, "bad-request".into())
  );

  keep(&standing, OWNER, "first").await;
  assert_eq!(
    refused(one.create_account(&adding("imap-smtp", None)).await.unwrap_err()),
    (409, "conflict".into())
  );

  let asked = |kind: &str| NewAuthorization {
    kind: kind.to_owned(),
    return_to: "https://console.example.com/mail".to_owned(),
  };

  assert_eq!(
    refused(one.create_authorization(&asked("imap-smtp")).await.unwrap_err()),
    (422, "no-authorization".into())
  );
  assert_eq!(
    refused(one.create_authorization(&asked("graph")).await.unwrap_err()),
    (422, "unknown-kind".into())
  );

  keep(&standing, OWNER, "second").await;

  let listed = one.accounts(&ListQuery::default()).await.expect("the accounts answer");

  assert_eq!(
    listed.items.iter().map(|one| (one.name.as_str(), one.position)).collect::<Vec<_>>(),
    [("first", 1), ("second", 2)]
  );
  assert!(listed.items.iter().all(|one| !one.reachable && one.kind == "imap-smtp"));
  assert!(listed.items.iter().all(|one| !one.settings.contains_key("password")));

  let renamed = one
    .change_account(
      "second",
      &AccountPatch {
        name: Some("Renamed".to_owned()),
        position: Some(1),
        ..AccountPatch::default()
      },
    )
    .await
    .expect("the account changes");

  assert_eq!((renamed.name.as_str(), renamed.position), ("renamed", 1));
  assert_eq!(
    refused(
      one
        .change_account(
          "renamed",
          &AccountPatch {
            name: Some("first".to_owned()),
            ..AccountPatch::default()
          }
        )
        .await
        .unwrap_err()
    ),
    (409, "conflict".into())
  );
  assert_eq!(
    refused(
      one
        .change_account(
          "renamed",
          &AccountPatch {
            secrets: Some(fields(&[("password", "another")])),
            ..AccountPatch::default()
          }
        )
        .await
        .unwrap_err()
    ),
    (422, "sign-in-failed".into())
  );

  one.delete_account("first").await.expect("the account goes");

  let left = one.accounts(&ListQuery::default()).await.expect("the accounts answer");

  assert_eq!(
    left.items.iter().map(|one| (one.name.as_str(), one.position)).collect::<Vec<_>>(),
    [("renamed", 1)]
  );

  let events = one.events(&EventQuery::default()).await.expect("the events answer");
  let kinds: Vec<&str> = events.items.iter().map(|event| event.kind.as_str()).collect();

  assert!(kinds.contains(&"account.changed") && kinds.contains(&"account.deleted"));
}

#[tokio::test]
async fn one_owner_key_never_reaches_anothers_accounts() {
  let folder = scratch("owners");
  let (standing, address) = service(&folder, true).await;
  let one = MailClient::new(&address, TOKEN).acting_for(OWNER);
  let other = MailClient::new(&address, TOKEN).acting_for(OTHER);

  keep(&standing, OWNER, "shared").await;
  keep(&standing, OTHER, "shared").await;
  keep(&standing, OWNER, "own").await;

  let names = |page: bmo_mail::wire::Page<bmo_mail::wire::Account>| {
    page.items.into_iter().map(|one| one.name).collect::<Vec<_>>()
  };

  assert_eq!(names(one.accounts(&ListQuery::default()).await.unwrap()), ["shared", "own"]);
  assert_eq!(names(other.accounts(&ListQuery::default()).await.unwrap()), ["shared"]);
  assert_eq!(other.summary().await.unwrap().accounts.len(), 1);

  let not_found = (404, "not-found".to_owned());

  assert_eq!(refused(other.account_of("own").await.unwrap_err()), not_found);
  assert_eq!(refused(other.folders("own", &ListQuery::default()).await.unwrap_err()), not_found);
  assert_eq!(refused(other.labels("own", &ListQuery::default()).await.unwrap_err()), not_found);
  assert_eq!(refused(other.settings("own").await.unwrap_err()), not_found);
  assert_eq!(
    refused(
      other
        .change_account(
          "own",
          &AccountPatch {
            name: Some("taken".to_owned()),
            ..AccountPatch::default()
          }
        )
        .await
        .unwrap_err()
    ),
    not_found
  );
  assert_eq!(refused(other.delete_account("own").await.unwrap_err()), not_found);

  other.delete_account("shared").await.expect("the other owner's own account goes");

  assert_eq!(names(one.accounts(&ListQuery::default()).await.unwrap()), ["shared", "own"]);
  assert!(
    other
      .events(&EventQuery::default())
      .await
      .unwrap()
      .items
      .iter()
      .all(|event| { event.kind != "account.deleted" || event.data["name"] == "shared" })
  );
  assert!(one.events(&EventQuery::default()).await.unwrap().items.is_empty());
}

#[tokio::test]
async fn a_service_without_a_key_keeps_no_account() {
  let folder = scratch("unsealed");
  let (_standing, address) = service(&folder, false).await;
  let one = MailClient::new(&address, TOKEN).acting_for(OWNER);
  let (settings, secrets) = unanswered();
  let adding = NewAccount {
    kind: "imap-smtp".to_owned(),
    name: None,
    address: Some("first@example.com".to_owned()),
    settings: Some(settings),
    secrets: Some(secrets),
    authorization: None,
  };

  assert!(one.accounts(&ListQuery::default()).await.unwrap().items.is_empty());
  assert_eq!(refused(one.create_account(&adding).await.unwrap_err()), (503, "not-ready".into()));
}

const ACCOUNTS_FILE: &str = "# the stand-in server
imap 127.0.0.1 993 imap.example.com
smtp 127.0.0.1 587 -
account first@example.com first-login first-password
account Second@example.com second-login second-password
";

#[tokio::test]
async fn the_accounts_file_is_imported_once_with_what_the_store_held() {
  let folder = scratch("import");
  let file = folder.join("mail-accounts");
  let key_file = folder.join("mail-secrets.key");

  std::fs::write(&file, ACCOUNTS_FILE).expect("the accounts file is written");
  std::fs::write(&key_file, base64_of(&KEY)).expect("the key file is written");

  let keys = |administrator: &str| {
    vec![
      ("BMO_MAIL_ACCOUNTS__FILE", file.display().to_string()),
      ("BMO_MAIL_ACCOUNTS__ADMINISTRATOR", administrator.to_owned()),
      ("BMO_MAIL_SECRETS__KEY_FILE", key_file.display().to_string()),
    ]
  };
  let read = settings(&folder, &keys("administrator"));
  let store = Store::open(&read.store.path).await.expect("the store opens");
  let pool = store.pool();

  for (table, columns, values) in [
    (
      "labels",
      "account, id, name, color, position, created_at",
      "?, 'l1', 'Work', '#112233', 1, 0",
    ),
    ("settings", "account, signature, receipt_default, show_images", "?, 'sig', 0, 0"),
    ("marks", "account, folder, uid_validity, last_uid", "?, 'INBOX', '5', 41"),
  ] {
    sqlx::query(&format!("INSERT INTO {table} ({columns}) VALUES ({values})"))
      .bind("second@example.com")
      .execute(pool)
      .await
      .expect("a row of the address stands");
  }

  sqlx::query("INSERT INTO events (kind, account, at, data) VALUES ('folder.changed', 'second', 0, '{}')")
    .execute(pool)
    .await
    .expect("an event of the account stands");

  let without_key = Standing::new(read.clone(), store.clone(), Wording::load(&read).unwrap(), None);

  assert!(matches!(
    import_accounts_file(pool, &without_key.accounts, &read).await.unwrap(),
    Imported::Waiting(_)
  ));

  let secrets = Secrets::read(read.secrets.key_file.as_deref().unwrap()).expect("the key file reads");
  let standing = Standing::new(read.clone(), store.clone(), Wording::load(&read).unwrap(), Some(secrets));

  assert_eq!(import_accounts_file(pool, &standing.accounts, &read).await.unwrap(), Imported::Now(2));

  let imported = standing.accounts.list("administrator").await.unwrap();
  let shapes: Vec<(&str, &str, i64)> = imported
    .iter()
    .map(|one| (one.name.as_str(), one.kind.as_str(), one.position))
    .collect();

  assert_eq!(shapes, [("first", "imap-smtp", 1), ("second", "imap-smtp", 2)]);
  assert_eq!(imported[1].address, "second@example.com");
  assert_eq!(imported[1].secrets.get("password").map(String::as_str), Some("second-password"));
  assert_eq!(imported[1].settings.get("login").map(String::as_str), Some("second-login"));
  assert_eq!(imported[1].settings.get("imapTlsName").map(String::as_str), Some("imap.example.com"));
  assert_eq!(imported[1].settings.get("smtpTlsName").map(String::as_str), Some("-"));

  let sealed: Vec<u8> = sqlx::query_scalar("SELECT secret FROM accounts WHERE name = 'second'")
    .fetch_one(pool)
    .await
    .unwrap();

  assert!(!sealed.windows(15).any(|window| window == b"second-password"));

  let second = imported[1].store_key();

  for table in ["labels", "settings", "marks"] {
    let moved: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table} WHERE account = ?"))
      .bind(&second)
      .fetch_one(pool)
      .await
      .unwrap();

    assert_eq!(moved, 1, "the {table} row stays the account's");
  }

  let owned: Option<String> = sqlx::query_scalar("SELECT owner FROM events WHERE account = 'second'")
    .fetch_one(pool)
    .await
    .unwrap();

  assert_eq!(owned.as_deref(), Some("administrator"));

  std::fs::remove_file(&file).expect("the accounts file goes");
  assert_eq!(import_accounts_file(pool, &standing.accounts, &read).await.unwrap(), Imported::Before);
  assert_eq!(standing.accounts.list("administrator").await.unwrap().len(), 2);

  let moved_on = settings(&folder, &keys("the-new-administrator"));

  assert_eq!(
    import_accounts_file(pool, &standing.accounts, &moved_on).await.unwrap(),
    Imported::Before
  );
  assert!(standing.accounts.list("administrator").await.unwrap().is_empty());
  assert_eq!(standing.accounts.list("the-new-administrator").await.unwrap().len(), 2);
}
