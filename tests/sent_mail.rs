//! The hosted tool (`docs/mail.md`, 「A person's sent mail, read on their own machine」), called as the tool host
//! calls it, against stand-ins of agency, the node service, bmo-ms-teams and Microsoft Graph served here: whose
//! person and which machine, the Outlook road and the Graph road in their order, what each upstream is asked
//! with which token, and the refusals. The schemas the tool's folder keeps are the types'.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::{Path as Segment, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use bmo_mail::sent_mail::call::answer;
use bmo_mail::sent_mail::graph::SCOPES;
use bmo_mail::sent_mail::outlook::{FROM_VARIABLE, SHELL, TO_VARIABLE};
use bmo_mail::sent_mail::schemas;
use chrono::{DateTime, Utc};
use flate2::Compression;
use flate2::write::GzEncoder;
use serde_json::{Value, json};

const AGENCY_TOKEN: &str = "agency-test-token";
const RECEIVER_TOKEN: &str = "receiver-test-token";
const ACCOUNTS_TOKEN: &str = "accounts-test-token";
const NODE_TOKEN: &str = "node-caller-test-token";
const TEAMS_TOKEN: &str = "teams-caller-test-token";
const PAGE_TOKEN: &str = "page-test-token";

/// The first moment of 2026-11-20 in Asia/Seoul, the settings' default zone, and of the day after.
const DAY_FROM: i64 = 1_795_100_400_000;
const DAY_TO: i64 = 1_795_186_800_000;

/// What the Outlook read on the node answers.
#[derive(Clone)]
enum Outlook {
  /// The read ran and printed this answer, packed as the script packs it.
  Answers(Value),
  /// The read ended with this code and this line on standard error.
  Fails(i64, &'static str),
}

/// What the page token road answers.
#[derive(Clone)]
enum Teams {
  Hands,
  Refuses(StatusCode, &'static str),
}

/// The stand-in's script and what it was asked.
struct Upstreams {
  outlook: Outlook,
  teams: Teams,
  /// Whether the account service links the receiver's owner sender to an account.
  owner_linked: bool,
  runs: Vec<(String, Value, String)>,
  page_tokens: Vec<(Value, String)>,
  graph_pages: Vec<(Option<String>, String)>,
}

type Shared = Arc<Mutex<Upstreams>>;

fn bearer(headers: &HeaderMap) -> String {
  headers
    .get("authorization")
    .and_then(|value| value.to_str().ok())
    .and_then(|value| value.strip_prefix("Bearer "))
    .unwrap_or_default()
    .to_owned()
}

fn refused(status: StatusCode, code: &str) -> Response {
  (status, axum::Json(json!({ "code": code, "error": format!("{code} by the stand-in") }))).into_response()
}

fn packed(answer: &Value) -> String {
  let mut gzip = GzEncoder::new(Vec::new(), Compression::default());

  gzip.write_all(answer.to_string().as_bytes()).expect("the answer is packed");
  STANDARD.encode(gzip.finish().expect("the answer is packed"))
}

async fn supervisor(headers: HeaderMap, Segment(name): Segment<String>) -> Response {
  if bearer(&headers) != AGENCY_TOKEN {
    return refused(StatusCode::UNAUTHORIZED, "unauthorized");
  }

  match name.as_str() {
    "u60" => axum::Json(json!({ "name": "u60", "ownerAccount": 60 })).into_response(),
    "bmo" => axum::Json(json!({ "name": "bmo", "ownerAccount": null })).into_response(),
    _ => refused(StatusCode::NOT_FOUND, "not-found"),
  }
}

async fn owner(headers: HeaderMap) -> Response {
  if bearer(&headers) != RECEIVER_TOKEN {
    return refused(StatusCode::UNAUTHORIZED, "unauthorized");
  }

  axum::Json(json!({ "senderId": 7, "room": 70 })).into_response()
}

async fn sender_user(
  State(upstreams): State<Shared>,
  headers: HeaderMap,
  Segment(sender): Segment<i64>,
) -> Response {
  if bearer(&headers) != ACCOUNTS_TOKEN {
    return refused(StatusCode::UNAUTHORIZED, "unauthorized");
  }

  if sender == 7 && upstreams.lock().expect("the stand-in's record opens").owner_linked {
    axum::Json(json!({ "user": 4 })).into_response()
  } else {
    refused(StatusCode::NOT_FOUND, "not-found")
  }
}

async fn role_agents(headers: HeaderMap) -> Response {
  if bearer(&headers) != AGENCY_TOKEN {
    return refused(StatusCode::UNAUTHORIZED, "unauthorized");
  }

  axum::Json(json!({ "items": [
    { "name": "actor-01", "role": "actor", "ownerAccount": null },
    { "name": "actor-u61", "role": "actor", "ownerAccount": "61" },
  ] }))
  .into_response()
}

async fn person_nodes(headers: HeaderMap) -> Response {
  if bearer(&headers) != AGENCY_TOKEN {
    return refused(StatusCode::UNAUTHORIZED, "unauthorized");
  }

  axum::Json(json!({ "items": [
    { "ownerAccount": "61", "node": "u4", "shared": true, "setAt": 1 },
    { "ownerAccount": "4", "node": "u4", "shared": true, "setAt": 1 },
    { "ownerAccount": "60", "node": "u60", "shared": false, "setAt": 1 },
  ] }))
  .into_response()
}

async fn run(
  State(upstreams): State<Shared>,
  headers: HeaderMap,
  Segment(node): Segment<String>,
  axum::Json(body): axum::Json<Value>,
) -> Response {
  let mut upstreams = upstreams.lock().expect("the stand-in's record opens");

  upstreams.runs.push((node, body, bearer(&headers)));
  match upstreams.outlook.clone() {
    Outlook::Answers(answer) => axum::Json(json!({
      "node": "u60", "shell": "powershell", "exitCode": 0, "signal": null,
      "stdout": packed(&answer), "stderr": "", "stdoutCut": false, "stderrCut": false,
    }))
    .into_response(),
    Outlook::Fails(code, line) => axum::Json(json!({
      "node": "u60", "shell": "powershell", "exitCode": code, "signal": null,
      "stdout": "", "stderr": format!("At line:1\n{line}\n"), "stdoutCut": false, "stderrCut": false,
    }))
    .into_response(),
  }
}

async fn page_token(
  State(upstreams): State<Shared>,
  headers: HeaderMap,
  axum::Json(body): axum::Json<Value>,
) -> Response {
  let mut upstreams = upstreams.lock().expect("the stand-in's record opens");

  upstreams.page_tokens.push((body, bearer(&headers)));
  match upstreams.teams {
    Teams::Hands => axum::Json(json!({
      "token": PAGE_TOKEN, "scopes": "Mail.ReadWrite Calendars.Read", "expiresAt": 1, "node": "u60",
    }))
    .into_response(),
    Teams::Refuses(status, code) => refused(status, code),
  }
}

async fn graph_messages(
  State((upstreams, address)): State<(Shared, String)>,
  headers: HeaderMap,
  Query(query): Query<Vec<(String, String)>>,
) -> Response {
  let filter = query.iter().find(|(key, _)| key == "$filter").map(|(_, value)| value.clone());
  let second = query.iter().any(|(key, value)| key == "page" && value == "2");

  upstreams
    .lock()
    .expect("the stand-in's record opens")
    .graph_pages
    .push((filter, bearer(&headers)));
  if second {
    return axum::Json(json!({ "value": [{
      "subject": null,
      "sentDateTime": "2026-11-20T00:10:00Z",
      "toRecipients": [{ "emailAddress": { "address": "lead@example.com" } }],
    }] }))
    .into_response();
  }

  axum::Json(json!({
    "value": [{
      "subject": "Weekly plan",
      "sentDateTime": "2026-11-20T03:00:00Z",
      "toRecipients": [{ "emailAddress": { "name": "Team", "address": "team@example.com" } }],
      "ccRecipients": [{ "emailAddress": { "name": "Lead", "address": "lead@example.com" } }],
      "bccRecipients": [{ "emailAddress": { "name": "Archive", "address": "archive@example.com" } }],
    }],
    "@odata.nextLink": format!("{address}/v1.0/me/mailFolders/sentitems/messages?page=2"),
  }))
  .into_response()
}

/// The stand-in served on a free port of this machine, with a data root whose settings name it for every
/// upstream and `u4` as the owner's node; the server ends with the test's runtime.
async fn stand_in(name: &str, outlook: Outlook, teams: Teams) -> (Shared, PathBuf) {
  let upstreams = Arc::new(Mutex::new(Upstreams {
    outlook,
    teams,
    owner_linked: true,
    runs: Vec::new(),
    page_tokens: Vec::new(),
    graph_pages: Vec::new(),
  }));
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("a port is taken");
  let address = format!("http://{}", listener.local_addr().expect("the port reads"));
  let graph = Router::new()
    .route("/v1.0/me/mailFolders/sentitems/messages", get(graph_messages))
    .with_state((Arc::clone(&upstreams), address.clone()));
  let router = Router::new()
    .route("/supervisors/{name}", get(supervisor))
    .route("/role-agents", get(role_agents))
    .route("/owner", get(owner))
    .route("/senders/{sender}/user", get(sender_user))
    .route("/person-nodes", get(person_nodes))
    .route("/nodes/{node}/runs", post(run))
    .route("/page-tokens", post(page_token))
    .with_state(Arc::clone(&upstreams))
    .merge(graph);

  tokio::spawn(async move { axum::serve(listener, router).await });

  (upstreams, data_root(name, &address))
}

fn data_root(name: &str, address: &str) -> PathBuf {
  let root = std::env::temp_dir().join(format!("bmo-mail-sent-{}-{name}", std::process::id()));
  let env = root.join("env");
  let _ = std::fs::remove_dir_all(&root);

  std::fs::create_dir_all(&env).expect("the data root is made");
  for (file, token) in [
    ("agency.token", AGENCY_TOKEN),
    ("receiver.token", RECEIVER_TOKEN),
    ("node.token", NODE_TOKEN),
    ("teams.token", TEAMS_TOKEN),
  ] {
    std::fs::write(env.join(file), format!("{token}\n")).expect("a token file is written");
  }
  std::fs::write(
    env.join("mail.toml"),
    format!(
      "[agency]\naddress = \"{address}\"\ntoken_file = \"agency.token\"\n\
       [receiver]\naddress = \"{address}\"\ntoken_file = \"receiver.token\"\n\
       [account_service]\naddress = \"{address}\"\ntoken = \"{ACCOUNTS_TOKEN}\"\n\
       [outlook]\nowner_node = \"u4\"\n\
       [node]\naddress = \"{address}\"\ntoken_file = \"node.token\"\n\
       [ms_teams]\naddress = \"{address}\"\ntoken_file = \"teams.token\"\n\
       [graph]\naddress = \"{address}/v1.0\"\n"
    ),
  )
  .expect("the settings are written");

  root
}

/// A call as the tool host hands it, for the session `holder` holds.
fn call(root: &Path, holder: &str, arguments: Value) -> String {
  json!({
    "protocol": 1,
    "kind": "call",
    "entry": "bmo/mail_sent",
    "arguments": arguments,
    "session": {
      "holderName": holder, "namespaces": ["bmo"], "capabilities": ["room.tools"], "grants": [],
      "variables": { "BMO_DATA_DIR": root.to_string_lossy() }, "createdAt": 1,
    },
  })
  .to_string()
}

/// 09:30 on 2026-11-20 in Asia/Seoul.
fn morning() -> DateTime<Utc> {
  DateTime::from_timestamp_millis(DAY_FROM + 9 * 3_600_000 + 1_800_000).expect("the moment reads")
}

fn refusal_text(result: &Value) -> String {
  assert_eq!(result["isError"], json!(true), "a refusal: {result}");

  result["content"][0]["text"].as_str().expect("the refusal says why").to_owned()
}

#[tokio::test]
async fn the_outlook_road_reads_the_callers_own_persons_day_on_their_node() {
  let messages = json!([
    { "subject": "Second", "sentAt": DAY_FROM + 7_200_000, "recipients": [
      { "name": "Team", "address": "team@example.com", "kind": "to" },
    ] },
    { "subject": "First", "sentAt": DAY_FROM + 3_600_000, "recipients": [
      { "name": "Lead", "address": "lead@example.com", "kind": "cc" },
    ] },
  ]);
  let (upstreams, root) = stand_in(
    "outlook",
    Outlook::Answers(json!({ "standing": true, "messages": messages })),
    Teams::Hands,
  )
  .await;

  let result = answer(&call(&root, "u60", json!({})), morning()).await;

  assert_eq!(result["isError"], json!(false), "{result}");
  let sent = &result["structuredContent"];
  assert_eq!(sent["day"], json!("2026-11-20"));
  assert_eq!(sent["road"], json!("outlook"));
  assert_eq!(sent["node"], json!("u60"));
  let subjects: Vec<&Value> =
    sent["messages"].as_array().expect("messages").iter().map(|m| &m["subject"]).collect();
  assert_eq!(subjects, [&json!("First"), &json!("Second")], "in the order they were sent");
  let reading = result["content"][0]["text"].as_str().expect("a reading");
  assert!(reading.contains("01:00 First (cc: Lead <lead@example.com>)"), "{reading}");

  let upstreams = upstreams.lock().expect("the stand-in's record opens");
  let (node, body, token) = &upstreams.runs[0];
  assert_eq!((node.as_str(), token.as_str()), ("u60", NODE_TOKEN));
  assert_eq!(body["account"], json!("60"));
  assert_eq!(body["shell"], json!(SHELL));
  assert_eq!(body["env"][FROM_VARIABLE], json!(DAY_FROM.to_string()));
  assert_eq!(body["env"][TO_VARIABLE], json!(DAY_TO.to_string()));
  assert!(upstreams.page_tokens.is_empty(), "the Graph road is not asked while Outlook stands");
}

#[tokio::test]
async fn the_graph_road_reads_with_the_page_token_where_outlook_does_not_stand() {
  let (upstreams, root) = stand_in(
    "graph",
    Outlook::Answers(json!({ "standing": false, "reason": "no Outlook is running" })),
    Teams::Hands,
  )
  .await;

  let result = answer(&call(&root, "u60", json!({ "day": "2026-11-20" })), morning()).await;

  assert_eq!(result["isError"], json!(false), "{result}");
  assert_eq!(
    result["structuredContent"],
    json!({ "day": "2026-11-20", "road": "graph", "node": "u60", "messages": [
      { "subject": "", "sentAt": DAY_FROM + 33_000_000, "recipients": [
        { "name": "", "address": "lead@example.com", "kind": "to" },
      ] },
      { "subject": "Weekly plan", "sentAt": DAY_FROM + 43_200_000, "recipients": [
        { "name": "Team", "address": "team@example.com", "kind": "to" },
        { "name": "Lead", "address": "lead@example.com", "kind": "cc" },
        { "name": "Archive", "address": "archive@example.com", "kind": "bcc" },
      ] },
    ] })
  );

  let upstreams = upstreams.lock().expect("the stand-in's record opens");
  assert_eq!(
    upstreams.page_tokens,
    [(json!({ "account": 60, "scopes": SCOPES }), TEAMS_TOKEN.to_owned())]
  );
  assert_eq!(
    upstreams.graph_pages,
    [
      (
        Some("sentDateTime ge 2026-11-19T15:00:00Z and sentDateTime lt 2026-11-20T15:00:00Z".to_owned()),
        PAGE_TOKEN.to_owned()
      ),
      (None, PAGE_TOKEN.to_owned()),
    ]
  );
}

#[tokio::test]
async fn a_machine_shared_with_other_accounts_is_refused_before_any_road() {
  let (upstreams, root) =
    stand_in("shared", Outlook::Answers(json!({ "standing": true })), Teams::Hands).await;

  let why = refusal_text(&answer(&call(&root, "actor-u61", json!({})), morning()).await);

  assert!(why.contains("the account 61's node u4 is shared"), "{why}");
  let upstreams = upstreams.lock().expect("the stand-in's record opens");
  assert!(upstreams.runs.is_empty() && upstreams.page_tokens.is_empty(), "no road is tried");
}

#[tokio::test]
async fn the_deployments_own_caller_reads_the_owners_account_on_the_owners_node() {
  let (upstreams, root) =
    stand_in("owner", Outlook::Answers(json!({ "standing": true, "messages": [] })), Teams::Hands).await;

  let result = answer(&call(&root, "bmo", json!({})), morning()).await;

  assert_eq!(result["structuredContent"]["node"], json!("u4"), "{result}");
  let upstreams = upstreams.lock().expect("the stand-in's record opens");
  let (node, body, _) = &upstreams.runs[0];
  assert_eq!((node.as_str(), &body["account"]), ("u4", &json!("4")));
}

#[tokio::test]
async fn an_owner_linked_to_no_account_reads_nobodys_mail() {
  let (upstreams, root) =
    stand_in("unlinked", Outlook::Answers(json!({ "standing": true })), Teams::Hands).await;
  upstreams.lock().expect("the stand-in's record opens").owner_linked = false;

  let why = refusal_text(&answer(&call(&root, "bmo", json!({})), morning()).await);

  assert!(
    why.contains("bmo acts for the owner's account, which the owner's sender is linked to none of"),
    "{why}"
  );
}

#[tokio::test]
async fn where_neither_road_stands_the_refusal_names_each_reason() {
  let (_, root) = stand_in(
    "neither",
    Outlook::Fails(1, "Exception calling GetActiveObject"),
    Teams::Refuses(StatusCode::SERVICE_UNAVAILABLE, "not-ready"),
  )
  .await;

  let why = refusal_text(&answer(&call(&root, "u60", json!({})), morning()).await);

  assert!(
    why.contains("Outlook: the read on u60 ended with 1: Exception calling GetActiveObject"),
    "{why}"
  );
  assert!(why.contains("Graph: bmo-ms-teams answered 503 not-ready"), "{why}");
}

#[tokio::test]
async fn a_day_that_is_not_a_date_is_refused() {
  let (_, root) = stand_in("bad-day", Outlook::Answers(json!({ "standing": true })), Teams::Hands).await;

  let why = refusal_text(&answer(&call(&root, "u60", json!({ "day": "20 November" })), morning()).await);

  assert!(why.contains("the day 20 November is not a date"), "{why}");
}

#[test]
fn the_schemas_the_tools_folder_keeps_are_the_types() {
  let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("tools").join("sent-mail");
  let (input, output) = schemas();
  let kept = |file: &str| std::fs::read_to_string(folder.join(file)).expect("the schema file reads");

  assert_eq!(kept("input.json"), input, "cargo run --example tool_schemas writes them again");
  assert_eq!(kept("output.json"), output, "cargo run --example tool_schemas writes them again");
}
