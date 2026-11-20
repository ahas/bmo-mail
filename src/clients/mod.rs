//! The clients of the units the hosted tool calls (`docs/api.md`, 「The hosted tool」), one module for each,
//! holding only the calls this repository makes, and what every one of them stands on: a unit reached at its
//! address with the token its file holds.

pub mod accounts;
pub mod agency;
pub mod ms_teams;
pub mod node;
pub mod receiver;

use std::path::PathBuf;

use reqwest::{Method, RequestBuilder};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::settings::Upstream;
use crate::wire::WireError;

/// Why a call of a unit came to no answer of the shape it asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
  /// The settings name no address or no token file for the unit, or the file does not read.
  Unconfigured(String),
  /// The unit answered with a status that is no success: its code and sentence where its body was the common
  /// error shape.
  Refused {
    unit: &'static str,
    status: u16,
    answer: Option<WireError>,
  },
  /// Nothing answered at the address.
  Unreached(String),
  /// A success came back in no shape the call reads.
  Unreadable(String),
}

impl std::fmt::Display for Failure {
  fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Unconfigured(why) | Self::Unreached(why) | Self::Unreadable(why) => out.write_str(why),
      Self::Refused {
        unit,
        status,
        answer: Some(answer),
      } => write!(out, "{unit} answered {status} {}: {}", answer.code, answer.error),
      Self::Refused {
        unit,
        status,
        answer: None,
      } => write!(out, "{unit} answered {status}"),
    }
  }
}

/// Where a unit's token is had: given itself, or read from its file at each call.
#[derive(Clone)]
enum Token {
  Given(String),
  File(PathBuf),
}

/// A unit reached at its address with its token.
#[derive(Clone)]
pub struct Reach {
  name: &'static str,
  address: String,
  token: Token,
  http: reqwest::Client,
}

impl Reach {
  /// The unit `name` as `upstream` names it; refused where the settings leave its address or its token out.
  pub fn of(name: &'static str, upstream: &Upstream, http: reqwest::Client) -> Result<Self, Failure> {
    let address = upstream.address.trim_end_matches('/');
    let token = match (upstream.token.trim(), &upstream.token_file) {
      ("", Some(file)) => Some(Token::File(file.clone())),
      ("", None) => None,
      (given, _) => Some(Token::Given(given.to_owned())),
    };

    match token {
      Some(token) if !address.is_empty() => Ok(Self {
        name,
        address: address.to_owned(),
        token,
        http,
      }),
      _ => Err(Failure::Unconfigured(format!("the settings name no address and token for {name}"))),
    }
  }

  /// The token as it stands now, a file's surrounding whitespace dropped. The sentence names the file, never
  /// what it holds.
  fn token(&self) -> Result<String, Failure> {
    let file = match &self.token {
      Token::Given(token) => return Ok(token.clone()),
      Token::File(file) => file,
    };
    let unread =
      |why: String| Failure::Unconfigured(format!("the token file for {} does not read: {why}", self.name));
    let token = std::fs::read_to_string(file).map_err(|why| unread(why.to_string()))?;
    let token = token.trim();

    if token.is_empty() {
      Err(unread("it is empty".to_owned()))
    } else {
      Ok(token.to_owned())
    }
  }

  fn request(&self, method: Method, path: &str) -> Result<RequestBuilder, Failure> {
    let token = self.token()?;

    Ok(self.http.request(method, format!("{}{path}", self.address)).bearer_auth(token))
  }

  /// `GET path`, read as `T`.
  pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, Failure> {
    self.send(self.request(Method::GET, path)?).await
  }

  /// `POST path` with `body`, read as `T`.
  pub async fn post<B: Serialize, T: DeserializeOwned>(&self, path: &str, body: &B) -> Result<T, Failure> {
    self.send(self.request(Method::POST, path)?.json(body)).await
  }

  async fn send<T: DeserializeOwned>(&self, request: RequestBuilder) -> Result<T, Failure> {
    let answer = request
      .send()
      .await
      .map_err(|why| Failure::Unreached(format!("{} could not be reached: {why}", self.name)))?;
    let status = answer.status();
    let body = answer
      .bytes()
      .await
      .map_err(|why| Failure::Unreached(format!("{}'s answer broke off: {why}", self.name)))?;

    if !status.is_success() {
      return Err(Failure::Refused {
        unit: self.name,
        status: status.as_u16(),
        answer: serde_json::from_slice(&body).ok(),
      });
    }

    serde_json::from_slice(&body)
      .map_err(|why| Failure::Unreadable(format!("{}'s answer does not read: {why}", self.name)))
  }
}

/// An account's id written out, as a unit writes it either as a number or as a string; none for anything else.
pub fn account_id(value: &Value) -> Option<String> {
  match value {
    Value::Number(number) => Some(number.to_string()),
    Value::String(text) if !text.is_empty() => Some(text.clone()),
    _ => None,
  }
}
