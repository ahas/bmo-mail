//! One account's work connection (`docs/mail.md`, 「Connections and the watcher」): the connection the
//! routes go through, one folder at a time. It is opened when first needed and opened again when it has
//! dropped; the watcher's connection is a separate one of its own.

use std::fmt::Debug;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use async_imap::extensions::idle::IdleResponse;
use async_imap::imap_proto::{MailboxDatum, Response};
use async_imap::types::{Mailbox, UnsolicitedResponse};
use async_imap::{Client, Session};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};

use crate::adapters::imap_smtp::{MailAccount, MailEndpoint};
use crate::imap::wire_name;
use crate::routes::requests::MailFault;

/// What a connection runs over: TLS, or a plain stream where the account's TLS name is `-`.
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
struct Open {
  session: ImapSession,
  mailbox: Option<Mailbox>,
  selected: Option<String>,
}

/// The work connection held for one piece of work, the folder it selected with it. A piece of work ends
/// by settling what it did, so a connection a server fault was met on is opened again by the next.
pub struct Connection {
  guard: OwnedMutexGuard<Option<Open>>,
}

impl Connection {
  fn open(&mut self) -> &mut Open {
    self.guard.as_mut().expect("a held connection is open")
  }

  /// The IMAP session.
  pub fn session(&mut self) -> &mut ImapSession {
    &mut self.open().session
  }

  /// Selects `path`, read from the server now: its message count and UID validity as they stand.
  pub async fn select(&mut self, path: &str) -> Result<(), MailFault> {
    let open = self.open();

    open.mailbox = None;
    open.selected = None;
    open.mailbox = Some(open.session.select(wire_name(path)).await.map_err(MailFault::server)?);
    open.selected = Some(path.to_owned());

    Ok(())
  }

  /// The folder the connection has selected, by its path.
  pub fn selected(&mut self) -> Option<String> {
    self.open().selected.clone()
  }

  /// How many messages the selected folder holds.
  pub fn exists(&mut self) -> u32 {
    self.open().mailbox.as_ref().map_or(0, |mailbox| mailbox.exists)
  }

  /// The selected folder's UID validity, as text; empty where the server named none.
  pub fn uid_validity(&mut self) -> String {
    self
      .open()
      .mailbox
      .as_ref()
      .and_then(|mailbox| mailbox.uid_validity)
      .map(|v| v.to_string())
      .unwrap_or_default()
  }

  /// What the work came to, the connection let go of where the fault was the server's.
  pub fn settle<T>(mut self, done: Result<T, MailFault>) -> Result<T, MailFault> {
    if matches!(done, Err(MailFault::Server(_))) {
      *self.guard = None;
    }

    done
  }

  /// A connection of its own, held by its one user alone: the watcher's.
  pub async fn alone(session: ImapSession) -> Self {
    let open = Open {
      session,
      mailbox: None,
      selected: None,
    };
    let guard = Arc::new(AsyncMutex::new(Some(open))).lock_owned().await;

    Self { guard }
  }

  /// The next UID the selected folder will give, as the server said when it was selected.
  pub fn uid_next(&mut self) -> u32 {
    self.open().mailbox.as_ref().and_then(|mailbox| mailbox.uid_next).unwrap_or(0)
  }

  /// Stands in `IDLE` on the selected folder until the server says something or the wait ends, and says
  /// what woke it. The connection stays selected as it was.
  pub async fn idle(&mut self, wait: Duration) -> Result<Woke, MailFault> {
    let Some(open) = self.guard.take() else {
      return Err(MailFault::server("the connection is closed"));
    };
    let Open {
      session,
      mailbox,
      selected,
    } = open;
    let mut handle = session.idle();

    handle.init().await.map_err(MailFault::server)?;

    let outcome = {
      let (waiting, _interrupt) = handle.wait_with_timeout(wait);

      waiting.await.map_err(MailFault::server)?
    };
    let woke = match outcome {
      IdleResponse::NewData(response) => Woke::of(response.parsed()),
      IdleResponse::Timeout | IdleResponse::ManualInterrupt => Woke::Quiet,
    };
    let session = handle.done().await.map_err(MailFault::server)?;

    *self.guard = Some(Open {
      session,
      mailbox,
      selected,
    });

    Ok(woke)
  }

  /// Asks the server for the selected folder's news without standing in `IDLE`, for a server that has no
  /// `IDLE`, and says what it brought.
  pub async fn poll(&mut self) -> Result<Woke, MailFault> {
    let session = self.session();

    session.noop().await.map_err(MailFault::server)?;

    let mut woke = Woke::Quiet;

    while let Ok(said) = session.unsolicited_responses.try_recv() {
      woke = woke.and(match said {
        UnsolicitedResponse::Exists(_) | UnsolicitedResponse::Recent(_) => Woke::Grown,
        UnsolicitedResponse::Expunge(_) => Woke::Changed,
        UnsolicitedResponse::Other(response) => Woke::of(response.parsed()),
        _ => Woke::Quiet,
      });
    }

    Ok(woke)
  }
}

/// What the server said of the folder a connection watches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Woke {
  /// Nothing: the wait ran out.
  Quiet,
  /// A flag changed, or a message went.
  Changed,
  /// The folder grew.
  Grown,
}

impl Woke {
  fn of(response: &Response<'_>) -> Self {
    match response {
      Response::MailboxData(MailboxDatum::Exists(_) | MailboxDatum::Recent(_)) => Self::Grown,
      Response::Expunge(_) | Response::Vanished { .. } | Response::Fetch(..) => Self::Changed,
      _ => Self::Quiet,
    }
  }

  /// The more telling of two: the folder growing says the most.
  fn and(self, other: Self) -> Self {
    match (self, other) {
      (Self::Grown, _) | (_, Self::Grown) => Self::Grown,
      (Self::Changed, _) | (_, Self::Changed) => Self::Changed,
      _ => Self::Quiet,
    }
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
  /// The work connection of an account, opened when it is first needed.
  pub fn new(endpoint: MailEndpoint, account: MailAccount) -> Self {
    Self {
      endpoint,
      account,
      open: Arc::new(AsyncMutex::new(None)),
    }
  }

  /// The connection held for this caller alone, opened now if it is not, with no folder chosen for the
  /// work: listing folders, asking a folder's state.
  pub async fn connection(&self) -> Result<Connection, MailFault> {
    let mut guard = Arc::clone(&self.open).lock_owned().await;

    if guard.is_none() {
      let session = open(&self.endpoint, &self.account).await?;

      *guard = Some(Open {
        session,
        mailbox: None,
        selected: None,
      });
    }

    Ok(Connection { guard })
  }

  /// The connection held for this caller alone with the folder selected. Selecting it reads it up to date:
  /// the server takes mail that arrived since into a connection's view only at a command, so without that
  /// the first search after new mail would miss it.
  pub async fn folder(&self, path: &str) -> Result<Connection, MailFault> {
    let mut connection = self.connection().await?;

    match connection.select(path).await {
      Ok(()) => Ok(connection),
      Err(why) => connection.settle(Err(why)),
    }
  }

  /// Logs the connection out, or drops it where it will not.
  pub async fn close(&self) {
    let open = self.open.lock().await.take();

    if let Some(mut open) = open {
      let _ = open.session.logout().await;
    }
  }
}
