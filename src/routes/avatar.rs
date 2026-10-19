//! Sender avatars (`docs/api.md`, 「Sender avatars」).

use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::avatars::avatar_domain;
use crate::http::{Call, Refused, Route};

/// `GET /api/mail/avatar`: one sender domain's logo (`docs/mail.md`, 「Sender avatars」), fetched by the
/// service and never by the browser. A domain that publishes none answers with nothing at all, and the
/// screen draws the sender's initial in its place; that answer is kept by the browser for the hour, so a
/// list drawn again does not ask for every row it already asked for.
pub struct Avatar;

fn with_headers(mut response: Response, headers: &[(header::HeaderName, &str)]) -> Response {
  for (name, value) in headers {
    if let Ok(value) = HeaderValue::from_str(value) {
      response.headers_mut().insert(name.clone(), value);
    }
  }

  response
}

impl Route for Avatar {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/api/mail/avatar";

  async fn answer(&self, call: Call) -> Result<Response, Refused> {
    let standing = call.standing().clone();
    let asked = call
      .query()
      .get("domain")
      .and_then(|domain| domain.as_str().map(str::to_owned))
      .unwrap_or_default();
    let found = match avatar_domain(&asked) {
      Some(domain) => standing.avatars.sender_avatar(standing.store.pool().clone(), domain).await,
      None => None,
    };
    let Some(found) = found else {
      return Ok(with_headers(
        StatusCode::NO_CONTENT.into_response(),
        &[(header::CACHE_CONTROL, "private, max-age=3600")],
      ));
    };
    let headers = [
      (header::CONTENT_TYPE, found.kind.as_str()),
      (
        header::CONTENT_SECURITY_POLICY,
        "sandbox; default-src 'none'; style-src 'unsafe-inline'",
      ),
      (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
      (header::CACHE_CONTROL, "private, max-age=86400"),
    ];

    Ok(with_headers((StatusCode::OK, found.bytes).into_response(), &headers))
  }
}

crate::route!(Avatar);
