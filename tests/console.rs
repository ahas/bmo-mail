//! The console face (`docs/api.md`, 「The console plugin bundle」): the manifest the service serves read through
//! bmo-ui's own reading in the release the client vendors, how far the console's credential and the service's
//! token reach, and a service serving no face without a credential or refusing to start on a broken build. What
//! the face answers and refuses of itself is bmo-chat's `bmo-console-face` crate's own tests'.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bmo_console_face::read_manifest;
use bmo_mail::http::{Standing, router};
use bmo_mail::settings::Settings;
use bmo_mail::store::Store;
use bmo_mail::wording::Wording;
use serde_json::{Value, json};

const TOKEN: &str = "test-token";
const CONSOLE: &str = "test-console-credential";
const OWNER: &str = "owner-one";

/// Chooses the one crypto provider every TLS client of the service runs on, as the binary does at start.
fn provider() {
  let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
}

fn scratch(name: &str) -> PathBuf {
  provider();

  let folder = std::env::temp_dir().join(format!("bmo-mail-console-{}-{name}", std::process::id()));
  let _ = std::fs::remove_dir_all(&folder);
  std::fs::create_dir_all(&folder).expect("the scratch folder is made");

  folder
}

/// The bmo-ui release the client vendors, the one packed file under `client/vendor/` named for bmo-ui.
fn vendored_release() -> PathBuf {
  let vendor = Path::new(env!("CARGO_MANIFEST_DIR")).join("client").join("vendor");
  let mut releases: Vec<PathBuf> = std::fs::read_dir(&vendor)
    .expect("the vendor folder reads")
    .filter_map(|entry| entry.ok().map(|entry| entry.path()))
    .filter(|path| {
      path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("bmo-ui-"))
    })
    .collect();

  assert_eq!(releases.len(), 1, "one bmo-ui release stands vendored: {releases:?}");

  releases.remove(0)
}

/// The layer the vendored release names, as its bundle build writes it.
fn vendored_layer() -> String {
  let release = vendored_release();
  let name = release.file_name().and_then(|name| name.to_str()).expect("the release is named");

  name.trim_start_matches("bmo-ui-").trim_end_matches(".tgz").to_owned()
}

/// A bundle build in the folder's `console/`: a module, a stylesheet, words and the layer it was built on.
fn built(folder: &Path, layer: Option<&str>) {
  let console = folder.join("console");

  std::fs::create_dir_all(&console).expect("the console folder is made");
  std::fs::write(console.join("module.js"), "export default { routes: [] };\n")
    .expect("the module is written");
  std::fs::write(console.join("style.css"), ".mail-row{display:flex}").expect("the stylesheet is written");
  std::fs::write(console.join("words.json"), r#"{"ko":{"mail":{"title":{"screen":"Mail"}}}}"#)
    .expect("the words are written");

  if let Some(layer) = layer {
    std::fs::write(console.join("layer.json"), json!({ "layer": layer }).to_string())
      .expect("the layer is written");
  }
}

/// The routes of a service in `folder`, its settings naming the console's credential given, or what stops it.
async fn routes(folder: &Path, console: &str) -> Result<axum::Router, Vec<String>> {
  let environment = HashMap::from([
    ("BMO_MAIL_SERVER__TOKEN".to_owned(), TOKEN.to_owned()),
    ("BMO_MAIL_CONSOLE__TOKEN".to_owned(), console.to_owned()),
  ]);
  let settings = Settings::read(folder, Some(environment)).expect("the settings read");
  let store = Store::open(&settings.store.path).await.expect("the store opens");
  let wording = Wording::load(&settings).expect("the words load");

  router(Arc::new(Standing::new(settings, store, wording, None)))
}

/// A service in `folder`, answering on a port of its own.
async fn service(folder: &Path, console: &str) -> String {
  let app = routes(folder, console).await.expect("the routes stand");
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("the service listens");
  let address = format!("http://{}", listener.local_addr().expect("the address is known"));

  tokio::spawn(async move { axum::serve(listener, app).await });

  address
}

/// A call with these headers: its status and its body.
async fn call(address: &str, path: &str, headers: &[(&str, &str)]) -> (u16, Vec<u8>) {
  let mut request = reqwest::Client::new().get(format!("{address}{path}"));

  for (name, value) in headers {
    request = request.header(*name, *value);
  }

  let answer = request.send().await.expect("the service answers");
  let status = answer.status().as_u16();

  (status, answer.bytes().await.expect("the body reads").to_vec())
}

#[tokio::test]
async fn the_served_manifest_reads_through_the_vendored_release() {
  let folder = scratch("manifest");
  let layer = vendored_layer();

  built(&folder, Some(&layer));

  let address = service(&folder, CONSOLE).await;
  let bearer = format!("Bearer {CONSOLE}");
  let (status, body) = call(&address, "/console-manifest", &[("authorization", &bearer)]).await;

  assert_eq!(status, 200);

  let manifest: Value = serde_json::from_slice(&body).expect("the manifest is JSON");
  let read = read_manifest(&manifest, &vendored_release()).expect("bmo-ui's release reads the manifest");

  assert_eq!(read["layer"], layer);
  assert_eq!(read["name"], "mail");
  assert_eq!(read["role"], "member");
  assert_eq!(read["pushKinds"], json!(["message.created"]));
  assert_eq!(read["sidebarUnread"], json!({ "path": "/summary", "field": "unreadCount" }));
}

#[tokio::test]
async fn the_console_credential_reaches_the_screen_routes_alone() {
  let folder = scratch("credential");

  built(&folder, Some(&vendored_layer()));

  let address = service(&folder, CONSOLE).await;
  let console = format!("Bearer {CONSOLE}");
  let service_token = format!("Bearer {TOKEN}");
  let reader = [("owner-key", OWNER), ("owner-role", "member")];

  for bearer in [&console, &service_token] {
    let headers = [("authorization", bearer.as_str()), reader[0], reader[1]];

    assert_eq!(call(&address, "/accounts", &headers).await.0, 200, "the accounts to {bearer}");
    assert_eq!(
      call(&address, "/accounts", &headers[..1]).await.0,
      403,
      "the accounts to {bearer} with no reader"
    );
  }

  assert_eq!(call(&address, "/health", &[("authorization", &console)]).await.0, 401);
  assert_eq!(call(&address, "/health", &[("authorization", &service_token)]).await.0, 200);
  assert_eq!(call(&address, "/console-manifest", &[("authorization", &service_token)]).await.0, 401);
}

#[tokio::test]
async fn a_service_with_no_console_credential_serves_no_face() {
  let folder = scratch("no-credential");

  built(&folder, Some(&vendored_layer()));

  let address = service(&folder, "").await;

  assert_eq!(call(&address, "/console-manifest", &[("authorization", "Bearer ")]).await.0, 404);
  assert_eq!(
    call(&address, "/console-manifest", &[("authorization", &format!("Bearer {TOKEN}"))])
      .await
      .0,
    404
  );
}

#[tokio::test]
async fn a_build_the_face_refuses_stops_the_service() {
  let folder = scratch("no-layer");

  built(&folder, None);

  let faults = routes(&folder, CONSOLE).await.expect_err("the routes are refused");

  assert!(faults.iter().any(|fault| fault.contains("console face")), "{faults:?}");
}

#[test]
fn the_reach_names_the_address_and_both_credentials() {
  let environment = HashMap::from([
    ("BMO_MAIL_SERVER__LISTEN".to_owned(), "127.0.0.1:3971".to_owned()),
    ("BMO_MAIL_SERVER__TOKEN".to_owned(), TOKEN.to_owned()),
    ("BMO_MAIL_CONSOLE__TOKEN".to_owned(), CONSOLE.to_owned()),
  ]);
  let settings = Settings::read(&scratch("reach"), Some(environment)).expect("the settings read");

  assert_eq!(
    bmo_mail::console::reach(&settings),
    json!({ "address": "http://127.0.0.1:3971", "consoleToken": CONSOLE, "token": TOKEN })
  );
}
