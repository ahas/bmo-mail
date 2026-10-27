//! The console plugin bundle (`docs/api.md`, 「The console plugin bundle」): the manifest read through bmo-ui's
//! own reading and agreeing with the declaration the pack carries, the guard of every route the screen uses,
//! and the module's and the stylesheet's names following their bytes.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use bmo_mail::client::{ClientFailure, MailClient};
use bmo_mail::http::{Standing, router};
use bmo_mail::settings::Settings;
use bmo_mail::store::Store;
use bmo_mail::wire::{ConsoleManifest, Role};
use bmo_mail::wording::Wording;
use serde_json::{Value, json};

const TOKEN: &str = "test-token";
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

/// A built bundle in the folder's `console/`: a module, a stylesheet and words.
fn built(folder: &Path, module: &str) {
  let console = folder.join("console");

  std::fs::create_dir_all(&console).expect("the console folder is made");
  std::fs::write(console.join("module.js"), module).expect("the module is written");
  std::fs::write(console.join("style.css"), ".mail-row{display:flex}").expect("the stylesheet is written");
  std::fs::write(console.join("words.json"), r#"{"ko":{"mail":{"title":{"screen":"Mail"}}}}"#)
    .expect("the words are written");
}

/// A service in `folder`, answering on a port of its own.
async fn service(folder: &Path) -> String {
  let environment = HashMap::from([("BMO_MAIL_SERVER__TOKEN".to_owned(), TOKEN.to_owned())]);
  let settings = Settings::read(folder, Some(environment)).expect("the settings read");
  let store = Store::open(&settings.store.path).await.expect("the store opens");
  let wording = Wording::load(&settings).expect("the words load");
  let standing = Arc::new(Standing::new(settings, store, wording, None));
  let app = router(standing).expect("the routes stand");
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("the service listens");
  let address = format!("http://{}", listener.local_addr().expect("the address is known"));

  tokio::spawn(async move { axum::serve(listener, app).await });

  address
}

/// The status a call with these headers is answered.
async fn status(address: &str, path: &str, headers: &[(&str, &str)]) -> u16 {
  let mut request = reqwest::Client::new().get(format!("{address}{path}"));

  for (name, value) in headers {
    request = request.header(*name, *value);
  }

  request.send().await.expect("the service answers").status().as_u16()
}

/// The client's project, where bmo-ui stands installed.
fn client_project() -> PathBuf {
  Path::new(env!("CARGO_MANIFEST_DIR")).join("client")
}

/// The manifest as bmo-ui's own `consoleManifest` reads it, run by Bun in the client's project: the reading
/// with its defaults filled, or the reading's refusal.
fn read_by_bmo_ui(manifest: &ConsoleManifest) -> Result<Value, String> {
  let script = r#"import { consoleManifest } from "bmo-ui/shared/console-manifest";
console.log(JSON.stringify(consoleManifest(JSON.parse(await Bun.stdin.text()))));"#;
  let mut bun = Command::new("bun")
    .args(["--eval", script])
    .current_dir(client_project())
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .expect("bun runs");

  bun
    .stdin
    .take()
    .expect("bun takes the manifest")
    .write_all(&serde_json::to_vec(manifest).expect("the manifest is written"))
    .expect("bun reads the manifest");

  let done = bun.wait_with_output().expect("bun ends");

  if !done.status.success() {
    return Err(String::from_utf8_lossy(&done.stderr).into_owned());
  }

  serde_json::from_slice(&done.stdout).map_err(|why| why.to_string())
}

#[tokio::test]
async fn the_manifest_keeps_the_contract_and_agrees_with_the_declaration() {
  let folder = scratch("manifest");

  built(&folder, "export default { routes: [] };\n");

  let address = service(&folder).await;
  let manifest = MailClient::new(&address, TOKEN).console_manifest().await.expect("the manifest answers");
  let read = read_by_bmo_ui(&manifest).expect("bmo-ui reads the manifest");

  assert_eq!(read["role"], "member");
  assert_eq!(read["ownerKey"], true);
  assert_eq!(read["events"], true);
  assert_eq!(read["apiPrefix"], "/");
  assert_eq!(read["pushKinds"], json!(["message.created"]));
  assert_eq!(read["sidebarUnread"], json!({ "path": "/summary", "field": "unreadCount" }));
  assert!(manifest.bundle.starts_with("/console-bundle/module.") && manifest.bundle.ends_with(".js"));
  assert!(manifest.stylesheet.as_deref().is_some_and(|path| path.ends_with(".css")));

  let declaration: Value = serde_json::from_str(
    &std::fs::read_to_string(client_project().join("console-bundle.json")).expect("the declaration reads"),
  )
  .expect("the declaration is JSON");

  for field in [
    "name",
    "title",
    "icon",
    "screenPath",
    "serviceName",
    "events",
    "websocket",
    "pushKinds",
    "sidebarUnread",
  ] {
    assert_eq!(read[field], declaration[field], "the manifest and the declaration differ in {field}");
  }

  let mut broken = manifest.clone();

  broken.title = "title.screen".to_owned();
  assert!(read_by_bmo_ui(&broken).is_err_and(|why| why.contains("title")));

  let words = MailClient::new(&address, TOKEN).console_words().await.expect("the words answer");

  assert_eq!(words["ko"]["mail"]["title"]["screen"], "Mail");
}

#[tokio::test]
async fn every_route_the_screen_uses_needs_the_role_and_the_owner_key() {
  let folder = scratch("guard");

  built(&folder, "export default { routes: [] };\n");

  let address = service(&folder).await;
  let manifest = MailClient::new(&address, TOKEN).console_manifest().await.expect("the manifest answers");
  let module = manifest.bundle.as_str();
  let bearer = format!("Bearer {TOKEN}");
  let token = ("authorization", bearer.as_str());
  let key = ("owner-key", OWNER);

  for path in ["/console-manifest", "/console-words", module, "/accounts", "/account-kinds"] {
    assert_eq!(status(&address, path, &[]).await, 401, "{path} without the token");
    assert_eq!(status(&address, path, &[("authorization", "Bearer wrong")]).await, 401, "{path}");
  }

  for path in ["/console-manifest", "/console-words", "/health"] {
    assert_eq!(status(&address, path, &[token]).await, 200, "{path} with the token alone");
  }

  for path in [module, "/accounts", "/account-kinds", "/summary", "/events"] {
    assert_eq!(status(&address, path, &[token]).await, 403, "{path} with no role");
    assert_eq!(status(&address, path, &[token, key]).await, 403, "{path} with no role");
    assert_eq!(status(&address, path, &[token, key, ("owner-role", "guest")]).await, 403, "{path}");
    assert_eq!(
      status(&address, path, &[token, ("owner-role", "member")]).await,
      403,
      "{path} with no key"
    );
    assert_eq!(status(&address, path, &[token, ("owner-key", ""), ("owner-role", "member")]).await, 403);
  }

  for role in ["member", "administrator"] {
    let reader = [token, key, ("owner-role", role)];

    assert_eq!(status(&address, module, &reader).await, 200, "the module to a {role}");
    assert_eq!(status(&address, "/accounts", &reader).await, 200, "the accounts to a {role}");
  }

  let reader = MailClient::new(&address, TOKEN).acting_for(OWNER, Role::Member);
  let stylesheet = manifest.stylesheet.as_deref().expect("the manifest names the stylesheet");

  assert_eq!(
    reader.console_file(stylesheet).await.expect("the stylesheet answers"),
    b".mail-row{display:flex}"
  );
}

#[tokio::test]
async fn the_module_is_named_by_its_bytes() {
  let first = scratch("bytes-first");
  let again = scratch("bytes-again");
  let changed = scratch("bytes-changed");

  built(&first, "export default { routes: [] };\n");
  built(&again, "export default { routes: [] };\n");
  built(&changed, "export default { routes: [{ path: \"/mail\" }] };\n");

  let manifest = |address: String| async move {
    MailClient::new(&address, TOKEN).console_manifest().await.expect("the manifest answers")
  };
  let first_address = service(&first).await;
  let changed_address = service(&changed).await;
  let first_manifest = manifest(first_address.clone()).await;
  let again_manifest = manifest(service(&again).await).await;
  let changed_manifest = manifest(changed_address.clone()).await;

  assert_eq!(first_manifest.bundle, again_manifest.bundle);
  assert_ne!(first_manifest.bundle, changed_manifest.bundle);
  assert_eq!(first_manifest.stylesheet, changed_manifest.stylesheet);

  let reader = |address: &str| MailClient::new(address, TOKEN).acting_for(OWNER, Role::Member);

  assert_eq!(
    reader(&changed_address)
      .console_file(&changed_manifest.bundle)
      .await
      .expect("the module answers"),
    b"export default { routes: [{ path: \"/mail\" }] };\n"
  );
  assert!(matches!(
    reader(&changed_address).console_file(&first_manifest.bundle).await,
    Err(ClientFailure::Answered { status: 404, .. })
  ));
}

#[tokio::test]
async fn a_service_without_a_built_bundle_offers_none() {
  let address = service(&scratch("none")).await;

  assert!(matches!(
    MailClient::new(&address, TOKEN).console_manifest().await,
    Err(ClientFailure::Answered { status: 404, .. })
  ));
}
