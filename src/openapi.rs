//! The service's OpenAPI 3.1 document (`docs/api.md`, 「The contract」): every registered route with the part
//! of the document it states beside its answer, the bodies' schemas written from the types the routes take and
//! answer, the events, and the harness plugin's tools from the table the MCP server lists. The repository keeps
//! it as `docs/openapi.json`.

use schemars::generate::SchemaGenerator;
use serde_json::{Map, Value, json};

use crate::announcements::{
  ACCOUNT_CHANGED, ACCOUNT_CREATED, ACCOUNT_DELETED, FOLDER_CHANGED, MESSAGE_CHANGED, MESSAGE_CREATED,
  MESSAGE_DELETED,
};
use crate::http::{Answer, Guard, Operation, routes};
use crate::mcp::{ENTRY, MCP, PLUGIN, generator, tools};
use crate::wire::WireError;
use crate::writing::send::SEND_CHANGED;

/// Where the schemas the routes refer to stand in the document.
const SCHEMAS: &str = "/components/schemas";

const DESCRIPTION: &str = "Mailboxes read, written and watched behind one API, each account an owner's and \
  of the kind whose adapter reaches it: accounts, folders, messages and threads, drafts and sends, labels, \
  rules, spam senders and settings, and the console bundle the mail screen is. Every route the screen uses \
  takes the role the reader holds in `Owner-Role` and the opaque owner key the caller acts for in `Owner-Key`; \
  a route that reads or changes an account reaches that owner's accounts alone. A mailbox's members nest under its \
  account. Bodies are JSON with camelCase keys; times are integers of epoch \
  milliseconds. A list is a page; a page of messages is newest first with a message's `at` as its cursor, and \
  the small lists are paged by `position`. A key holding a slash stands in a path percent-encoded. Every \
  failure is answered in the error shape with its status. The harness plugin's tools, each a thin call of one \
  route, are stated under `x-tools`.";

/// Each code a route can answer, its status and what it means.
const CODES: &[(&str, u16, &str)] = &[
  ("bad-request", 400, "the body, the query or the path is not the shape the route takes"),
  ("unauthorized", 401, "the bearer token is missing or wrong"),
  (
    "forbidden",
    403,
    "the role is missing or does not allow the screen's, or the owner key is missing",
  ),
  ("no-route", 404, "no route answers this method and path"),
  ("not-found", 404, "nothing stands at the key the path names"),
  ("conflict", 409, "an account's or a label's name is taken, or a send has already gone"),
  ("gone", 410, "the events after the number asked for are no longer kept"),
  ("too-large", 413, "a file or a message is past the size the submission server takes"),
  ("too-many-labels", 422, "the account holds the most labels it may"),
  ("flag-not-settable", 422, "a flag is not one a request may set"),
  ("unknown-label", 422, "a body names a label the account does not hold"),
  ("unknown-folder", 422, "a body names a folder the mailbox does not hold"),
  ("unknown-message", 422, "a body names a message the account does not hold"),
  ("outside-trash", 422, "a message to delete is not in spam or trash"),
  ("no-recipients", 422, "a draft to send has no recipient"),
  ("send-too-late", 422, "a send's sendAt is past the undo span from now"),
  ("no-receipt-asked", 422, "the message asks for no read receipt"),
  ("draft-not-placed", 422, "the server did not say where it put a saved draft"),
  ("unknown-kind", 422, "a body names a kind of account the service does not declare"),
  ("sign-in-failed", 422, "the account's servers did not take its sign-in"),
  (
    "no-authorization",
    422,
    "an authorization is asked for a kind that signs in by password",
  ),
  (
    "unknown-authorization",
    422,
    "a body names an authorization that is not the owner's, has run out or was used",
  ),
  ("failed", 500, "the service failed"),
  ("upstream-failed", 502, "the mail server did not answer, or failed"),
  ("not-ready", 503, "the service has no key file to seal an account's secrets with"),
];

/// The parameters of a query's schema, each property one.
fn query_parameters(schema: &Value) -> Vec<Value> {
  let required: Vec<&str> = schema["required"]
    .as_array()
    .map(|names| names.iter().filter_map(Value::as_str).collect())
    .unwrap_or_default();
  let Some(properties) = schema["properties"].as_object() else {
    return Vec::new();
  };

  properties
    .iter()
    .map(|(name, property)| {
      let mut parameter =
        json!({ "name": name, "in": "query", "required": required.contains(&name.as_str()), "schema": property });

      if let Some(description) = property.get("description") {
        parameter["description"] = description.clone();
      }

      parameter
    })
    .collect()
}

fn operation_of(operation: Operation, generator: &mut SchemaGenerator) -> Value {
  let mut parameters: Vec<Value> = operation
    .keys
    .iter()
    .map(|(name, description)| {
      json!({ "name": name, "in": "path", "required": true, "description": description, "schema": { "type": "string" } })
    })
    .collect();

  let screen = operation.guard == Guard::Screen;

  if screen {
    parameters.push(json!({
      "name": "Owner-Role",
      "in": "header",
      "required": true,
      "description": "The role the reader holds; either allows the screen's, member. The console sets it for the person signed in.",
      "schema": { "type": "string", "enum": ["administrator", "member"] },
    }));
    parameters.push(json!({
      "name": "Owner-Key",
      "in": "header",
      "required": true,
      "description": "The opaque key of the owner the caller acts for; the console sets it for the person signed in.",
      "schema": { "type": "string", "minLength": 1, "maxLength": 200 },
    }));
  }

  if let Some(query) = operation.query {
    parameters.extend(query_parameters(&query(generator).to_value()));
  }

  let mut codes = vec!["unauthorized"];

  if screen {
    codes.push("forbidden");
  }

  if !parameters.is_empty() || operation.body.is_some() {
    codes.push("bad-request");
  }

  codes.extend(operation.codes);
  codes.push("failed");

  if operation.keys.iter().any(|(name, _)| *name == "accountName") {
    codes.push("upstream-failed");
  }

  let content = match operation.answer {
    Answer::Nothing => None,
    Answer::Json(schema) => Some(json!({ "application/json": { "schema": schema(generator) } })),
    Answer::Bytes => Some(json!({
      "application/octet-stream": { "schema": { "type": "string", "contentEncoding": "binary" } },
    })),
    Answer::Events(page) => Some(json!({
      "text/event-stream": { "schema": { "type": "string", "description": "Server-sent events, each an event." } },
      "application/json": { "schema": page(generator) },
    })),
  };
  let mut success = json!({ "description": operation.summary });

  if let Some(content) = content {
    success["content"] = content;
  }

  let mut responses = Map::new();

  responses.insert(operation.status.to_string(), success);
  responses.insert("default".into(), json!({ "$ref": "#/components/responses/Error" }));

  let mut described = json!({
    "operationId": operation.id,
    "tags": [operation.tag],
    "summary": operation.summary,
    "responses": responses,
    "x-codes": codes,
  });

  if !parameters.is_empty() {
    described["parameters"] = parameters.into();
  }

  if let Some(body) = operation.body {
    described["requestBody"] =
      json!({ "required": true, "content": { "application/json": { "schema": body(generator) } } });
  }

  described
}

/// The OpenAPI document.
pub fn openapi() -> Value {
  let mut generator = generator(SCHEMAS);
  let mut paths = Map::new();
  let mut registered: Vec<_> = routes().collect();

  registered.sort_by_key(|route| (route.path(), route.method().to_string()));

  for route in registered {
    let method = route.method().as_str().to_lowercase();
    let described = operation_of(route.operation(), &mut generator);

    paths.entry(route.path()).or_insert_with(|| json!({}))[method] = described;
  }

  let x_tools: Vec<Value> = tools()
    .iter()
    .map(|tool| {
      json!({
        "name": tool.name,
        "description": tool.description,
        "input": tool.input(&mut generator),
        "output": tool.output(&mut generator),
        "route": tool.route,
      })
    })
    .collect();
  let error = generator.subschema_for::<WireError>();
  let codes: Vec<String> = CODES
    .iter()
    .map(|(code, status, meaning)| format!("- `{code}` ({status}): {meaning}"))
    .collect();

  json!({
    "openapi": "3.1.0",
    "info": { "title": "bmo-mail", "version": env!("CARGO_PKG_VERSION"), "description": DESCRIPTION },
    "servers": [{ "url": "/", "description": "the mail service" }],
    "security": [{ "bearer": [] }],
    "x-layer": "upstream",
    "x-stage": 2,
    "x-calls": [],
    "x-harness": { "plugin": PLUGIN, "mcp": MCP, "entry": ENTRY },
    "tags": [
      { "name": "health" },
      { "name": "events" },
      { "name": "accounts" },
      { "name": "summary" },
      { "name": "avatars" },
      { "name": "console" },
    ],
    "paths": paths,
    "components": {
      "schemas": generator.take_definitions(true),
      "responses": {
        "Error": {
          "description": format!("A failure, in the common error shape. The codes:\n{}", codes.join("\n")),
          "content": { "application/json": { "schema": error } },
        },
      },
      "securitySchemes": {
        "bearer": {
          "type": "http",
          "scheme": "bearer",
          "description": "The service's token, from the caller's own configuration.",
        },
      },
    },
    "x-events": [
      MESSAGE_CREATED,
      MESSAGE_CHANGED,
      MESSAGE_DELETED,
      SEND_CHANGED,
      FOLDER_CHANGED,
      ACCOUNT_CREATED,
      ACCOUNT_CHANGED,
      ACCOUNT_DELETED,
    ],
    "x-tools": x_tools,
  })
}

/// The document as the repository keeps it: two-space indented JSON with a closing newline.
pub fn openapi_json() -> String {
  let mut text = serde_json::to_string_pretty(&openapi()).unwrap_or_default();

  text.push('\n');
  text
}
