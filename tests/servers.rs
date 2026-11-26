//! The mail servers (`docs/mail.md`, 「Mail servers」, 「The platform's system mail」; `docs/api.md`, 「Servers」,
//! 「System mail」): the servers an owner reaches, their organisation's first, as a stand-in account service's
//! delegation chain offers them, an account choosing among them; and the account service's system send.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, put};
use bmo_mail::accounts::Keeping;
use bmo_mail::client::{ClientFailure, MailClient};
use bmo_mail::http::{Standing, router};
use bmo_mail::secrets::Secrets;
use bmo_mail::settings::Settings;
use bmo_mail::store::Store;
use bmo_mail::wire::{
  Direction, Fields, NewAccount, NewServer, NewServerTest, NewSystemMail, Protocol, Role, Security,
  ServerAuth, ServerPatch, ServerQuery,
};
use bmo_mail::wording::Wording;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

const TOKEN: &str = "test-token";
const SYSTEM_TOKEN: &str = "system-mail-token";
/// The head of the company, who writes its link; a person of its unit, who writes nothing.
const HEAD: &str = "7";
const MEMBER: &str = "8";

fn provider() {
  let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
}

fn scratch(name: &str) -> PathBuf {
  provider();

  let folder = std::env::temp_dir().join(format!("bmo-mail-servers-{}-{name}", std::process::id()));
  let _ = std::fs::remove_dir_all(&folder);
  std::fs::create_dir_all(&folder).expect("the scratch folder is made");

  folder
}

fn settings(folder: &Path, keys: &[(&str, String)]) -> Settings {
  let mut environment = HashMap::from([("BMO_MAIL_SERVER__TOKEN".to_owned(), TOKEN.to_owned())]);

  environment.extend(keys.iter().map(|(key, value)| ((*key).to_owned(), value.clone())));

  Settings::read(folder, Some(environment)).expect("the settings read")
}

fn closed_port() -> u16 {
  let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port is taken");

  listener.local_addr().expect("the port is known").port()
}

fn refused(failure: ClientFailure) -> (u16, String) {
  match failure {
    ClientFailure::Answered { status, answer, .. } => {
      (status, answer.map(|one| one.code).unwrap_or_default())
    }
    other => panic!("the call was not answered: {other}"),
  }
}

/// The sentence a refusal came back with, for a check on what it names.
fn reason(failure: &ClientFailure) -> String {
  match failure {
    ClientFailure::Answered {
      answer: Some(answer),
      ..
    } => answer.error.clone(),
    other => panic!("the call was not answered: {other}"),
  }
}

/// A plain server of a protocol on this machine, held by `link` where one is given.
fn plain(protocol: Protocol, port: u16, link: Option<&str>) -> NewServer {
  NewServer {
    link: link.map(str::to_owned),
    protocol,
    name: None,
    host: "127.0.0.1".to_owned(),
    port: Some(port),
    security: Some(Security::None),
    tls_name: None,
    auth: None,
  }
}

/// The service answering on a port of its own, and its standing.
async fn service(folder: &Path, keys: &[(&str, String)]) -> (Arc<Standing>, String) {
  let settings = settings(folder, keys);
  let store = Store::open(&settings.store.path).await.expect("the store opens");
  let wording = Wording::load(&settings).expect("the words load");
  let standing = Arc::new(Standing::new(settings, store, wording, Some(Secrets::new(&[7; 32]))));
  let app = router(Arc::clone(&standing)).expect("the routes stand");
  let listener = TcpListener::bind("127.0.0.1:0").await.expect("the service listens");
  let address = format!("http://{}", listener.local_addr().expect("the address is known"));

  tokio::spawn(async move { axum::serve(listener, app).await });

  (standing, address)
}

/// What the stand-in account service holds: its chain's path for every person, the link a lock cuts it at, and
/// every value written.
#[derive(Default)]
struct Chain {
  locked_by: Option<String>,
  written: Vec<(String, String, Value)>,
}

type Shared = Arc<Mutex<Chain>>;

fn owner_of(headers: &HeaderMap) -> String {
  headers
    .get("owner-key")
    .and_then(|value| value.to_str().ok())
    .unwrap_or_default()
    .to_owned()
}

fn forbidden() -> Response {
  (
    StatusCode::FORBIDDEN,
    Json(json!({ "code": "forbidden", "error": "not a writer of the link" })),
  )
    .into_response()
}

/// The account service as the servers ask it: one company, `unit:1`, with a unit under it, `unit:2`; the head
/// writes the company's link, the administrator, `0`, the platform's, and the member nothing.
async fn account_service(chain: Shared) -> String {
  async fn declare() -> Json<Value> {
    Json(json!({}))
  }

  async fn resolve(State(chain): State<Shared>, Query(query): Query<HashMap<String, String>>) -> Response {
    let person = query.get("person").cloned().unwrap_or_default();

    if person == "stranger" {
      return (StatusCode::NOT_FOUND, Json(json!({ "code": "not-found", "error": "no such person" })))
        .into_response();
    }

    let locked_by = chain.lock().unwrap().locked_by.clone();

    Json(json!({ "path": ["platform", "unit:1", "unit:2", format!("person:{person}")], "lockedBy": locked_by }))
      .into_response()
  }

  async fn unit_of() -> Json<Value> {
    Json(json!({ "path": [{ "id": 1, "name": "Company" }, { "id": 2, "name": "Team" }] }))
  }

  fn writes(link: &str, owner: &str) -> bool {
    (link == "platform" && owner == "0") || (link.starts_with("unit:") && (owner == HEAD || owner == "0"))
  }

  async fn entry(UrlPath(link): UrlPath<String>, headers: HeaderMap) -> Response {
    let owner = owner_of(&headers);

    if writes(&link, &owner) {
      Json(json!({ "writable": true })).into_response()
    } else {
      forbidden()
    }
  }

  async fn value(
    State(chain): State<Shared>,
    UrlPath(link): UrlPath<String>,
    headers: HeaderMap,
    Json(body): Json<Value>,
  ) -> Response {
    let owner = owner_of(&headers);

    if !writes(&link, &owner) || headers.get("declared-by").is_none_or(|by| by != "mail") {
      return forbidden();
    }

    chain.lock().unwrap().written.push((link.clone(), owner, body["value"].clone()));

    Json(json!({ "link": link, "writable": true })).into_response()
  }

  let app = Router::new()
    .route("/chain/kinds/mail.servers", put(declare))
    .route("/chain/kinds/mail.servers/resolve", get(resolve))
    .route("/chain/kinds/mail.servers/links/{link}", get(entry))
    .route("/chain/kinds/mail.servers/links/{link}/value", put(value))
    .route("/users/{user}/unit", get(unit_of))
    .with_state(chain);
  let listener = TcpListener::bind("127.0.0.1:0").await.expect("the stand-in listens");
  let address = format!("http://{}", listener.local_addr().expect("the address is known"));

  tokio::spawn(async move { axum::serve(listener, app).await });

  address
}

#[tokio::test]
async fn an_owner_reaches_their_organisations_servers_first_and_an_account_chooses_among_them() {
  let folder = scratch("reach");
  let chain: Shared = Arc::default();
  let accounts_address = account_service(Arc::clone(&chain)).await;
  let (standing, address) = service(
    &folder,
    &[
      ("BMO_MAIL_ACCOUNT_SERVICE__ADDRESS", accounts_address),
      ("BMO_MAIL_ACCOUNT_SERVICE__TOKEN", "account-token".to_owned()),
    ],
  )
  .await;
  let as_who = |owner: &str| MailClient::new(&address, TOKEN).acting_for(owner, Role::Member);
  let (admin, head, member) = (as_who("0"), as_who(HEAD), as_who(MEMBER));
  let port = closed_port();

  let company = head
    .create_server(&plain(Protocol::Imap, port, Some("unit:1")))
    .await
    .expect("the head keeps one");
  let team_smtp = head.create_server(&plain(Protocol::Smtp, port, Some("unit:2"))).await.unwrap();
  let platform = admin.create_server(&plain(Protocol::Smtp, port, Some("platform"))).await.unwrap();
  let own = member
    .create_server(&plain(Protocol::Pop3, port, None))
    .await
    .expect("anyone keeps their own");

  assert_eq!(
    chain
      .lock()
      .unwrap()
      .written
      .iter()
      .map(|(link, _, value)| (link.as_str(), value.clone()))
      .collect::<Vec<_>>(),
    [
      ("unit:1", json!([company.id])),
      ("unit:2", json!([team_smtp.id])),
      ("platform", json!([platform.id]))
    ],
    "a link's list is written on the chain in the name of the one changing it"
  );
  assert_eq!(
    refused(member.create_server(&plain(Protocol::Imap, port, Some("unit:1"))).await.unwrap_err()),
    (403, "forbidden".into())
  );
  assert!(
    standing.servers.held_by(&["unit:1".to_owned()], None).await.unwrap().len() == 1,
    "a refused write keeps nothing"
  );

  let reached = member.servers(&ServerQuery::default()).await.expect("the servers answer");
  let order: Vec<(i64, &str, Option<&str>, bool)> = reached
    .items
    .iter()
    .map(|one| (one.id, one.holder.link.as_str(), one.holder.name.as_deref(), one.may_change))
    .collect();

  assert_eq!(
    order,
    [
      (company.id, "unit:1", Some("Company"), false),
      (team_smtp.id, "unit:2", Some("Team"), false),
      (platform.id, "platform", None, false),
      (own.id, "own", None, true),
    ]
  );

  let outgoing = member
    .servers(&ServerQuery {
      direction: Some(Direction::Outgoing),
      link: None,
    })
    .await
    .unwrap();

  assert_eq!(outgoing.items.iter().map(|one| one.id).collect::<Vec<_>>(), [team_smtp.id, platform.id]);
  assert!(
    head
      .servers(&ServerQuery {
        direction: None,
        link: Some("unit:1".to_owned())
      })
      .await
      .unwrap()
      .items[0]
      .may_change
  );
  assert_eq!(
    refused(
      member
        .servers(&ServerQuery {
          direction: None,
          link: Some("unit:1".to_owned())
        })
        .await
        .unwrap_err()
    ),
    (403, "forbidden".into())
  );

  let adding = |incoming: i64, outgoing: i64| NewAccount {
    kind: None,
    name: None,
    address: Some("person@example.com".to_owned()),
    incoming_server: Some(incoming),
    outgoing_server: Some(outgoing),
    settings: None,
    secrets: Some(Fields::from([("password".to_owned(), "secret".to_owned())])),
    authorization: None,
  };
  let stranger_own =
    as_who("someone-else").create_server(&plain(Protocol::Imap, port, None)).await.unwrap();

  assert_eq!(
    refused(member.create_account(&adding(company.id, team_smtp.id)).await.unwrap_err()),
    (422, "sign-in-failed".into()),
    "an organisation's server is chosen, and the sign-in is what fails at a closed port"
  );
  assert_eq!(
    refused(member.create_account(&adding(stranger_own.id, team_smtp.id)).await.unwrap_err()),
    (422, "unknown-server".into())
  );
  assert_eq!(
    refused(member.create_account(&adding(team_smtp.id, platform.id)).await.unwrap_err()),
    (422, "server-mismatch".into())
  );

  chain.lock().unwrap().locked_by = Some("unit:1".to_owned());

  let under_lock = member.servers(&ServerQuery::default()).await.unwrap();

  assert_eq!(
    under_lock.items.iter().map(|one| one.id).collect::<Vec<_>>(),
    [company.id, platform.id, own.id],
    "a unit below the lock offers nothing while it stands"
  );
  chain.lock().unwrap().locked_by = None;

  let stranger = as_who("stranger").servers(&ServerQuery::default()).await.unwrap();

  assert_eq!(stranger.items.iter().map(|one| one.id).collect::<Vec<_>>(), [platform.id]);

  standing
    .accounts
    .add(
      MEMBER,
      &Keeping {
        name: "kept".to_owned(),
        kind: "pop3-smtp".to_owned(),
        address: "kept@example.com".to_owned(),
        settings: Fields::new(),
        secrets: Fields::from([("password".to_owned(), "secret".to_owned())]),
        incoming_server: Some(own.id),
        outgoing_server: Some(platform.id),
      },
    )
    .await
    .expect("the account is kept");

  assert_eq!(member.server(own.id).await.unwrap().account_count, 1);
  assert_eq!(refused(member.delete_server(own.id).await.unwrap_err()), (409, "server-in-use".into()));
  assert_eq!(
    refused(as_who("someone-else").server(own.id).await.unwrap_err()),
    (404, "not-found".into())
  );

  let renamed = member
    .change_server(
      own.id,
      &ServerPatch {
        name: Some("My box".to_owned()),
        ..ServerPatch::default()
      },
    )
    .await
    .expect("an owner changes their own");

  assert_eq!((renamed.name.as_str(), renamed.protocol), ("My box", Protocol::Pop3));
  assert_eq!(
    standing
      .accounts
      .find(MEMBER, "kept")
      .await
      .unwrap()
      .unwrap()
      .incoming
      .unwrap()
      .draft
      .name,
    "My box",
    "a change of a server reaches the account on it"
  );
  assert_eq!(
    refused(member.change_server(platform.id, &ServerPatch::default()).await.unwrap_err()),
    (403, "forbidden".into())
  );

  head.delete_server(team_smtp.id).await.expect("a server no account stands on goes");
  assert_eq!(chain.lock().unwrap().written.last().unwrap().2, json!([]));

  let tested = member
    .test_server(&NewServerTest {
      protocol: Some(Protocol::Imap),
      host: Some("127.0.0.1".to_owned()),
      port: Some(port),
      security: Some(Security::None),
      ..NewServerTest::default()
    })
    .await
    .unwrap();

  assert_eq!(
    (tested.reached, tested.signed_in, tested.failure.as_deref()),
    (false, None, Some("unreachable"))
  );
}

#[tokio::test]
async fn without_an_account_service_only_an_owners_own_servers_stand() {
  let folder = scratch("alone");
  let (_standing, address) = service(&folder, &[]).await;
  let one = MailClient::new(&address, TOKEN).acting_for("one", Role::Member);
  let own = one.create_server(&plain(Protocol::Imap, closed_port(), None)).await.unwrap();

  assert_eq!(
    refused(
      one
        .create_server(&plain(Protocol::Imap, closed_port(), Some("platform")))
        .await
        .unwrap_err()
    ),
    (503, "not-ready".into())
  );
  assert_eq!(
    one
      .servers(&ServerQuery::default())
      .await
      .unwrap()
      .items
      .iter()
      .map(|s| s.id)
      .collect::<Vec<_>>(),
    [own.id]
  );
  assert_eq!(
    refused(
      one
        .create_server(&NewServer {
          security: Some(Security::Starttls),
          ..plain(Protocol::Pop3, 110, None)
        })
        .await
        .unwrap_err()
    ),
    (400, "bad-request".into()),
    "a POP3 server upgrades no session"
  );
}

/// A submission stand-in that takes `AUTH PLAIN` with one of `logins` alone and keeps what it is handed: each
/// message's recipients, its bytes and the login it went under.
async fn submission(logins: &[&str], kept: Arc<Mutex<Vec<(String, String)>>>) -> u16 {
  let taken: Arc<Vec<String>> = Arc::new(logins.iter().map(|login| (*login).to_owned()).collect());

  async fn serve(stream: TcpStream, kept: Arc<Mutex<Vec<(String, String)>>>, taken: Arc<Vec<String>>) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let mut login = String::new();
    let _ = reader.get_mut().write_all(b"220 stand-in\r\n").await;

    loop {
      line.clear();

      if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
        return;
      }

      let upper = line.to_ascii_uppercase();
      let answer = if upper.starts_with("EHLO") {
        "250-stand-in\r\n250 AUTH PLAIN\r\n"
      } else if upper.starts_with("AUTH PLAIN") {
        use base64::Engine as _;

        let given = line.split_whitespace().nth(2).unwrap_or_default().to_owned();
        let decoded = base64::engine::general_purpose::STANDARD.decode(given).unwrap_or_default();

        login = String::from_utf8_lossy(&decoded).replace('\0', " ").trim().to_owned();

        if taken.contains(&login) { "235 ok\r\n" } else { "535 no\r\n" }
      } else if upper.starts_with("DATA") {
        let _ = reader.get_mut().write_all(b"354 go on\r\n").await;
        let mut data = Vec::new();

        while !data.ends_with(b"\r\n.\r\n") {
          let mut byte = [0u8; 1];

          if reader.read_exact(&mut byte).await.is_err() {
            return;
          }

          data.push(byte[0]);
        }

        kept.lock().unwrap().push((login.clone(), String::from_utf8_lossy(&data).into_owned()));
        "250 taken\r\n"
      } else if upper.starts_with("QUIT") {
        let _ = reader.get_mut().write_all(b"221 bye\r\n").await;
        return;
      } else {
        "250 ok\r\n"
      };

      if reader.get_mut().write_all(answer.as_bytes()).await.is_err() {
        return;
      }
    }
  }

  let listener = TcpListener::bind("127.0.0.1:0").await.expect("the stand-in listens");
  let port = listener.local_addr().expect("the port is known").port();

  tokio::spawn(async move {
    while let Ok((stream, _)) = listener.accept().await {
      tokio::spawn(serve(stream, Arc::clone(&kept), Arc::clone(&taken)));
    }
  });

  port
}

#[tokio::test]
async fn the_account_service_sends_system_mail_through_a_platform_server_with_a_named_secret() {
  let folder = scratch("system");
  let secrets = folder.join("env");

  std::fs::create_dir_all(&secrets).unwrap();
  std::fs::write(secrets.join("system-mailer"), "USERNAME=mailer\nPASSWORD=the-password\n").unwrap();
  std::fs::write(secrets.join("wrong-mailer"), "USERNAME=mailer\nPASSWORD=not-it\n").unwrap();

  let chain: Shared = Arc::default();
  let accounts_address = account_service(chain).await;
  let (_standing, address) = service(
    &folder,
    &[
      ("BMO_MAIL_ACCOUNT_SERVICE__ADDRESS", accounts_address),
      ("BMO_MAIL_ACCOUNT_SERVICE__TOKEN", "account-token".to_owned()),
      ("BMO_MAIL_SYSTEM_MAIL__TOKEN", SYSTEM_TOKEN.to_owned()),
      ("BMO_MAIL_SYSTEM_MAIL__SECRETS_DIR", secrets.display().to_string()),
    ],
  )
  .await;
  let kept = Arc::new(Mutex::new(Vec::new()));
  let port = submission(&["mailer the-password"], Arc::clone(&kept)).await;
  let admin = MailClient::new(&address, TOKEN).acting_for("0", Role::Administrator);
  let platform = admin.create_server(&plain(Protocol::Smtp, port, Some("platform"))).await.unwrap();
  let own = admin.create_server(&plain(Protocol::Smtp, port, None)).await.unwrap();
  let system = MailClient::new(&address, SYSTEM_TOKEN);

  assert_eq!(
    refused(MailClient::new(&address, "wrong").outgoing_servers().await.unwrap_err()),
    (401, "unauthorized".into())
  );
  assert_eq!(
    refused(
      MailClient::new(&address, SYSTEM_TOKEN)
        .acting_for("0", Role::Member)
        .servers(&ServerQuery::default())
        .await
        .unwrap_err()
    ),
    (401, "unauthorized".into()),
    "the system mail's credential reaches its own routes alone"
  );

  let offered = system.outgoing_servers().await.expect("the platform's outgoing servers answer");

  assert_eq!(
    offered.items.iter().map(|one| (one.id, one.auth)).collect::<Vec<_>>(),
    [(platform.id, ServerAuth::Password)]
  );

  let mail = |server: i64, secret: Option<&str>| NewSystemMail {
    server,
    from: "noreply@example.com".to_owned(),
    from_name: Some("The platform".to_owned()),
    secret: secret.map(str::to_owned),
    to: "person@example.com".to_owned(),
    subject: "Reset your password".to_owned(),
    text: "Open the link".to_owned(),
    html: Some("<p>Open the link</p>".to_owned()),
    kind: "password-reset".to_owned(),
  };
  let sent = system
    .send_system_mail(&mail(platform.id, Some("system-mailer")))
    .await
    .expect("the mail goes");

  assert!(sent.sent && sent.message_id.ends_with("@example.com"));

  {
    let kept = kept.lock().unwrap();
    let (login, message) = kept.last().expect("the stand-in took the mail");

    assert_eq!(login, "mailer the-password");
    assert!(message.contains("Subject: Reset your password") && message.contains("person@example.com"));
    assert!(message.contains("text/html") && message.contains("text/plain"));
  }

  assert_eq!(
    refused(system.send_system_mail(&mail(own.id, Some("system-mailer"))).await.unwrap_err()),
    (422, "unknown-server".into()),
    "an owner's own server is none of the platform's"
  );
  let unit = admin.create_server(&plain(Protocol::Smtp, port, Some("unit:1"))).await.unwrap();
  let incoming = admin.create_server(&plain(Protocol::Imap, port, Some("platform"))).await.unwrap();

  assert_eq!(
    refused(system.send_system_mail(&mail(unit.id, Some("system-mailer"))).await.unwrap_err()),
    (422, "unknown-server".into()),
    "a company's or a unit's server is none of the platform's"
  );
  assert_eq!(
    refused(system.send_system_mail(&mail(incoming.id, Some("system-mailer"))).await.unwrap_err()),
    (422, "unknown-server".into()),
    "the platform's incoming server sends nothing"
  );
  assert_eq!(
    system
      .outgoing_servers()
      .await
      .unwrap()
      .items
      .iter()
      .map(|one| one.id)
      .collect::<Vec<_>>(),
    [platform.id],
    "the choice offers the platform's outgoing servers alone"
  );
  assert_eq!(kept.lock().unwrap().len(), 1, "nothing refused reached a server");
  assert_eq!(
    refused(system.send_system_mail(&mail(platform.id, Some("../env"))).await.unwrap_err()),
    (422, "unknown-secret".into())
  );
  assert_eq!(
    refused(system.send_system_mail(&mail(platform.id, None)).await.unwrap_err()),
    (422, "unknown-secret".into()),
    "a server that signs in takes no send without a secret"
  );
  assert_eq!(
    refused(system.send_system_mail(&mail(platform.id, Some("wrong-mailer"))).await.unwrap_err()),
    (422, "sign-in-failed".into())
  );
}

#[tokio::test]
async fn a_system_mail_signs_in_with_the_secret_at_a_unit_address() {
  let folder = scratch("unit-address");
  let secrets = folder.join("env");

  std::fs::create_dir_all(secrets.join("units/7")).unwrap();
  std::fs::write(secrets.join("platform-mailer"), "USERNAME=mailer\nPASSWORD=the-password\n").unwrap();
  std::fs::write(secrets.join("units/7/login"), "USERNAME=unit-mailer\nPASSWORD=unit-password\n").unwrap();

  let chain: Shared = Arc::default();
  let accounts_address = account_service(chain).await;
  let (_standing, address) = service(
    &folder,
    &[
      ("BMO_MAIL_ACCOUNT_SERVICE__ADDRESS", accounts_address),
      ("BMO_MAIL_ACCOUNT_SERVICE__TOKEN", "account-token".to_owned()),
      ("BMO_MAIL_SYSTEM_MAIL__TOKEN", SYSTEM_TOKEN.to_owned()),
      ("BMO_MAIL_SYSTEM_MAIL__SECRETS_DIR", secrets.display().to_string()),
    ],
  )
  .await;
  let kept = Arc::new(Mutex::new(Vec::new()));
  let port = submission(&["mailer the-password", "unit-mailer unit-password"], Arc::clone(&kept)).await;
  let admin = MailClient::new(&address, TOKEN).acting_for("0", Role::Administrator);
  let platform = admin.create_server(&plain(Protocol::Smtp, port, Some("platform"))).await.unwrap();
  let system = MailClient::new(&address, SYSTEM_TOKEN);
  let last_login = || kept.lock().unwrap().last().expect("the stand-in took a mail").0.clone();

  let mail = |named: &str| NewSystemMail {
    server: platform.id,
    from: "unit-noreply@example.com".to_owned(),
    from_name: None,
    secret: Some(named.to_owned()),
    to: "person@example.com".to_owned(),
    subject: "A notice of the unit".to_owned(),
    text: "The unit's notice".to_owned(),
    html: None,
    kind: "test".to_owned(),
  };

  system
    .send_system_mail(&mail("units/7/login"))
    .await
    .expect("the unit's own secret signs the send in");

  assert_eq!(
    last_login(),
    "unit-mailer unit-password",
    "the unit's file was read, not the platform's"
  );

  // A flat name keeps standing for the platform's own secret beside the unit's.
  system
    .send_system_mail(&mail("platform-mailer"))
    .await
    .expect("the platform's secret still reads");

  assert_eq!(last_login(), "mailer the-password");

  // An address whose unit or file does not stand is refused naming the whole address.
  for named in ["units/8/login", "units/7/absent"] {
    let failure = system.send_system_mail(&mail(named)).await.unwrap_err();
    let why = reason(&failure);

    assert_eq!(refused(failure), (422, "unknown-secret".into()), "{named} does not stand");
    assert!(why.contains(named), "the refusal names the address: {why}");
  }

  // A space the service does not read, and an address reaching outside the store's folder, are refused alike.
  for named in ["users/7/login", "units/7/../platform-mailer", "units/7", "units//login"] {
    let failure = system.send_system_mail(&mail(named)).await.unwrap_err();
    let why = reason(&failure);

    assert_eq!(
      refused(failure),
      (422, "unknown-secret".into()),
      "{named} is no address the store gives"
    );
    assert!(why.contains(named), "the refusal names the address: {why}");
  }

  assert_eq!(kept.lock().unwrap().len(), 2, "nothing refused reached a server");
}
