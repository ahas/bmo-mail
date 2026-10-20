//! The mail accounts file (`docs/mail.md`, 「Accounts and credentials」): the IMAP and submission servers
//! and one login per account. It is a secret, so nothing read from it but an account's address ever leaves
//! this module's callers for a log line or an answer.
//!
//! Its line syntax is this service's own: `imap <host> <port> <tls name>`, `smtp <host> <port> <tls name>`
//! and `account <address> <login> <password>`, with blank lines and `#` lines passed over.

use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// A server the accounts file names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MailEndpoint {
  pub host: String,
  pub port: u16,
  /// The name the server's certificate is checked against, or none for a plain connection, which only a
  /// stand-in server uses.
  pub tls_name: Option<String>,
}

/// One account of the accounts file.
#[derive(Clone, PartialEq, Eq)]
pub struct MailAccount {
  pub address: String,
  /// The address's local part: the account's name, which the routes name it by.
  pub key: String,
  pub login: String,
  pub password: String,
}

// An account written into a log names its address alone.
impl fmt::Debug for MailAccount {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter
      .debug_struct("MailAccount")
      .field("address", &self.address)
      .finish_non_exhaustive()
  }
}

/// The accounts file, read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MailConfig {
  pub imap: MailEndpoint,
  pub smtp: MailEndpoint,
  pub accounts: Vec<MailAccount>,
}

impl MailConfig {
  /// The account a name names, matched without regard to case.
  pub fn find_account(&self, key: &str) -> Option<&MailAccount> {
    let key = key.to_lowercase();

    self.accounts.iter().find(|account| account.key == key)
  }
}

/// Why the accounts file does not read. The fault names its line and never repeats it, which may hold a
/// password.
#[derive(Debug, thiserror::Error)]
#[error("mail accounts file{0}")]
pub struct MailAccountsError(String);

fn fault(line: usize, what: impl fmt::Display) -> MailAccountsError {
  MailAccountsError(format!(", line {line}: {what}"))
}

fn endpoint(words: &[&str], line: usize) -> Result<MailEndpoint, MailAccountsError> {
  let shape = || fault(line, "a server line is <host> <port> <tls name>");
  let [host, port, tls_name] = words else {
    return Err(shape());
  };
  let port = port.parse::<u16>().ok().filter(|port| *port > 0).ok_or_else(shape)?;

  Ok(MailEndpoint {
    host: (*host).to_owned(),
    port,
    tls_name: (*tls_name != "-").then(|| (*tls_name).to_owned()),
  })
}

/// The local part of an address of the shape `<local>@<domain>`, neither holding `@` nor a space.
fn local_part(address: &str) -> Option<&str> {
  let (local, domain) = address.split_once('@')?;
  let plain = |part: &str| !part.is_empty() && !part.contains('@') && !part.contains(char::is_whitespace);

  (plain(local) && plain(domain)).then_some(local)
}

/// Reads the file's text.
pub fn parse_accounts_file(text: &str) -> Result<MailConfig, MailAccountsError> {
  let mut imap = None;
  let mut smtp = None;
  let mut accounts: Vec<MailAccount> = Vec::new();

  for (index, raw) in text.split('\n').enumerate() {
    let line = raw.trim();
    let number = index + 1;

    if line.is_empty() || line.starts_with('#') {
      continue;
    }

    let words: Vec<&str> = line.split_whitespace().collect();

    match words.split_first() {
      Some((&"imap", rest)) => imap = Some(endpoint(rest, number)?),
      Some((&"smtp", rest)) => smtp = Some(endpoint(rest, number)?),
      Some((&"account", rest)) => {
        let shape = || fault(number, "an account line is <address> <login> <password>");
        let [address, login, password] = rest else {
          return Err(shape());
        };
        let key = local_part(address).ok_or_else(shape)?.to_lowercase();

        if accounts.iter().any(|one| one.key == key) {
          return Err(fault(number, format!("a second account named {key}")));
        }

        accounts.push(MailAccount {
          address: address.to_lowercase(),
          key,
          login: (*login).to_owned(),
          password: (*password).to_owned(),
        });
      }
      _ => return Err(fault(number, "unknown line kind")),
    }
  }

  let (Some(imap), Some(smtp)) = (imap, smtp) else {
    return Err(MailAccountsError(": it needs an imap line and an smtp line".to_owned()));
  };

  if accounts.is_empty() {
    return Err(MailAccountsError(": it names no account".to_owned()));
  }

  Ok(MailConfig {
    imap,
    smtp,
    accounts,
  })
}

/// The file as last read: when it was changed, and what it said.
#[derive(Debug)]
struct Read {
  changed: Option<SystemTime>,
  config: Option<Arc<MailConfig>>,
}

/// The accounts file the settings name, read again only when it changed, so writing it takes effect
/// without a restart.
#[derive(Debug)]
pub struct AccountsFile {
  path: Option<PathBuf>,
  last: Mutex<Option<Read>>,
}

impl AccountsFile {
  pub fn new(path: Option<PathBuf>) -> Self {
    Self {
      path,
      last: Mutex::new(None),
    }
  }

  /// The accounts the service reads, or none where no file is named, or the named one is missing or does
  /// not read. A file that exists but does not read is said in the log once per change of the file.
  pub fn read(&self) -> Option<Arc<MailConfig>> {
    let path = self.path.as_ref()?;
    let mut last = self.last.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Ok(changed) = std::fs::metadata(path).map(|meta| meta.modified().ok()) else {
      *last = None;

      return None;
    };

    if let Some(read) = last.as_ref().filter(|read| read.changed.is_some() && read.changed == changed) {
      return read.config.clone();
    }

    let config = match std::fs::read_to_string(path) {
      Ok(text) => match parse_accounts_file(&text) {
        Ok(config) => Some(Arc::new(config)),
        Err(why) => {
          eprintln!("{why}");
          None
        }
      },
      Err(_) => {
        eprintln!("mail accounts file: {} could not be read", path.display());
        None
      }
    };

    *last = Some(Read {
      changed,
      config: config.clone(),
    });

    config
  }
}
