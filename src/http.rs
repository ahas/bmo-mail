//! Routes, the calls they answer, their refusals, and the router the service stands them on (`docs/api.md`,
//! 「The contract」). Each route states its own part of the OpenAPI document beside its answer; the plumbing
//! here knows none of them by name.

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use axum::Router;
use axum::extract::{DefaultBodyLimit, FromRequestParts, Query, RawPathParams, Request, State};
use axum::http::{Method, StatusCode, header};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{MethodFilter, MethodRouter};
use schemars::generate::SchemaGenerator;
use schemars::{JsonSchema, Schema};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::json;

use crate::accounts::Accounts;
use crate::accounts::authorizations::Authorizations;
use crate::adapters::{Context, Mailboxes};
use crate::avatars::Avatars;
use crate::console::{self, SCREEN_ROLE};
use crate::events::Events;
use crate::secrets::Secrets;
use crate::servers::Servers;
use crate::servers::organisation::Organisation;
use crate::settings::Settings;
use crate::store::Store;
use crate::wire::Role;
use crate::wording::Wording;
use crate::writing::send::Sends;

pub use inventory;

/// A schema written where the generator keeps its definitions.
pub type SchemaOf = fn(&mut SchemaGenerator) -> Schema;

/// The schema of `T`, referred to where the generator keeps its definitions.
pub fn schema<T: JsonSchema>(generator: &mut SchemaGenerator) -> Schema {
  generator.subschema_for::<T>()
}

/// The schema of `T` inline: a query's, whose properties are each one parameter.
pub fn inline<T: JsonSchema>(generator: &mut SchemaGenerator) -> Schema {
  T::json_schema(generator)
}

/// What a route's success carries.
#[derive(Clone, Copy)]
pub enum Answer {
  /// No body.
  Nothing,
  /// A JSON body of this schema.
  Json(SchemaOf),
  /// Bytes of the type the answer names.
  Bytes,
  /// The event stream, or a page of it of this schema.
  Events(SchemaOf),
}

/// A route as the OpenAPI document states it.
#[derive(Clone, Copy)]
pub struct Operation {
  pub id: &'static str,
  pub summary: &'static str,
  pub tag: &'static str,
  /// The path's keys, by name, with what each one is.
  pub keys: &'static [(&'static str, &'static str)],
  /// The query's type, each of its properties one parameter.
  pub query: Option<SchemaOf>,
  pub body: Option<SchemaOf>,
  /// The success's status.
  pub status: u16,
  pub answer: Answer,
  /// The codes of its own, beside the token's, the shape's and the failures every route can answer.
  pub codes: &'static [&'static str],
  /// What a call needs beside the token (`docs/api.md`, 「The contract」).
  pub guard: Guard,
}

/// What a call needs before its route answers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Guard {
  /// The service's token alone: a route the screen does not use.
  Token,
  /// The service's token or the console's credential, an `Owner-Role` allowing the screen's role and an
  /// `Owner-Key`: every route the screen uses.
  Screen,
  /// The service's token or the account service's credential for the platform's system mail, and no owner.
  SystemMail,
  /// The service's token, or the console's credential with `Owner-Role` `administrator`: a removal of an owner or
  /// a unit (`docs/api.md`, 「Removing an owner」).
  Administrator,
}

/// One route the service answers, registered beside its answer with `bmo_mail::route!`.
pub trait Route: Send + Sync + 'static {
  const METHOD: Method;
  /// The path, in the router's own syntax: `/accounts/{accountName}/folders`.
  const PATH: &'static str;
  const OPERATION: Operation;

  fn answer(&self, call: Call) -> impl Future<Output = Result<Response, Refused>> + Send;
}

type Answering<'a> = Pin<Box<dyn Future<Output = Result<Response, Refused>> + Send + 'a>>;

/// A route as the router holds it, whatever its type.
pub trait Answers: Send + Sync {
  fn method(&self) -> Method;
  fn path(&self) -> &'static str;
  fn operation(&self) -> Operation;
  fn answer(&self, call: Call) -> Answering<'_>;
}

impl<R: Route> Answers for R {
  fn method(&self) -> Method {
    R::METHOD
  }

  fn path(&self) -> &'static str {
    R::PATH
  }

  fn operation(&self) -> Operation {
    R::OPERATION
  }

  fn answer(&self, call: Call) -> Answering<'_> {
    Box::pin(Route::answer(self, call))
  }
}

/// One registered route.
pub struct Registered(pub &'static dyn Answers);

inventory::collect!(Registered);

/// Registers a route with the service: `bmo_mail::route!(Health);`
#[macro_export]
macro_rules! route {
  ($route:expr) => {
    $crate::http::inventory::submit! { $crate::http::Registered(&$route) }
  };
}

/// Every route linked into this binary.
pub fn routes() -> impl Iterator<Item = &'static dyn Answers> {
  inventory::iter::<Registered>.into_iter().map(|entry| entry.0)
}

/// A route's name as a caller writes it: its method and its path.
pub fn route_name(method: &Method, path: &str) -> String {
  format!("{method} {path}")
}

/// What is wrong with the routes linked in: two answers for one method and path would leave which one
/// answers to the order they were linked, so the service refuses to stand on any.
pub fn route_faults() -> Vec<String> {
  let mut seen = BTreeSet::new();

  routes()
    .map(|route| route_name(&route.method(), route.path()))
    .filter(|name| !seen.insert(name.clone()))
    .map(|name| format!("{name}: registered twice"))
    .collect()
}

/// The codes the conventions give every service.
pub const BAD_REQUEST: &str = "bad-request";
pub const UNAUTHORIZED: &str = "unauthorized";
pub const NO_ROUTE: &str = "no-route";
pub const NOT_FOUND: &str = "not-found";
pub const CONFLICT: &str = "conflict";
pub const GONE: &str = "gone";
pub const TOO_LARGE: &str = "too-large";
pub const FAILED: &str = "failed";
pub const UPSTREAM_FAILED: &str = "upstream-failed";
pub const NOT_READY: &str = "not-ready";
pub const FORBIDDEN: &str = "forbidden";

/// The header the caller names the owner key it acts for in.
pub const OWNER_KEY: &str = "owner-key";
/// The header the caller names the role the reader holds in.
pub const OWNER_ROLE: &str = "owner-role";
/// The longest an owner key may be.
const OWNER_KEY_LONGEST: usize = 200;

/// A refusal: a status, and a body of the kebab-case code a caller matches on and the English sentence for a
/// log (`docs/api.md`, 「Refusals」).
#[derive(Debug, PartialEq, Eq)]
pub struct Refused {
  pub status: StatusCode,
  pub code: &'static str,
  pub error: String,
}

impl Refused {
  /// A refusal under the status and the code the route gives it.
  pub fn new(status: StatusCode, code: &'static str, error: impl Into<String>) -> Self {
    Self {
      status,
      code,
      error: error.into(),
    }
  }

  /// A body or a query not of the route's shape.
  pub fn bad_request(error: impl Into<String>) -> Self {
    Self::new(StatusCode::BAD_REQUEST, BAD_REQUEST, error)
  }

  /// A call the guard of the screen's routes does not let through.
  pub fn forbidden(error: impl Into<String>) -> Self {
    Self::new(StatusCode::FORBIDDEN, FORBIDDEN, error)
  }

  /// Nothing at the key the path names.
  pub fn not_found(error: impl Into<String>) -> Self {
    Self::new(StatusCode::NOT_FOUND, NOT_FOUND, error)
  }

  /// What a route answers for a failure it did not expect: 500, with the failure's own words.
  pub fn failed(error: impl std::fmt::Display) -> Self {
    Self::new(StatusCode::INTERNAL_SERVER_ERROR, FAILED, error.to_string())
  }
}

impl IntoResponse for Refused {
  fn into_response(self) -> Response {
    (self.status, Json(json!({ "code": self.code, "error": self.error }))).into_response()
  }
}

/// What the service stands on, which every call carries.
#[derive(Debug)]
pub struct Standing {
  pub settings: Settings,
  /// The store, open and migrated.
  pub store: Store,
  /// The accounts, each an owner's.
  pub accounts: Accounts,
  /// Each account's open mailbox.
  pub mailboxes: Mailboxes,
  /// The authorizations waiting for the browser to come back.
  pub authorizations: Authorizations,
  /// The event stream and its window.
  pub events: Events,
  /// The service's own words.
  pub wording: Wording,
  /// The sends waiting before they go.
  pub sends: Sends,
  /// The lookups of sender logos.
  pub avatars: Avatars,
  /// The mail servers.
  pub servers: Servers,
  /// The account service, asked who offers which server.
  pub organisation: Organisation,
}

impl Standing {
  /// What the service stands on, its accounts' secrets sealed with `secrets` where it has a key.
  pub fn new(settings: Settings, store: Store, wording: Wording, secrets: Option<Secrets>) -> Self {
    let accounts = Accounts::new(store.pool().clone(), secrets);
    let events = Events::new(store.pool().clone());
    let servers = Servers::new(store.pool().clone());
    let organisation = Organisation::new(&settings);
    Self {
      settings,
      store,
      accounts,
      mailboxes: Mailboxes::default(),
      authorizations: Authorizations::default(),
      events,
      wording,
      sends: Sends::default(),
      avatars: Avatars::default(),
      servers,
      organisation,
    }
  }

  /// What a mailbox stands on beside its account.
  pub fn context(&self) -> Context {
    Context {
      pool: self.store.pool().clone(),
      staging: self.settings.staging.dir.clone(),
    }
  }
}

/// One request a route answers.
pub struct Call {
  pub request: Request,
  standing: Arc<Standing>,
}

// The most a JSON body read whole may carry: 64 MiB, room for a draft whose markup carries its pictures.
pub const BODY_LIMIT: usize = 64 * 1024 * 1024;

impl Call {
  pub fn new(request: Request, standing: Arc<Standing>) -> Self {
    Self { request, standing }
  }

  /// What the service stands on.
  pub fn standing(&self) -> &Arc<Standing> {
    &self.standing
  }

  /// One key of the route's path, percent-decoded.
  pub async fn key(&mut self, name: &str) -> Result<String, Refused> {
    let (mut parts, body) = std::mem::take(&mut self.request).into_parts();
    let read = RawPathParams::from_request_parts(&mut parts, &()).await;

    self.request = Request::from_parts(parts, body);

    let params = read.map_err(|why| Refused::bad_request(why.body_text()))?;

    params
      .iter()
      .find(|(key, _)| *key == name)
      .map(|(_, value)| value.to_owned())
      .ok_or_else(|| Refused::failed(format!("the route's path names no {name}")))
  }

  /// The owner key the request acts for: the `Owner-Key` header, 1 to 200 characters taken as they stand. A
  /// request without one is refused 403, and one past 200 characters 400.
  pub fn owner(&self) -> Result<String, Refused> {
    let key = header_of(&self.request, OWNER_KEY);

    if key.is_empty() {
      return Err(Refused::forbidden("the request names no owner key"));
    }

    if key.chars().count() > OWNER_KEY_LONGEST {
      return Err(Refused::bad_request("the owner key is longer than 200 characters"));
    }

    Ok(key.to_owned())
  }

  /// The query read as the route's type; a query not of its shape is refused 400.
  pub fn query<T: DeserializeOwned>(&self) -> Result<T, Refused> {
    Query::<T>::try_from_uri(self.request.uri())
      .map(|Query(query)| query)
      .map_err(|why| Refused::bad_request(why.body_text()))
  }

  /// The body read as the route's type; no body reads as an empty object, and a body not of the type's shape
  /// is refused 400. A route reads it once.
  pub async fn body<T: DeserializeOwned>(&mut self) -> Result<T, Refused> {
    let body = std::mem::take(self.request.body_mut());
    let bytes = axum::body::to_bytes(body, BODY_LIMIT).await.map_err(|why| {
      Refused::new(StatusCode::PAYLOAD_TOO_LARGE, TOO_LARGE, format!("the body could not be read: {why}"))
    })?;
    let bytes: &[u8] = if bytes.iter().all(u8::is_ascii_whitespace) { b"{}" } else { &bytes };

    serde_json::from_slice(bytes)
      .map_err(|why| Refused::bad_request(format!("the body is not the shape this route takes: {why}")))
  }
}

/// An answer: a member or a page as JSON under a status.
pub fn answer(status: StatusCode, body: &impl Serialize) -> Response {
  (status, Json(body)).into_response()
}

/// An answer with no body.
pub fn nothing() -> Response {
  StatusCode::NO_CONTENT.into_response()
}

/// A header's value as text; empty where the request carries none, or none that is text.
fn header_of<'a>(request: &'a Request, name: &str) -> &'a str {
  request.headers().get(name).and_then(|value| value.to_str().ok()).unwrap_or_default()
}

/// Whether a call to a route the screen uses carries what the console sets for a reader who may open the
/// screen: an `Owner-Role` allowing the screen's role, and an `Owner-Key`.
fn screen_reader(request: &Request) -> Result<(), Refused> {
  let allowed = Role::named(header_of(request, OWNER_ROLE)).is_some_and(|held| held.allows(SCREEN_ROLE));

  if !allowed {
    return Err(Refused::forbidden(format!(
      "the request names no role allowing the screen's, {}",
      SCREEN_ROLE.as_str()
    )));
  }

  if header_of(request, OWNER_KEY).is_empty() {
    return Err(Refused::forbidden("the request names no owner key"));
  }

  Ok(())
}

/// Refuses a removal the console's credential carries for anyone but the administrator; the service's token
/// passes.
fn administrator(standing: &Standing, request: &Request) -> Result<(), Refused> {
  let token = &standing.settings.server.token;
  let service = request
    .headers()
    .get(header::AUTHORIZATION)
    .and_then(|value| value.to_str().ok())
    .and_then(|value| value.strip_prefix("Bearer "))
    .is_some_and(|presented| !token.is_empty() && presented == token);

  if service || header_of(request, OWNER_ROLE) == ADMINISTRATOR {
    Ok(())
  } else {
    Err(Refused::forbidden("a removal is the platform administrator's alone"))
  }
}

/// The role a console call names to reach a removal.
const ADMINISTRATOR: &str = "administrator";

/// Whether a request carries as its bearer the service's token, or on a route the screen uses the console's
/// credential; an empty token or credential lets nobody in.
fn authorized(standing: &Standing, request: &Request, guard: Guard) -> bool {
  let presented = request
    .headers()
    .get(header::AUTHORIZATION)
    .and_then(|value| value.to_str().ok())
    .and_then(|value| value.strip_prefix("Bearer "));
  let Some(presented) = presented else {
    return false;
  };
  let settings = &standing.settings;
  let accepted = |expected: &str| !expected.is_empty() && presented == expected;

  accepted(&settings.server.token)
    || (matches!(guard, Guard::Screen | Guard::Administrator) && accepted(&settings.console.token))
    || (guard == Guard::SystemMail
      && settings.system_mail.credential().is_some_and(|credential| accepted(&credential)))
}

/// What a request to a path no route claims is answered.
async fn no_route(request: Request) -> Response {
  Refused::new(
    StatusCode::NOT_FOUND,
    NO_ROUTE,
    format!("nothing answers {}", route_name(request.method(), request.uri().path())),
  )
  .into_response()
}

/// A path's methods as the router holds them, beside the guard every one of them shares.
type Guarded = (Guard, MethodRouter<Arc<Standing>>);

/// The router answering every registered route behind the token, with the console face beside them where the
/// settings make one and its guard laid over them, or what is wrong with the routes linked in or the face.
pub fn router(standing: Arc<Standing>) -> Result<Router, Vec<String>> {
  let face = console::face(&standing.settings.console).map_err(|why| vec![why])?;
  let faults = route_faults();

  if !faults.is_empty() {
    return Err(faults);
  }

  // Each path's methods, and whether the screen uses them, which every method of one path agrees on.
  let mut by_path: BTreeMap<&'static str, Guarded> = BTreeMap::new();
  let mut faults = Vec::new();

  for route in routes() {
    let Ok(filter) = MethodFilter::try_from(route.method()) else {
      faults.push(format!("{}: the method cannot be routed", route_name(&route.method(), route.path())));
      continue;
    };
    let handler = move |State(standing): State<Arc<Standing>>, request: Request| async move {
      if !authorized(&standing, &request, route.operation().guard) {
        return Refused::new(StatusCode::UNAUTHORIZED, UNAUTHORIZED, "a missing or wrong token")
          .into_response();
      }

      if route.operation().guard == Guard::Screen
        && let Err(refused) = screen_reader(&request)
      {
        return refused.into_response();
      }

      if route.operation().guard == Guard::Administrator
        && let Err(refused) = administrator(&standing, &request)
      {
        return refused.into_response();
      }

      route
        .answer(Call::new(request, standing))
        .await
        .unwrap_or_else(IntoResponse::into_response)
    };
    let guard = route.operation().guard;
    let (held, methods) = by_path.remove(route.path()).unwrap_or((guard, MethodRouter::default()));

    if held != guard {
      faults.push(format!("{}: the path's methods differ in their guard", route.path()));
    }

    by_path.insert(route.path(), (guard, methods.on(filter, handler)));
  }

  if !faults.is_empty() {
    return Err(faults);
  }

  let (screens, others): (Vec<_>, Vec<_>) =
    by_path.into_iter().partition(|(_, (guard, _))| *guard == Guard::Screen);
  let routed = |routes: Vec<(&'static str, Guarded)>| {
    routes
      .into_iter()
      .fold(Router::new(), |router, (path, (_, methods))| router.route(path, methods))
  };
  let screens = match &face {
    Some(face) => face.guard_screens(routed(screens)),
    None => routed(screens),
  };
  let router = routed(others)
    .merge(screens)
    .fallback(no_route)
    .layer(DefaultBodyLimit::max(BODY_LIMIT))
    .with_state(standing);

  Ok(match face {
    Some(face) => router.merge(face.router()),
    None => router,
  })
}
