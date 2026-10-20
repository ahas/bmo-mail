//! Sender avatars (`docs/api.md`, 「Sender avatars」).

use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::avatars::avatar_domain;
use crate::http::{Answer, Call, NOT_FOUND, Operation, Refused, Route};

/// `GET /avatars/{address}`: the logo a sender's domain publishes (`docs/mail.md`, 「Sender avatars」),
/// fetched by the service and never by a browser. A domain that publishes none is a 404, which a reader keeps
/// for the hour, so a list drawn again does not ask for every row it already asked for.
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
  const PATH: &'static str = "/avatars/{address}";
  const OPERATION: Operation = Operation {
    id: "readAvatarsByAddress",
    summary: "A sender's picture.",
    tag: "avatars",
    keys: &[("address", "A mail address, or its domain.")],
    query: None,
    body: None,
    status: 200,
    answer: Answer::Bytes,
    codes: &["not-found"],
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let standing = call.standing().clone();
    let asked = call.key("address").await?;
    let found = match avatar_domain(&asked) {
      Some(domain) => standing.avatars.sender_avatar(standing.store.pool().clone(), domain).await,
      None => None,
    };
    let Some(found) = found else {
      return Ok(with_headers(
        Refused::new(StatusCode::NOT_FOUND, NOT_FOUND, "the domain publishes no logo").into_response(),
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
