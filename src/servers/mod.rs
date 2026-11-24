//! The mail servers (`docs/mail.md`, 「Mail servers」): each one kept once in the store under its holder and chosen
//! by any number of accounts. A server holds no secret; its holder is a person's owner key or a link of the
//! account service's delegation chain, which `organisation` asks who offers what.

pub mod organisation;
pub mod probe;

use sqlx::{SqliteConnection, SqlitePool};

use crate::adapters::servers::MailEndpoint;
use crate::routes::requests::{Asked, MailFault};
use crate::wire::{Direction, Protocol, Security, ServerAuth};

/// The longest a server's name may be.
const NAME_LONGEST: usize = 80;
/// The chain's link that holds the platform's servers.
pub const PLATFORM: &str = "platform";
/// What a person's own holder begins with, before their owner key.
const OWN: &str = "owner:";

/// The holder a person's own servers stand under.
pub fn own_holder(owner: &str) -> String {
  format!("{OWN}{owner}")
}

/// A link of the delegation chain a server may stand under, `platform` or `unit:<id>`, as written; none where
/// it is not one.
pub fn chain_link(link: &str) -> Option<String> {
  let link = link.trim();

  if link == PLATFORM {
    return Some(link.to_owned());
  }

  let id = link.strip_prefix("unit:")?;

  (!id.is_empty() && id.chars().all(|c| c.is_ascii_digit())).then(|| link.to_owned())
}

impl Protocol {
  /// Whether a server of the protocol is read from or handed mail.
  pub fn direction(self) -> Direction {
    match self {
      Self::Imap | Self::Pop3 => Direction::Incoming,
      Self::Smtp => Direction::Outgoing,
    }
  }

  /// The protocol's name as the store keeps it.
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Imap => "imap",
      Self::Pop3 => "pop3",
      Self::Smtp => "smtp",
    }
  }

  fn named(name: &str) -> Option<Self> {
    [Self::Imap, Self::Pop3, Self::Smtp].into_iter().find(|one| one.as_str() == name)
  }

  /// The protocol's usual port for a security.
  pub fn usual_port(self, security: Security) -> u16 {
    match (self, security) {
      (Self::Imap, Security::Tls) => 993,
      (Self::Imap, _) => 143,
      (Self::Pop3, Security::Tls) => 995,
      (Self::Pop3, _) => 110,
      (Self::Smtp, Security::Tls) => 465,
      (Self::Smtp, Security::Starttls) => 587,
      (Self::Smtp, Security::None) => 25,
    }
  }

  /// The security a server of the protocol has where none is given.
  pub fn usual_security(self) -> Security {
    match self.direction() {
      Direction::Incoming => Security::Tls,
      Direction::Outgoing => Security::Starttls,
    }
  }

  /// The security a server of the protocol was read as before servers were records, as the accounts file still
  /// writes it: a certificate name of `-` is plain; else IMAP on 993 and submission on 465 speak TLS from the
  /// first byte, POP3 always does, and every other port is upgraded.
  pub fn security_as_written(self, port: u16, plain: bool) -> Security {
    match (self, port) {
      _ if plain => Security::None,
      (Self::Pop3, _) | (Self::Imap, 993) | (Self::Smtp, 465) => Security::Tls,
      _ => Security::Starttls,
    }
  }
}

impl Security {
  fn as_str(self) -> &'static str {
    match self {
      Self::Tls => "tls",
      Self::Starttls => "starttls",
      Self::None => "none",
    }
  }

  fn named(name: &str) -> Option<Self> {
    [Self::Tls, Self::Starttls, Self::None].into_iter().find(|one| one.as_str() == name)
  }
}

impl ServerAuth {
  fn as_str(self) -> &'static str {
    match self {
      Self::Password => "password",
      Self::None => "none",
    }
  }

  fn named(name: &str) -> Option<Self> {
    [Self::Password, Self::None].into_iter().find(|one| one.as_str() == name)
  }
}

/// A server's settings, checked: everything a record holds but its id and its holder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerDraft {
  pub protocol: Protocol,
  pub name: String,
  pub host: String,
  pub port: u16,
  pub security: Security,
  /// None for the host.
  pub tls_name: Option<String>,
  pub auth: ServerAuth,
}

/// A server's settings as a body gives them, each one left out taking its default.
#[derive(Clone, Debug, Default)]
pub struct ServerFields {
  pub name: Option<String>,
  pub host: Option<String>,
  pub port: Option<u16>,
  pub security: Option<Security>,
  pub tls_name: Option<String>,
  pub auth: Option<ServerAuth>,
}

impl ServerDraft {
  /// The settings of a server of `protocol`, each one checked and the ones left out given their defaults, or why
  /// they are not a server's.
  pub fn checked(protocol: Protocol, fields: ServerFields) -> Result<Self, Asked> {
    let host = fields.host.as_deref().map(str::trim).unwrap_or_default().to_lowercase();

    if host.is_empty() || host.chars().any(|c| c.is_whitespace() || c == '/' || c.is_control()) {
      return Err(Asked::bad_request("a server's host is a host name or an address"));
    }

    let security = fields.security.unwrap_or_else(|| protocol.usual_security());

    if protocol == Protocol::Pop3 && security == Security::Starttls {
      return Err(Asked::bad_request("a POP3 server is reached over TLS or plain, never with STARTTLS"));
    }

    let port = fields.port.unwrap_or_else(|| protocol.usual_port(security));

    if port == 0 {
      return Err(Asked::bad_request("a server's port is 1 to 65535"));
    }

    let auth = fields.auth.unwrap_or(ServerAuth::Password);

    if auth == ServerAuth::None && protocol.direction() == Direction::Incoming {
      return Err(Asked::bad_request("an incoming server is signed in to with the account's password"));
    }

    let name = fields
      .name
      .as_deref()
      .map(str::trim)
      .filter(|name| !name.is_empty())
      .unwrap_or(&host)
      .to_owned();

    if name.chars().count() > NAME_LONGEST || name.chars().any(char::is_control) {
      return Err(Asked::bad_request("a server's name is 1 to 80 characters on one line"));
    }

    let tls_name = fields
      .tls_name
      .map(|name| name.trim().to_lowercase())
      .filter(|name| !name.is_empty() && *name != host && security != Security::None);

    Ok(Self {
      protocol,
      name,
      host,
      port,
      security,
      tls_name,
      auth,
    })
  }

  /// The settings as a body's fields, to be changed by a patch.
  pub fn fields(&self) -> ServerFields {
    ServerFields {
      name: Some(self.name.clone()),
      host: Some(self.host.clone()),
      port: Some(self.port),
      security: Some(self.security),
      tls_name: self.tls_name.clone(),
      auth: Some(self.auth),
    }
  }

  /// A connection to the server, as these settings name it.
  pub fn endpoint(&self) -> MailEndpoint {
    MailEndpoint {
      host: self.host.clone(),
      port: self.port,
      security: self.security,
      tls_name: self.tls_name.clone().unwrap_or_else(|| self.host.clone()),
      auth: self.auth,
    }
  }
}

/// A server as the store keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MailServer {
  pub id: i64,
  /// `owner:` and the owner key for a person's own, else the chain's link.
  pub holder: String,
  pub draft: ServerDraft,
}

impl MailServer {
  /// A connection to the server.
  pub fn endpoint(&self) -> MailEndpoint {
    self.draft.endpoint()
  }

  pub fn protocol(&self) -> Protocol {
    self.draft.protocol
  }

  /// The owner key whose own it is; none for a server of a link.
  pub fn owner(&self) -> Option<&str> {
    self.holder.strip_prefix(OWN)
  }
}

/// A row of `servers`.
type Row = (i64, String, String, String, String, i64, String, Option<String>, String);

const COLUMNS: &str = "id, holder, protocol, name, host, port, security, tls_name, auth";

fn opened(row: Row) -> Result<MailServer, MailFault> {
  let (id, holder, protocol, name, host, port, security, tls_name, auth) = row;
  let unread = || MailFault::server(format!("the server {id} in the store does not read"));

  Ok(MailServer {
    id,
    holder,
    draft: ServerDraft {
      protocol: Protocol::named(&protocol).ok_or_else(unread)?,
      name,
      host,
      port: u16::try_from(port).map_err(|_| unread())?,
      security: Security::named(&security).ok_or_else(unread)?,
      tls_name,
      auth: ServerAuth::named(&auth).ok_or_else(unread)?,
    },
  })
}

/// One server by its id, on a connection.
pub async fn server_on(
  connection: &mut SqliteConnection,
  id: i64,
) -> Result<Option<MailServer>, MailFault> {
  let row: Option<Row> = sqlx::query_as(&format!("SELECT {COLUMNS} FROM servers WHERE id = ?"))
    .bind(id)
    .fetch_optional(&mut *connection)
    .await?;

  row.map(opened).transpose()
}

/// Keeps a server under a holder on a connection, and answers its id.
pub async fn keep(
  connection: &mut SqliteConnection,
  holder: &str,
  draft: &ServerDraft,
) -> Result<i64, MailFault> {
  Ok(
    sqlx::query_scalar(
      "INSERT INTO servers (holder, protocol, name, host, port, security, tls_name, auth, created_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(holder)
    .bind(draft.protocol.as_str())
    .bind(&draft.name)
    .bind(&draft.host)
    .bind(i64::from(draft.port))
    .bind(draft.security.as_str())
    .bind(&draft.tls_name)
    .bind(draft.auth.as_str())
    .bind(chrono::Utc::now().timestamp_millis())
    .fetch_one(&mut *connection)
    .await?,
  )
}

/// The server of a holder whose settings are these, kept where the holder holds none: one record for every set
/// of identical settings, as the accounts file's import takes them.
pub async fn kept_or_keep(
  connection: &mut SqliteConnection,
  holder: &str,
  draft: &ServerDraft,
) -> Result<i64, MailFault> {
  let found: Option<i64> = sqlx::query_scalar(
    "SELECT MIN(id) FROM servers WHERE holder = ? AND protocol = ? AND host = ? AND port = ? AND security = ?
     AND tls_name IS ?",
  )
  .bind(holder)
  .bind(draft.protocol.as_str())
  .bind(&draft.host)
  .bind(i64::from(draft.port))
  .bind(draft.security.as_str())
  .bind(&draft.tls_name)
  .fetch_one(&mut *connection)
  .await?;

  match found {
    Some(id) => Ok(id),
    None => keep(connection, holder, draft).await,
  }
}

/// The ids of the servers a holder holds, in the order they were kept.
pub async fn ids_held(connection: &mut SqliteConnection, holder: &str) -> Result<Vec<i64>, MailFault> {
  Ok(
    sqlx::query_scalar("SELECT id FROM servers WHERE holder = ? ORDER BY id")
      .bind(holder)
      .fetch_all(&mut *connection)
      .await?,
  )
}

/// Writes a server's settings as they now stand; its holder and its protocol stay.
pub async fn change(
  connection: &mut SqliteConnection,
  id: i64,
  draft: &ServerDraft,
) -> Result<(), MailFault> {
  sqlx::query(
    "UPDATE servers SET name = ?, host = ?, port = ?, security = ?, tls_name = ?, auth = ? WHERE id = ?",
  )
  .bind(&draft.name)
  .bind(&draft.host)
  .bind(i64::from(draft.port))
  .bind(draft.security.as_str())
  .bind(&draft.tls_name)
  .bind(draft.auth.as_str())
  .bind(id)
  .execute(&mut *connection)
  .await?;

  Ok(())
}

/// Forgets a server.
pub async fn remove(connection: &mut SqliteConnection, id: i64) -> Result<(), MailFault> {
  sqlx::query("DELETE FROM servers WHERE id = ?")
    .bind(id)
    .execute(&mut *connection)
    .await?;

  Ok(())
}

/// How many accounts stand on a server, every owner's together.
pub async fn account_count(connection: &mut SqliteConnection, id: i64) -> Result<i64, MailFault> {
  Ok(
    sqlx::query_scalar("SELECT COUNT(*) FROM accounts WHERE incoming_server = ? OR outgoing_server = ?")
      .bind(id)
      .bind(id)
      .fetch_one(&mut *connection)
      .await?,
  )
}

/// The servers in the store.
#[derive(Clone, Debug)]
pub struct Servers {
  pool: SqlitePool,
}

impl Servers {
  pub fn new(pool: SqlitePool) -> Self {
    Self { pool }
  }

  /// The store's pool, for a change made in one transaction.
  pub fn pool(&self) -> &SqlitePool {
    &self.pool
  }

  /// One server by its id.
  pub async fn get(&self, id: i64) -> Result<Option<MailServer>, MailFault> {
    let mut connection = self.pool.acquire().await?;

    server_on(&mut connection, id).await
  }

  /// The servers the holders hold, the holders' order kept and each holder's in the order they were kept,
  /// narrowed to one direction where one is given.
  pub async fn held_by(
    &self,
    holders: &[String],
    direction: Option<Direction>,
  ) -> Result<Vec<MailServer>, MailFault> {
    let mut all = Vec::new();

    for holder in holders {
      let rows: Vec<Row> =
        sqlx::query_as(&format!("SELECT {COLUMNS} FROM servers WHERE holder = ? ORDER BY id"))
          .bind(holder)
          .fetch_all(&self.pool)
          .await?;

      for row in rows {
        let server = opened(row)?;

        if direction.is_none_or(|direction| server.protocol().direction() == direction) {
          all.push(server);
        }
      }
    }

    Ok(all)
  }

  /// How many accounts stand on a server.
  pub async fn account_count(&self, id: i64) -> Result<i64, MailFault> {
    let mut connection = self.pool.acquire().await?;

    account_count(&mut connection, id).await
  }
}
