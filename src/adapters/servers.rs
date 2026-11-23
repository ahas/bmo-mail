//! The servers an account that signs in by password names, as its fields hold them (`docs/mail.md`,
//! 「Accounts and credentials」): each server's host, port and the name its certificate is checked against, and
//! the login and password the account signs in with. A connection to one carries keep-alive probes and runs over
//! TLS checked against the web's root certificates, or plain where the TLS name is `-` (`docs/mail.md`,
//! 「Connections and the watcher」).

use std::fmt::{self, Debug};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use socket2::{SockRef, TcpKeepalive};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};

use crate::routes::requests::{Asked, MailFault};
use crate::wire::{FieldKind, Fields, KindField};

/// The TLS name that means a plain connection, which only a stand-in server uses.
pub const PLAIN: &str = "-";
/// The fields every such account names its login by.
pub const LOGIN: &str = "login";
pub const PASSWORD: &str = "password";
/// The fields every such account names its submission server by.
pub const SMTP_HOST: &str = "smtpHost";
pub const SMTP_PORT: &str = "smtpPort";
pub const SMTP_TLS_NAME: &str = "smtpTlsName";

/// How long a connection to a mail server stays silent before the first keep-alive probe asks whether the
/// server is still there, and how far apart the probes after it go.
const KEEP_ALIVE: Duration = Duration::from_secs(30);
const KEEP_ALIVE_PROBES: Duration = Duration::from_secs(10);

/// What a connection runs over: TLS, or a plain stream where the account's TLS name is `-`.
pub trait MailStream: AsyncRead + AsyncWrite + Unpin + Send + Debug {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send + Debug> MailStream for T {}

/// TLS checked against the web's root certificates, as one client configuration for every connection.
static TLS: LazyLock<TlsConnector> = LazyLock::new(|| {
  let roots = RootCertStore {
    roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
  };
  let provider = Arc::new(tokio_rustls::rustls::crypto::ring::default_provider());
  let config = ClientConfig::builder_with_provider(provider)
    .with_safe_default_protocol_versions()
    .expect("the ring provider speaks the default protocol versions")
    .with_root_certificates(roots)
    .with_no_client_auth();

  TlsConnector::from(Arc::new(config))
});

/// TLS over a connection already open, its certificate checked against `name`.
pub async fn tls(stream: TcpStream, name: &str) -> Result<Box<dyn MailStream>, MailFault> {
  let server = ServerName::try_from(name.to_owned())
    .map_err(|why| MailFault::server(format!("the TLS name {name} is not one: {why}")))?;
  let stream = TLS.connect(server, stream).await.map_err(MailFault::server)?;

  Ok(Box::new(stream))
}

/// A server an account names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MailEndpoint {
  pub host: String,
  pub port: u16,
  /// The name the server's certificate is checked against, or none for a plain connection, which only a
  /// stand-in server uses.
  pub tls_name: Option<String>,
}

impl MailEndpoint {
  /// A TCP connection to the server, carrying keep-alive probes so a server that is gone breaks it
  /// (`docs/mail.md`, 「Connections and the watcher」).
  pub async fn connect(&self) -> Result<TcpStream, MailFault> {
    let tcp = TcpStream::connect((self.host.as_str(), self.port)).await.map_err(MailFault::server)?;
    let probes = TcpKeepalive::new().with_time(KEEP_ALIVE).with_interval(KEEP_ALIVE_PROBES);

    SockRef::from(&tcp).set_tcp_keepalive(&probes).map_err(MailFault::server)?;

    Ok(tcp)
  }

  /// A connection to the server that speaks TLS from its first byte, or plain where the TLS name is `-`.
  pub async fn connect_secured(&self) -> Result<Box<dyn MailStream>, MailFault> {
    let tcp = self.connect().await?;

    match self.tls_name.as_deref() {
      None => Ok(Box::new(tcp)),
      Some(name) => tls(tcp, name).await,
    }
  }

  /// A server of the settings: its host, its port, and its TLS name — the host where none is written, none
  /// where it is `-`.
  pub fn of(settings: &Fields, host: &str, port: &str, tls_name: &str) -> Result<Self, MailFault> {
    let unread = || MailFault::server("the account's servers do not read");
    let host = settings.get(host).filter(|host| !host.is_empty()).ok_or_else(unread)?.clone();
    let port = settings
      .get(port)
      .and_then(|port| port.parse::<u16>().ok())
      .filter(|port| *port > 0)
      .ok_or_else(unread)?;
    let tls_name = match settings.get(tls_name).map(String::as_str) {
      Some(PLAIN) => None,
      Some(name) if !name.is_empty() => Some(name.to_owned()),
      _ => Some(host.clone()),
    };

    Ok(Self {
      host,
      port,
      tls_name,
    })
  }

  /// The TLS name as a field keeps it: `-` for a plain connection.
  pub fn tls_field(&self) -> String {
    self.tls_name.clone().unwrap_or_else(|| PLAIN.to_owned())
  }
}

/// An account's login to its servers.
#[derive(Clone, PartialEq, Eq)]
pub struct MailAccount {
  pub address: String,
  pub login: String,
  pub password: String,
}

// A login written into a log names its address alone.
impl Debug for MailAccount {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter
      .debug_struct("MailAccount")
      .field("address", &self.address)
      .finish_non_exhaustive()
  }
}

impl MailAccount {
  /// The login of an account's fields: the login written, else the address, and its password.
  pub fn of(address: &str, settings: &Fields, secrets: &Fields) -> Result<Self, MailFault> {
    let login = settings
      .get(LOGIN)
      .filter(|login| !login.is_empty())
      .cloned()
      .unwrap_or_else(|| address.to_owned());
    let password = secrets
      .get(PASSWORD)
      .cloned()
      .ok_or_else(|| MailFault::server("the account has no password"))?;

    Ok(Self {
      address: address.to_owned(),
      login,
      password,
    })
  }
}

/// One field a kind declares.
pub fn field(name: &str, kind: FieldKind, required: bool, default: Option<&str>) -> KindField {
  KindField {
    name: name.to_owned(),
    kind,
    required,
    default: default.map(str::to_owned),
  }
}

/// A sign-in the servers refused, said in the log with why and answered as `sign-in-failed`, its reason kept
/// out of the answer.
pub fn refused_sign_in(address: &str, why: MailFault) -> MailFault {
  let why = match why {
    MailFault::Asked(asked) => asked.error,
    MailFault::Server(why) => why,
  };

  eprintln!("mail: an account of {address} did not sign in: {why}");

  MailFault::from(Asked::rule("sign-in-failed", "the account's servers did not take its sign-in"))
}
