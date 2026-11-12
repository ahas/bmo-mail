//! The settings (`docs/api.md`, 「Settings」): the defaults built in from `settings.toml`, then the
//! settings folder's `settings.toml` and `settings.local.toml` where they stand, then the environment. A
//! relative path among them is read from the settings folder.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use config::{Config, Environment, File, FileFormat};
use serde::Deserialize;

/// The defaults, as the repository's own settings file states them.
const DEFAULTS: &str = include_str!("../settings.toml");
/// The settings file a deployment may give.
pub const SETTINGS_FILE: &str = "settings.toml";
/// The file a deployment overrides it with.
pub const LOCAL_SETTINGS_FILE: &str = "settings.local.toml";
/// The environment variable naming the settings folder.
pub const SETTINGS_FOLDER: &str = "BMO_MAIL_SETTINGS";
/// What every environment variable that sets a key begins with.
pub const ENVIRONMENT_PREFIX: &str = "BMO_MAIL";

/// Everything the service and its command are started with.
#[derive(Clone, Debug, Deserialize)]
pub struct Settings {
  /// The settings folder they were read from, which a relative path is read from.
  #[serde(skip)]
  pub folder: PathBuf,
  pub server: Server,
  pub store: Store,
  pub accounts: Accounts,
  pub secrets: Secrets,
  pub staging: Staging,
  pub wording: Wording,
  pub console: Console,
}

/// The HTTP server.
#[derive(Clone, Debug, Deserialize)]
pub struct Server {
  /// Where the service answers, a loopback address by default.
  pub listen: SocketAddr,
  /// The bearer token every caller presents; an empty one lets no caller in.
  pub token: String,
}

/// The store of labels, rules, settings, marks, known addresses and sender logos.
#[derive(Clone, Debug, Deserialize)]
pub struct Store {
  /// The SQLite file.
  pub path: PathBuf,
}

/// The accounts file imported once, and the administrator's owner key (`docs/mail.md`, 「The accounts file,
/// imported once」).
#[derive(Clone, Debug, Deserialize)]
pub struct Accounts {
  /// The file; an empty one is none, and nothing is imported.
  #[serde(default)]
  pub file: Option<PathBuf>,
  /// The administrator's owner key; empty where none is named.
  #[serde(default)]
  pub administrator: String,
}

/// The key file an account's secrets are sealed with (`docs/mail.md`, 「Accounts and credentials」).
#[derive(Clone, Debug, Deserialize)]
pub struct Secrets {
  /// The file; an empty one is none, and the service then keeps no account.
  #[serde(default)]
  pub key_file: Option<PathBuf>,
}

/// The folder uploads wait in until the message carrying them is saved or sent.
#[derive(Clone, Debug, Deserialize)]
pub struct Staging {
  pub dir: PathBuf,
}

/// The console face the service serves (`docs/api.md`, 「The console plugin bundle」).
#[derive(Clone, Debug, Deserialize)]
pub struct Console {
  /// The bundle build's folder: its module, its stylesheet, its words and the layer it was built on.
  pub dir: PathBuf,
  /// The console's credential, which reaches the console face and the routes the screen uses; an empty one
  /// serves no console face.
  #[serde(default)]
  pub token: String,
}

/// The service's own words (`docs/api.md`, 「The service's own words」).
#[derive(Clone, Debug, Deserialize)]
pub struct Wording {
  /// The language the words are written in and a list is ordered by.
  pub language: String,
  /// The time zone dates are written in.
  pub time_zone: String,
}

impl Settings {
  /// The settings of this process: its settings folder, then its environment.
  pub fn load() -> Result<Self, SettingsError> {
    Self::read(&folder(), None)
  }

  /// The settings read from `dir`, then from `environment` where it is given and from the process's own
  /// environment where it is not, with every relative path read from `dir`.
  pub fn read(dir: &Path, environment: Option<HashMap<String, String>>) -> Result<Self, SettingsError> {
    let environment = Environment::with_prefix(ENVIRONMENT_PREFIX)
      .prefix_separator("_")
      .separator("__")
      .try_parsing(true)
      .source(environment);
    let mut settings: Self = Config::builder()
      .add_source(File::from_str(DEFAULTS, FileFormat::Toml))
      .add_source(File::from(dir.join(SETTINGS_FILE)).format(FileFormat::Toml).required(false))
      .add_source(File::from(dir.join(LOCAL_SETTINGS_FILE)).format(FileFormat::Toml).required(false))
      .add_source(environment)
      .build()?
      .try_deserialize()?;

    settings.folder = dir.to_path_buf();
    settings.store.path = dir.join(&settings.store.path);
    settings.staging.dir = dir.join(&settings.staging.dir);
    settings.console.dir = dir.join(&settings.console.dir);
    settings.accounts.file = settings
      .accounts
      .file
      .filter(|file| !file.as_os_str().is_empty())
      .map(|file| dir.join(file));
    settings.secrets.key_file = settings
      .secrets
      .key_file
      .filter(|file| !file.as_os_str().is_empty())
      .map(|file| dir.join(file));

    Ok(settings)
  }
}

/// The settings folder: the one the environment names, else the folder above the one holding the binary,
/// a deployment's root.
fn folder() -> PathBuf {
  std::env::var_os(SETTINGS_FOLDER)
    .filter(|named| !named.is_empty())
    .map(PathBuf::from)
    .or_else(|| {
      let binary = std::env::current_exe().ok()?;

      Some(binary.parent()?.parent()?.to_path_buf())
    })
    .unwrap_or_else(|| PathBuf::from("."))
}

/// Why the settings could not be read.
#[derive(Debug, thiserror::Error)]
#[error("the settings could not be read: {0}")]
pub struct SettingsError(#[from] config::ConfigError);
