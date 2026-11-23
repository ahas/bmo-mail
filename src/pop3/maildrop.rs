//! A POP3 account's maildrop as the store keeps it (`docs/mail.md`, 「A POP3 account」): a row for each message
//! the maildrop holds, known by its `UIDL`, with its head, when it arrived, the flags the service keeps for it and,
//! once it was read whole, its parts and snippet. Every piece of work takes its turn at the one session the server
//! allows, opens it, brings the rows up to date with the maildrop's listing, and ends it with `QUIT`.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use mail_parser::{HeaderName, MessageParser};
use sqlx::SqlitePool;
use tokio::sync::{Mutex, MutexGuard};

use crate::adapters::servers::{MailAccount, MailEndpoint};
use crate::mail::MailParts;
use crate::pop3::INBOX;
use crate::pop3::mime::{Whole, read_whole};
use crate::pop3::session::{Listed, Pop3Session, open};
use crate::routes::requests::MailFault;

/// How long a listing brought up to date is taken as it stands before a read asks the server again.
const FRESH: Duration = Duration::from_secs(10);

/// One message of the maildrop as the store keeps it.
#[derive(Clone, Debug)]
pub struct Row {
  /// The number the service gave it, which is its UID.
  pub id: u32,
  pub uidl: String,
  /// When it arrived, in Unix milliseconds.
  pub at: i64,
  pub head: Vec<u8>,
  pub flags: Vec<String>,
  /// Its parts and snippet, once it was read whole.
  pub parts: Option<MailParts>,
  pub snippet: Option<String>,
}

type RowColumns = (i64, String, i64, Vec<u8>, String, Option<String>, Option<String>);

fn row_of((id, uidl, at, head, flags, parts, snippet): RowColumns) -> Option<Row> {
  Some(Row {
    id: u32::try_from(id).ok()?,
    uidl,
    at,
    head,
    flags: serde_json::from_str(&flags).unwrap_or_default(),
    parts: parts.and_then(|parts| serde_json::from_str(&parts).ok()),
    snippet,
  })
}

const ROW_COLUMNS: &str = "id, uidl, at, head, flags, parts, snippet";

/// Every message of the account's maildrop the store keeps, oldest first.
pub async fn rows(pool: &SqlitePool, account: &str) -> Result<Vec<Row>, MailFault> {
  let found: Vec<RowColumns> =
    sqlx::query_as(&format!("SELECT {ROW_COLUMNS} FROM pop3_messages WHERE account = ? ORDER BY id"))
      .bind(account)
      .fetch_all(pool)
      .await?;

  Ok(found.into_iter().filter_map(row_of).collect())
}

/// One message the store keeps, by its number.
pub async fn row(pool: &SqlitePool, account: &str, id: u32) -> Result<Option<Row>, MailFault> {
  let found: Option<RowColumns> =
    sqlx::query_as(&format!("SELECT {ROW_COLUMNS} FROM pop3_messages WHERE account = ? AND id = ?"))
      .bind(account)
      .bind(i64::from(id))
      .fetch_optional(pool)
      .await?;

  Ok(found.and_then(row_of))
}

/// Keeps the flags a message carries.
pub async fn keep_flags(
  pool: &SqlitePool,
  account: &str,
  id: u32,
  flags: &[String],
) -> Result<(), MailFault> {
  sqlx::query("UPDATE pop3_messages SET flags = ? WHERE account = ? AND id = ?")
    .bind(serde_json::to_string(flags).map_err(MailFault::server)?)
    .bind(account)
    .bind(i64::from(id))
    .execute(pool)
    .await?;

  Ok(())
}

/// Keeps what reading a message whole told: its parts and its snippet.
async fn keep_whole(pool: &SqlitePool, account: &str, id: u32, whole: &Whole) -> Result<(), MailFault> {
  sqlx::query("UPDATE pop3_messages SET parts = ?, snippet = ? WHERE account = ? AND id = ?")
    .bind(serde_json::to_string(&whole.parts).map_err(MailFault::server)?)
    .bind(&whole.snippet)
    .bind(account)
    .bind(i64::from(id))
    .execute(pool)
    .await?;

  Ok(())
}

/// When a message arrived, from its head: the date of its topmost `Received` line, the delivering server's own,
/// else its `Date`, else now.
pub fn arrived_at(head: &[u8]) -> i64 {
  let parser = MessageParser::new().header_received(HeaderName::Received).with_date_headers();
  let read = parser.parse_headers(head);
  let received = read
    .as_ref()
    .and_then(|message| message.received_all().next().and_then(|line| line.date()));
  let written = read.as_ref().and_then(|message| message.date().cloned());

  received
    .or(written)
    .map(|date| date.to_timestamp() * 1000)
    .unwrap_or_else(|| chrono::Utc::now().timestamp_millis())
}

/// A POP3 account's maildrop: the server and login a session is opened with, the store its rows stand in under
/// the account's key, and the one turn at a session, holding when the rows were last brought up to date.
#[derive(Debug)]
pub struct Maildrop {
  pub endpoint: MailEndpoint,
  pub login: MailAccount,
  pub pool: SqlitePool,
  pub key: String,
  turn: Mutex<Option<Instant>>,
}

/// A session open and the rows brought up to date with it: each message's number in this session by its
/// `UIDL`. Ended with `done`; a turn let go without it closes the connection, which deletes nothing either.
pub struct Turn<'a> {
  maildrop: &'a Maildrop,
  _held: MutexGuard<'a, Option<Instant>>,
  session: Pop3Session,
  numbers: HashMap<String, usize>,
}

impl Maildrop {
  pub fn new(endpoint: MailEndpoint, login: MailAccount, pool: SqlitePool, key: String) -> Self {
    Self {
      endpoint,
      login,
      pool,
      key,
      turn: Mutex::new(None),
    }
  }

  /// Forgets every row whose `UIDL` the listing no longer holds, with the arrival it may still be owed.
  async fn forget_gone(&self, listing: &[Listed]) -> Result<(), MailFault> {
    let listed: HashSet<&str> = listing.iter().map(|one| one.uidl.as_str()).collect();

    for gone in self.known().await?.iter().filter(|uidl| !listed.contains(uidl.as_str())) {
      sqlx::query(
        "DELETE FROM arrivals WHERE account = ? AND folder = ? AND uid IN
         (SELECT id FROM pop3_messages WHERE account = ? AND uidl = ?)",
      )
      .bind(&self.key)
      .bind(INBOX)
      .bind(&self.key)
      .bind(gone)
      .execute(&self.pool)
      .await?;
      sqlx::query("DELETE FROM pop3_messages WHERE account = ? AND uidl = ?")
        .bind(&self.key)
        .bind(gone)
        .execute(&self.pool)
        .await?;
    }

    Ok(())
  }

  async fn known(&self) -> Result<HashSet<String>, MailFault> {
    let known: Vec<String> = sqlx::query_scalar("SELECT uidl FROM pop3_messages WHERE account = ?")
      .bind(&self.key)
      .fetch_all(&self.pool)
      .await?;

    Ok(known.into_iter().collect())
  }

  /// Keeps a message come since with its head and a new number.
  async fn keep_new(&self, one: &Listed, head: &[u8]) -> Result<(), MailFault> {
    sqlx::query(
      "INSERT OR IGNORE INTO pop3_messages (account, uidl, at, size, head, flags, created_at)
       VALUES (?, ?, ?, ?, ?, '[]', ?)",
    )
    .bind(&self.key)
    .bind(&one.uidl)
    .bind(arrived_at(head))
    .bind(i64::try_from(one.size).unwrap_or(i64::MAX))
    .bind(head)
    .bind(chrono::Utc::now().timestamp_millis())
    .execute(&self.pool)
    .await?;

    Ok(())
  }

  /// Takes the turn, opens a session and brings the rows up to date with the maildrop's listing: a `UIDL` the
  /// store does not know is kept with its head and a new number, one the listing no longer holds is forgotten.
  /// A head past what the POP3 client reads in one answer is kept empty, and the session it spent is opened
  /// again for the rest.
  pub async fn turn(&self) -> Result<Turn<'_>, MailFault> {
    let mut held = self.turn.lock().await;
    let mut session = open(&self.endpoint, &self.login).await?;

    let listing = loop {
      let listing = session.listing().await?;

      self.forget_gone(&listing).await?;

      let known = self.known().await?;
      let mut spent = false;

      for one in listing.iter().filter(|one| !known.contains(&one.uidl)) {
        let head = session.head(one.number).await?;

        if head.is_none() {
          eprintln!(
            "mail: a message of {} is past what the POP3 client reads in one answer; it stands with no head",
            self.login.address
          );
        }

        self.keep_new(one, head.as_deref().unwrap_or_default()).await?;

        if head.is_none() {
          spent = true;
          break;
        }
      }

      if !spent {
        break listing;
      }

      session = open(&self.endpoint, &self.login).await?;
    };

    *held = Some(Instant::now());

    Ok(Turn {
      maildrop: self,
      _held: held,
      session,
      numbers: numbers_of(listing),
    })
  }

  /// Brings the rows up to date where they were not in the last few seconds, so reads made together sign in
  /// once.
  pub async fn fresh(&self) -> Result<(), MailFault> {
    let synced = *self.turn.lock().await;

    if synced.is_some_and(|at| at.elapsed() < FRESH) {
      return Ok(());
    }

    self.turn().await?.done().await;

    Ok(())
  }

  /// A message read whole from the server, its parts and snippet kept on its row; none where it is gone, and the
  /// server's failure where it is past what the POP3 client reads in one answer.
  pub async fn whole(&self, id: u32) -> Result<Option<(Row, Vec<u8>)>, MailFault> {
    let mut turn = self.turn().await?;
    let read = turn.whole(id).await;

    turn.done().await;

    match read? {
      None => Ok(None),
      Some((row, Some(raw))) => Ok(Some((row, raw))),
      Some((_, None)) => {
        Err(MailFault::server("the message is past what the POP3 client reads in one answer"))
      }
    }
  }
}

/// Each message's number in a session, by its `UIDL`.
fn numbers_of(listing: Vec<Listed>) -> HashMap<String, usize> {
  listing.into_iter().map(|one| (one.uidl, one.number)).collect()
}

impl Turn<'_> {
  /// A message read whole through this session, its parts and snippet kept on its row; none where the maildrop
  /// no longer holds it. A message past what the POP3 client reads in one answer comes back with its row alone,
  /// and the session it spent is opened again.
  pub async fn whole(&mut self, id: u32) -> Result<Option<(Row, Option<Vec<u8>>)>, MailFault> {
    let maildrop = self.maildrop;
    let Some(found) = row(&maildrop.pool, &maildrop.key, id).await? else {
      return Ok(None);
    };
    let Some(number) = self.numbers.get(&found.uidl).copied() else {
      return Ok(None);
    };
    let Some(raw) = self.session.message(number).await? else {
      self.session = open(&maildrop.endpoint, &maildrop.login).await?;
      self.numbers = numbers_of(self.session.listing().await?);

      return Ok(Some((found, None)));
    };

    if found.parts.is_none() {
      keep_whole(&maildrop.pool, &maildrop.key, id, &read_whole(&raw)).await?;
    }

    Ok(row(&maildrop.pool, &maildrop.key, id).await?.map(|row| (row, Some(raw))))
  }

  /// Ends the session with `QUIT` and lets the turn go.
  pub async fn done(self) {
    self.session.quit().await;
  }
}
