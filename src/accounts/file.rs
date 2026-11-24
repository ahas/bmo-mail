//! The mail accounts file (`docs/mail.md`, 「The accounts file, imported once」): the IMAP and submission
//! servers and one login per account, read once at start until its accounts are imported. It is a secret, so
//! nothing read from it but an account's address ever leaves this module's callers for a log line or an
//! answer.
//!
//! Its line syntax is this service's own: `imap <host> <port> <tls name>`, `smtp <host> <port> <tls name>`
//! and `account <address> <login> <password>`, with blank lines and `#` lines passed over.

use std::fmt;

use crate::adapters::servers::MailAccount;
use crate::servers::{ServerDraft, ServerFields};
use crate::wire::Protocol;

/// One account of the accounts file: its login, and the address's local part, the name it is kept under.
#[derive(Clone, PartialEq, Eq)]
pub struct FileAccount {
  /// The address's local part.
  pub key: String,
  pub account: MailAccount,
}

// An account written into a log names its address alone.
impl fmt::Debug for FileAccount {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter
      .debug_struct("FileAccount")
      .field("address", &self.account.address)
      .finish_non_exhaustive()
  }
}

/// The accounts file, read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MailConfig {
  pub imap: ServerDraft,
  pub smtp: ServerDraft,
  pub accounts: Vec<FileAccount>,
}

/// Why the accounts file does not read. The fault names its line and never repeats it, which may hold a
/// password.
#[derive(Debug, thiserror::Error)]
#[error("mail accounts file{0}")]
pub struct MailAccountsError(String);

fn fault(line: usize, what: impl fmt::Display) -> MailAccountsError {
  MailAccountsError(format!(", line {line}: {what}"))
}

/// A server line read as the server it names, its security read as the line writes it (`docs/mail.md`,
/// 「Mail servers」).
fn server(protocol: Protocol, words: &[&str], line: usize) -> Result<ServerDraft, MailAccountsError> {
  let shape = || fault(line, "a server line is <host> <port> <tls name>");
  let [host, port, tls_name] = words else {
    return Err(shape());
  };
  let port = port.parse::<u16>().ok().filter(|port| *port > 0).ok_or_else(shape)?;
  let plain = *tls_name == "-";
  let fields = ServerFields {
    host: Some((*host).to_owned()),
    port: Some(port),
    security: Some(protocol.security_as_written(port, plain)),
    tls_name: (!plain).then(|| (*tls_name).to_owned()),
    ..ServerFields::default()
  };

  ServerDraft::checked(protocol, fields).map_err(|why| fault(line, why.error))
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
  let mut accounts: Vec<FileAccount> = Vec::new();

  for (index, raw) in text.split('\n').enumerate() {
    let line = raw.trim();
    let number = index + 1;

    if line.is_empty() || line.starts_with('#') {
      continue;
    }

    let words: Vec<&str> = line.split_whitespace().collect();

    match words.split_first() {
      Some((&"imap", rest)) => imap = Some(server(Protocol::Imap, rest, number)?),
      Some((&"smtp", rest)) => smtp = Some(server(Protocol::Smtp, rest, number)?),
      Some((&"account", rest)) => {
        let shape = || fault(number, "an account line is <address> <login> <password>");
        let [address, login, password] = rest else {
          return Err(shape());
        };
        let key = local_part(address).ok_or_else(shape)?.to_lowercase();

        if accounts.iter().any(|one| one.key == key) {
          return Err(fault(number, format!("a second account named {key}")));
        }

        accounts.push(FileAccount {
          key,
          account: MailAccount {
            address: address.to_lowercase(),
            login: (*login).to_owned(),
            password: (*password).to_owned(),
          },
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

/// The file at `path`, read: none where it does not stand, and why it does not read where it does not.
pub fn read_accounts_file(path: &std::path::Path) -> Result<Option<MailConfig>, MailAccountsError> {
  match std::fs::read_to_string(path) {
    Ok(text) => parse_accounts_file(&text).map(Some),
    Err(why) if why.kind() == std::io::ErrorKind::NotFound => Ok(None),
    Err(_) => Err(MailAccountsError(format!(" {} could not be read", path.display()))),
  }
}
