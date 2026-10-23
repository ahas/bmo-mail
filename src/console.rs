//! The console plugin bundle the service serves (`docs/api.md`, 「The console plugin bundle」): its manifest,
//! its module and stylesheet under names that follow their bytes, and its words, read once at start from the
//! settings' console folder.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use ring::digest::{SHA256, digest};

use crate::announcements::MESSAGE_CREATED;
use crate::http::{Answer, Call, Guard, Operation, Refused, Route, answer, schema};
use crate::wire::{ConsoleManifest, Role};

/// The least role a reader holds to open the screen, and so the floor of every route the screen uses.
pub const SCREEN_ROLE: Role = Role::Member;

/// The bundle's name, its words' namespace.
const NAME: &str = "mail";
/// The path the module and the stylesheet are answered under.
const FILES: &str = "/console-bundle";
/// The path the words are answered at.
const WORDS: &str = "/console-words";

/// The files the build writes into the console folder.
const MODULE_FILE: &str = "module.js";
const STYLESHEET_FILE: &str = "style.css";
const WORDS_FILE: &str = "words.json";

/// How many hex digits of a file's SHA-256 its name carries.
const HASH_DIGITS: usize = 16;

/// A file of the bundle as it is answered: its name under `/console-bundle/` and its bytes.
#[derive(Debug)]
pub struct Served {
  pub name: String,
  pub bytes: Vec<u8>,
}

impl Served {
  /// A file named by its stem, the first hex digits of its bytes' SHA-256 and its ending.
  fn named(stem: &str, ending: &str, bytes: Vec<u8>) -> Self {
    let hash: String = digest(&SHA256, &bytes)
      .as_ref()
      .iter()
      .map(|byte| format!("{byte:02x}"))
      .collect::<String>()
      .chars()
      .take(HASH_DIGITS)
      .collect();

    Self {
      name: format!("{stem}.{hash}.{ending}"),
      bytes,
    }
  }

  /// The path of the service's API it is answered at.
  pub fn path(&self) -> String {
    format!("{FILES}/{}", self.name)
  }
}

/// The built bundle: its module, its stylesheet where it has one, and its words.
#[derive(Debug)]
pub struct Bundle {
  pub module: Served,
  pub stylesheet: Option<Served>,
  /// Each locale's tree under the locale's code, as the words file holds it.
  pub words: Vec<u8>,
}

/// A file of the folder, or none where it does not stand.
fn read_if_standing(path: &Path) -> io::Result<Option<Vec<u8>>> {
  match std::fs::read(path) {
    Ok(bytes) => Ok(Some(bytes)),
    Err(why) if why.kind() == io::ErrorKind::NotFound => Ok(None),
    Err(why) => Err(why),
  }
}

impl Bundle {
  /// The bundle the folder holds; none where it holds no module or no words.
  pub fn read(folder: &Path) -> io::Result<Option<Self>> {
    let (Some(module), Some(words)) = (
      read_if_standing(&folder.join(MODULE_FILE))?,
      read_if_standing(&folder.join(WORDS_FILE))?,
    ) else {
      return Ok(None);
    };

    serde_json::from_slice::<serde_json::Map<String, serde_json::Value>>(&words).map_err(|why| {
      io::Error::new(io::ErrorKind::InvalidData, format!("the words are no JSON object: {why}"))
    })?;

    Ok(Some(Self {
      module: Served::named("module", "js", module),
      stylesheet: read_if_standing(&folder.join(STYLESHEET_FILE))?
        .map(|bytes| Served::named("style", "css", bytes)),
      words,
    }))
  }

  /// The manifest naming this bundle.
  pub fn manifest(&self) -> ConsoleManifest {
    ConsoleManifest {
      name: NAME.to_owned(),
      title: format!("{NAME}.title.screen"),
      icon: "i-material-symbols-mail-outline".to_owned(),
      screen_path: format!("/{NAME}"),
      service_name: NAME.to_owned(),
      bundle: self.module.path(),
      stylesheet: self.stylesheet.as_ref().map(Served::path),
      words: WORDS.to_owned(),
      api_prefix: "/".to_owned(),
      role: SCREEN_ROLE,
      owner_key: true,
      events: true,
      websocket: false,
      push_kinds: vec![MESSAGE_CREATED.to_owned()],
    }
  }

  /// The module or the stylesheet the name names, with its type.
  fn file(&self, name: &str) -> Option<(&Served, &'static str)> {
    let module = (&self.module, "text/javascript; charset=utf-8");
    let stylesheet = self.stylesheet.as_ref().map(|served| (served, "text/css; charset=utf-8"));

    [Some(module), stylesheet].into_iter().flatten().find(|(served, _)| served.name == name)
  }
}

/// The bundle the service stands with, or the refusal a console reads as a service with no screen.
fn bundle_of(call: &Call) -> Result<&Bundle, Refused> {
  call
    .standing()
    .console
    .as_ref()
    .ok_or_else(|| Refused::not_found("the service holds no console bundle"))
}

/// An answer of bytes under its type and the headers given.
fn bytes(body: Vec<u8>, headers: &[(header::HeaderName, &'static str)]) -> Response {
  let mut response = body.into_response();

  for (name, value) in headers {
    response.headers_mut().insert(name.clone(), HeaderValue::from_static(value));
  }

  response
}

/// `GET /console-manifest`: the manifest a console reads for no reader.
pub struct Manifest;

impl Route for Manifest {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/console-manifest";
  const OPERATION: Operation = Operation {
    id: "readConsoleManifest",
    summary: "The console bundle's manifest.",
    tag: "console",
    keys: &[],
    query: None,
    body: None,
    status: 200,
    answer: Answer::Json(schema::<ConsoleManifest>),
    codes: &["not-found"],
    guard: Guard::Token,
  };

  async fn answer(&self, call: Call) -> Result<Response, Refused> {
    Ok(answer(StatusCode::OK, &bundle_of(&call)?.manifest()))
  }
}

crate::route!(Manifest);

/// The bundle's words: each locale's tree under the locale's code.
pub type ConsoleWords = BTreeMap<String, serde_json::Value>;

/// `GET /console-words`: the bundle's words, which a console reads for no reader.
pub struct Words;

impl Route for Words {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = WORDS;
  const OPERATION: Operation = Operation {
    id: "readConsoleWords",
    summary: "The console bundle's words, each locale's tree under the locale's code.",
    tag: "console",
    keys: &[],
    query: None,
    body: None,
    status: 200,
    answer: Answer::Json(schema::<ConsoleWords>),
    codes: &["not-found"],
    guard: Guard::Token,
  };

  async fn answer(&self, call: Call) -> Result<Response, Refused> {
    let words = bundle_of(&call)?.words.clone();

    Ok(bytes(
      words,
      &[(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "no-cache")],
    ))
  }
}

crate::route!(Words);

/// `GET /console-bundle/{file}`: the module or the stylesheet, at the name that follows its bytes.
pub struct File;

impl Route for File {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/console-bundle/{file}";
  const OPERATION: Operation = Operation {
    id: "readConsoleBundleByFile",
    summary: "The console bundle's module or stylesheet.",
    tag: "console",
    keys: &[("file", "The module's or the stylesheet's name, as the manifest gives it.")],
    query: None,
    body: None,
    status: 200,
    answer: Answer::Bytes,
    codes: &["not-found"],
    guard: Guard::Screen,
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let name = call.key("file").await?;
    let (served, kind) = bundle_of(&call)?
      .file(&name)
      .ok_or_else(|| Refused::not_found(format!("the console bundle has no file {name}")))?;

    Ok(bytes(
      served.bytes.clone(),
      &[
        (header::CONTENT_TYPE, kind),
        (header::CACHE_CONTROL, "private, max-age=31536000, immutable"),
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
      ],
    ))
  }
}

crate::route!(File);
