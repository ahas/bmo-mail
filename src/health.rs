//! Whether the service is up (`docs/api.md`, 「Health」): the route, and the command that asks it.

use axum::http::{Method, StatusCode};
use axum::response::Response;

use crate::http::{Answer, Call, Guard, Operation, Refused, Route, answer, schema};
use crate::settings::Settings;
use crate::wire::Health;

/// Readiness and the version. A service that answers has its store: it opens and migrates it before it
/// answers anything.
pub struct HealthRoute;

impl Route for HealthRoute {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/health";
  const OPERATION: Operation = Operation {
    id: "listHealth",
    summary: "Readiness and version.",
    tag: "health",
    keys: &[],
    query: None,
    body: None,
    status: 200,
    answer: Answer::Json(schema::<Health>),
    codes: &[],
    guard: Guard::Token,
  };

  async fn answer(&self, _call: Call) -> Result<Response, Refused> {
    let health = Health {
      ready: true,
      reason: None,
      version: env!("CARGO_PKG_VERSION").to_owned(),
    };

    Ok(answer(StatusCode::OK, &health))
  }
}

crate::route!(HealthRoute);

/// Whether the service the settings name answers ready, asked once with the settings' token and waited on
/// until it answers.
pub async fn ask(settings: &Settings) -> Result<(), String> {
  let address = format!("http://{}/health", settings.server.listen);
  let answer = reqwest::Client::new()
    .get(&address)
    .bearer_auth(&settings.server.token)
    .send()
    .await
    .map_err(|why| format!("the service at {} did not answer: {why}", settings.server.listen))?;
  let status = answer.status();

  if !status.is_success() {
    return Err(format!("the service answered {}", status.as_u16()));
  }

  let read: Health = answer.json().await.map_err(|why| format!("the answer could not be read: {why}"))?;

  if read.ready {
    Ok(())
  } else {
    Err(format!("the service is not ready: {}", read.reason.unwrap_or_default()))
  }
}
