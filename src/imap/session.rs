//! One account's work connection (`docs/mail.md`, 「Connections and the watcher」): the connection the
//! routes go through, one folder at a time. It is opened when first needed and opened again when it has
//! dropped; the watcher's connection is a separate one of its own.

use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use async_imap::{Client, Session};
use futures::future::BoxFuture;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};

use crate::accounts::{MailAccount, MailEndpoint};
use crate::routes::requests::MailFault;

/// What a connection runs over: TLS, or a plain stream where the accounts file says `-`.
pub trait MailStream: AsyncRead + AsyncWrite + Unpin + Send + Debug {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send + Debug> MailStream for T {}

/// An IMAP session, logged in.
pub type ImapSession = Session<Box<dyn MailStream>>;

/// The port IMAP is spoken over TLS from the first byte; any other port with a TLS name is upgraded with
/// STARTTLS.
const IMPLICIT_TLS_PORT: u16 = 993;
// How long a connection may take to open, greeting and login included.
const OPENING: Duration = Duration::from_secs(90);
/// The name the service gives the server where the server asks with `ID`.
const CLIENT_NAME: &str = "bmo-mail";

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

async fn tls(stream: TcpStream, name: &str) -> Result<Box<dyn MailStream>, MailFault> {
  let server = ServerName::try_from(name.to_owned())
    .map_err(|why| MailFault::server(format!("the TLS name {name} is not one: {why}")))?;
  let stream = TLS.connect(server, stream).await.map_err(MailFault::server)?;

  Ok(Box::new(stream))
}

/// Opens one connection of an account and logs in: over TLS on port 993, upgraded with STARTTLS on another
/// port with a TLS name, and plain where the TLS name is `-`.
pub async fn open(endpoint: &MailEndpoint, account: &MailAccount) -> Result<ImapSession, MailFault> {
  let opening = async {
    let tcp = TcpStream::connect((endpoint.host.as_str(), endpoint.port))
      .await
      .map_err(MailFault::server)?;
    let client: Client<Box<dyn MailStream>> = match endpoint.tls_name.as_deref() {
      None => greeted(Box::new(tcp)).await?,
      Some(name) if endpoint.port == IMPLICIT_TLS_PORT => greeted(tls(tcp, name).await?).await?,
      Some(name) => {
        let mut plain = Client::new(tcp);

        plain.read_response().await.map_err(MailFault::server)?;
        plain.run_command_and_check_ok("STARTTLS", None).await.map_err(MailFault::server)?;

        // After STARTTLS the server greets no more; the conversation goes on over TLS.
        Client::new(tls(plain.into_inner(), name).await?)
      }
    };
    let mut session = client
      .login(&account.login, &account.password)
      .await
      .map_err(|(why, _)| MailFault::server(why))?;

    if session.capabilities().await.map_err(MailFault::server)?.has_str("ID") {
      session.id([("name", Some(CLIENT_NAME))]).await.map_err(MailFault::server)?;
    }

    Ok(session)
  };

  tokio::time::timeout(OPENING, opening)
    .await
    .map_err(|_| MailFault::server("the mail server did not open a connection in time"))?
}

/// A client whose server's greeting has been read.
async fn greeted(stream: Box<dyn MailStream>) -> Result<Client<Box<dyn MailStream>>, MailFault> {
  let mut client = Client::new(stream);

  client.read_response().await.map_err(MailFault::server)?;

  Ok(client)
}

/// The open connection and the folder it has selected.
pub struct Open {
  pub session: ImapSession,
  selected: Option<String>,
}

impl Open {
  /// Selects `path` unless it is selected already.
  pub async fn select(&mut self, path: &str) -> Result<(), MailFault> {
    if self.selected.as_deref() != Some(path) {
      self.selected = None;
      self.session.select(path).await.map_err(MailFault::server)?;
      self.selected = Some(path.to_owned());
    }

    Ok(())
  }
}

/// A folder held for work that outlives the call: a part streamed to the browser holds the folder until the
/// stream ends.
pub struct Held {
  guard: OwnedMutexGuard<Option<Open>>,
}

impl Held {
  pub fn session(&mut self) -> &mut ImapSession {
    &mut self.guard.as_mut().expect("a held folder has its connection open").session
  }
}

/// One account's work connection, one piece of work at a time.
pub struct MailSession {
  pub endpoint: MailEndpoint,
  pub account: MailAccount,
  open: Arc<AsyncMutex<Option<Open>>>,
}

impl Debug for MailSession {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter
      .debug_struct("MailSession")
      .field("account", &self.account)
      .finish_non_exhaustive()
  }
}

impl MailSession {
  fn new(endpoint: MailEndpoint, account: MailAccount) -> Self {
    Self {
      endpoint,
      account,
      open: Arc::new(AsyncMutex::new(None)),
    }
  }

  /// The connection held for this caller alone, opened now if it is not.
  async fn connection(&self) -> Result<OwnedMutexGuard<Option<Open>>, MailFault> {
    let mut guard = Arc::clone(&self.open).lock_owned().await;

    if guard.is_none() {
      let session = open(&self.endpoint, &self.account).await?;

      *guard = Some(Open {
        session,
        selected: None,
      });
    }

    Ok(guard)
  }

  /// Runs the work with the folder selected, holding it for that work alone. A folder that was already
  /// selected is brought up to date first: the server takes mail that arrived since into a connection's
  /// view only at a command, so without that the first search after new mail would miss it while the
  /// message count, reported alongside, already counts it. A connection the work failed on is opened again
  /// by the next.
  pub async fn in_folder<T>(
    &self,
    path: &str,
    work: impl for<'s> FnOnce(&'s mut ImapSession) -> BoxFuture<'s, Result<T, MailFault>>,
  ) -> Result<T, MailFault> {
    let mut guard = self.connection().await?;
    let open = guard.as_mut().expect("the connection was just opened");
    let done = async {
      open.select(path).await?;
      open.session.noop().await.map_err(MailFault::server)?;

      work(&mut open.session).await
    }
    .await;

    if matches!(done, Err(MailFault::Server(_))) {
      *guard = None;
    }

    done
  }

  /// Runs the work on the connection with no folder chosen for it: listing folders, asking a folder's
  /// state.
  pub async fn with_connection<T>(
    &self,
    work: impl for<'s> FnOnce(&'s mut Open) -> BoxFuture<'s, Result<T, MailFault>>,
  ) -> Result<T, MailFault> {
    let mut guard = self.connection().await?;
    let done = work(guard.as_mut().expect("the connection was just opened")).await;

    if matches!(done, Err(MailFault::Server(_))) {
      *guard = None;
    }

    done
  }

  /// Selects the folder and hands it to the caller, held until the caller lets it go.
  pub async fn hold_folder(&self, path: &str) -> Result<Held, MailFault> {
    let mut guard = self.connection().await?;
    let selected = guard.as_mut().expect("the connection was just opened").select(path).await;

    if selected.is_err() {
      *guard = None;
    }

    selected.map(|()| Held { guard })
  }

  /// Logs the connection out, or drops it where it will not.
  pub async fn close(&self) {
    let open = self.open.lock().await.take();

    if let Some(mut open) = open {
      let _ = open.session.logout().await;
    }
  }
}

/// Every account's work connection, kept for as long as the service runs.
#[derive(Debug, Default)]
pub struct Sessions {
  by_address: Mutex<HashMap<String, Arc<MailSession>>>,
}

impl Sessions {
  /// The session of one account. A changed accounts file — a new password, another server — makes a new
  /// session rather than reusing one opened with the old words.
  pub fn session(&self, endpoint: &MailEndpoint, account: &MailAccount) -> Arc<MailSession> {
    let mut by_address = self.by_address.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

    if let Some(known) = by_address.get(&account.address)
      && known.endpoint == *endpoint
      && known.account == *account
    {
      return Arc::clone(known);
    }

    let session = Arc::new(MailSession::new(endpoint.clone(), account.clone()));

    if let Some(stale) = by_address.insert(account.address.clone(), Arc::clone(&session)) {
      tokio::spawn(async move { stale.close().await });
    }

    session
  }

  /// Logs every session out, as the service stops.
  pub async fn close_all(&self) {
    let all: Vec<Arc<MailSession>> = {
      let mut by_address = self.by_address.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

      by_address.drain().map(|(_, session)| session).collect()
    };

    futures::future::join_all(all.iter().map(|session| session.close())).await;
  }
}
