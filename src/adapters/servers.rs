//! The servers an account that signs in by password is reached through (`docs/mail.md`, 「Mail servers」): each
//! server's host, port, security and the name its certificate is checked against, and the login and password the
//! account signs in with. A connection to one carries keep-alive probes and runs over TLS checked against the
//! web's root certificates, or plain where the server's security is none (`docs/mail.md`, 「Connections and the
//! watcher」).

use std::fmt::{self, Debug};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use socket2::{SockRef, TcpKeepalive};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};

use crate::accounts::Account;
use crate::routes::requests::{Asked, MailFault};
use crate::wire::{FieldKind, Fields, KindField, Security, ServerAuth};

/// The fields every such account names its login by.
pub const LOGIN: &str = "login";
pub const PASSWORD: &str = "password";

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

/// A server an account names, as a connection is opened to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MailEndpoint {
  pub host: String,
  pub port: u16,
  /// How the connection is secured.
  pub security: Security,
  /// The name the server's certificate is checked against.
  pub tls_name: String,
  /// How the server is signed in to.
  pub auth: ServerAuth,
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

  /// A connection that speaks TLS from its first byte, or a plain one, for a protocol whose client upgrades no
  /// session: POP3's.
  pub async fn connect_secured(&self) -> Result<Box<dyn MailStream>, MailFault> {
    let tcp = self.connect().await?;

    match self.security {
      Security::None => Ok(Box::new(tcp)),
      Security::Tls => tls(tcp, &self.tls_name).await,
      Security::Starttls => {
        Err(MailFault::server("the server's protocol upgrades no session with STARTTLS"))
      }
    }
  }

  /// Whether the server is signed in to with the account's login.
  pub fn signs_in(&self) -> bool {
    self.auth == ServerAuth::Password
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

/// An account's incoming server, outgoing server and login, as the servers it names and its fields hold them.
pub fn servers_of(account: &Account) -> Result<(MailEndpoint, MailEndpoint, MailAccount), MailFault> {
  let incoming = account
    .incoming
    .as_ref()
    .ok_or_else(|| MailFault::server("the account names no incoming server"))?;
  let outgoing = account
    .outgoing
    .as_ref()
    .ok_or_else(|| MailFault::server("the account names no outgoing server"))?;

  Ok((
    incoming.endpoint(),
    outgoing.endpoint(),
    MailAccount::of(&account.address, &account.settings, &account.secrets)?,
  ))
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
