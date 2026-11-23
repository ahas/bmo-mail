//! The settings (`docs/api.md`, 「Settings」): the defaults built in from `settings.toml`, then the
//! settings folder's `settings.toml` and `settings.local.toml` where they stand, then the environment. A
//! relative path among them is read from the settings folder.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use config::{Config, Environment, File, FileFormat};
use serde::Deserialize;

/// The defaults, as the repository's own settings file states them.
const DEFAULTS: &str = include_str!("../settings.toml");
/// The settings file a deployment may give.
pub const SETTINGS_FILE: &str = "settings.toml";
/// The file a deployment overrides it with.
pub const LOCAL_SETTINGS_FILE: &str = "settings.local.toml";
/// The data root's folder the deployment's settings stand in, as the hosted tool reads them.
pub const DEPLOYED_FOLDER: &str = "env";
/// The deployment's settings file there, each release's `settings.local.toml`.
pub const DEPLOYED_FILE: &str = "mail.toml";
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
  pub watcher: Watcher,
  pub pop3: Pop3,
  pub wording: Wording,
  pub console: Console,
  pub agency: Upstream,
  pub receiver: Upstream,
  pub account_service: Upstream,
  pub outlook: Outlook,
  pub node: Upstream,
  pub ms_teams: Upstream,
  pub graph: Graph,
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

/// The account watchers (`docs/mail.md`, 「Connections and the watcher」).
#[derive(Clone, Debug, Deserialize)]
pub struct Watcher {
  /// The first wait, in milliseconds, before a dropped watch connection is opened again.
  pub retry_ms: u64,
}

impl Watcher {
  /// The first wait before a dropped watch connection is opened again.
  pub fn retry(&self) -> Duration {
    Duration::from_millis(self.retry_ms)
  }
}

/// The POP3 accounts (`docs/mail.md`, 「A POP3 account」).
#[derive(Clone, Debug, Deserialize)]
pub struct Pop3 {
  /// How often, in milliseconds, a POP3 account's watcher opens a session to look for new mail.
  pub poll_ms: u64,
}

impl Pop3 {
  /// How often a POP3 account's watcher looks for new mail.
  pub fn poll(&self) -> Duration {
    Duration::from_millis(self.poll_ms)
  }
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

/// A unit the hosted tool calls (`docs/api.md`, 「The hosted tool」): its address and the token it presents
/// there, given itself or as the file holding it. An address or a token left empty is none, and the tool's road
/// through that unit does not stand.
#[derive(Clone, Debug, Deserialize)]
pub struct Upstream {
  #[serde(default)]
  pub address: String,
  /// The token itself; it stands before the file where both are given.
  #[serde(default)]
  pub token: String,
  #[serde(default)]
  pub token_file: Option<PathBuf>,
}

/// Where the Outlook road reads the owner's own account.
#[derive(Clone, Debug, Deserialize)]
pub struct Outlook {
  /// The node the owner's account reads on; empty where its agency record is read as any account's.
  #[serde(default)]
  pub owner_node: String,
  /// The account the owner is known by to agency's node records, the node service and bmo-ms-teams; empty where
  /// it is his own.
  #[serde(default)]
  pub owner_account: String,
}

/// Microsoft Graph, which the hosted tool reads a person's sent items from with their page token.
#[derive(Clone, Debug, Deserialize)]
pub struct Graph {
  /// Its address, its version included.
  pub address: String,
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
    Self::layered(dir, &[dir.join(SETTINGS_FILE), dir.join(LOCAL_SETTINGS_FILE)], environment)
  }

  /// The settings the hosted tool reads (`docs/api.md`, 「The hosted tool」): the defaults, then the data root's
  /// `env/mail.toml`, the deployment's own local settings, with every relative path read from that folder.
  pub fn deployed(data_root: &Path) -> Result<Self, SettingsError> {
    let folder = data_root.join(DEPLOYED_FOLDER);
    let local = folder.join(DEPLOYED_FILE);

    Self::layered(&folder, &[local], Some(HashMap::new()))
  }

  /// The defaults, then each of `files` that stands, in order, then the environment, with every relative path
  /// read from `dir`.
  fn layered(
    dir: &Path,
    files: &[PathBuf],
    environment: Option<HashMap<String, String>>,
  ) -> Result<Self, SettingsError> {
    let environment = Environment::with_prefix(ENVIRONMENT_PREFIX)
      .prefix_separator("_")
      .separator("__")
      .try_parsing(true)
      .source(environment);
    let layers = files.iter().fold(
      Config::builder().add_source(File::from_str(DEFAULTS, FileFormat::Toml)),
      |layers, file| layers.add_source(File::from(file.as_path()).format(FileFormat::Toml).required(false)),
    );
    let mut settings: Self = layers.add_source(environment).build()?.try_deserialize()?;

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
    for upstream in [
      &mut settings.agency,
      &mut settings.receiver,
      &mut settings.account_service,
      &mut settings.node,
      &mut settings.ms_teams,
    ] {
      upstream.token_file = upstream
        .token_file
        .take()
        .filter(|file| !file.as_os_str().is_empty())
        .map(|file| dir.join(file));
    }

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
