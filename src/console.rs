//! The service's console face (`docs/api.md`, 「The console plugin bundle」): its declaration and the bundle
//! build's folder, handed to bmo-chat's console face, which answers the manifest and the bundle's files and
//! guards them by bmo-ui's console plugin contract.

use std::path::Path;

use bmo_console_face::{BundleFolder, ConsoleFace};

use crate::settings::{Console, Settings};
use crate::wire::Role;

/// The least role a reader holds to open the screen, and so the floor of every route the screen uses. The
/// declaration names the same role.
pub const SCREEN_ROLE: Role = Role::Member;

/// Every manifest field the service decides.
pub const DECLARATION: &str = include_str!("console.json");

/// The files the bundle build writes into the console folder that the manifest names.
const MODULE_FILE: &str = "module.js";
const STYLESHEET_FILE: &str = "style.css";
const WORDS_FILE: &str = "words.json";

/// The console face the settings make: none where they name no console credential or the console folder holds
/// no build, and the face's refusal where the build or the declaration breaks the contract.
pub fn face(console: &Console) -> Result<Option<ConsoleFace>, String> {
  if console.token.is_empty() || !console.dir.join(MODULE_FILE).is_file() {
    return Ok(None);
  }

  ConsoleFace::new(DECLARATION, &folder(&console.dir), &console.token)
    .map(Some)
    .map_err(|why| format!("the console face is refused: {why}"))
}

/// The bundle build's folder, its stylesheet named where the build wrote one.
fn folder(dir: &Path) -> BundleFolder {
  let folder = BundleFolder::new(dir, MODULE_FILE, WORDS_FILE);

  if dir.join(STYLESHEET_FILE).is_file() {
    folder.with_stylesheet(STYLESHEET_FILE)
  } else {
    folder
  }
}

/// How a console reaches the service, as the testbed's console probe asks it: the base address, the console's
/// credential and the service's own token (`docs/api.md`, 「The command」).
pub fn reach(settings: &Settings) -> serde_json::Value {
  serde_json::json!({
    "address": format!("http://{}", settings.server.listen),
    "consoleToken": settings.console.token,
    "token": settings.server.token,
  })
}
