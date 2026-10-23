//! The IMAP/SMTP adapter's watcher (`docs/mail.md`, 「Connections and the watcher」): one per account, with a
//! connection of its own standing in `IDLE` on the inbox for as long as the account stands. It takes every
//! message above the high-water mark kept in the store, one at a time — rules first, then the mark moved past
//! it, then its sender recorded — and then says on the event stream which messages stayed in the inbox and
//! that its counts moved. A message the service itself moved into the inbox only has the mark moved past it.

use std::sync::Arc;
use std::time::Duration;

use sqlx::SqlitePool;
use tokio::sync::watch;

use crate::accounts::Account;
use crate::adapters::Mailbox;
use crate::adapters::imap_smtp::{MailAccount, MailEndpoint};
use crate::announcements::{MESSAGE_CREATED, folder_now, message_now};
use crate::http::Standing;
use crate::imap::messages::heads;
use crate::imap::moved_in::{moved_in, moves_settled};
use crate::imap::session::{Connection, Woke, open};
use crate::imap::{uid_fetch, uid_search};
use crate::mail::MailRef;
use crate::routes::requests::MailFault;
use crate::rules::{apply_plan, message_facts, plan_rules, rule_context};
use crate::writing::addresses::record_addresses;

/// The folder a watcher stands on.
const INBOX: &str = "INBOX";
/// The first wait before a dropped connection is opened again; it doubles with each failure in a row.
const RETRY: Duration = Duration::from_secs(5);
/// The longest wait before a dropped connection is opened again.
const RETRY_CAP: Duration = Duration::from_secs(60);
/// How long a watcher stands in `IDLE`, or waits between asking a server that has none, before it asks
/// again.
const POLL: Duration = Duration::from_secs(30);

/// Resolves once the word to stop is given, holding nothing of it after.
async fn stopped(stop: &mut watch::Receiver<bool>) {
  let _ = stop.wait_for(|stopped| *stopped).await;
}

/// The high-water mark of a folder: the UID validity it was taken under and the highest UID handled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MailMark {
  pub validity: String,
  pub last_uid: u32,
}

async fn read_mark(pool: &SqlitePool, account: &str, folder: &str) -> Result<Option<MailMark>, MailFault> {
  let row: Option<(String, i64)> =
    sqlx::query_as("SELECT uid_validity, last_uid FROM marks WHERE account = ? AND folder = ?")
      .bind(account)
      .bind(folder)
      .fetch_optional(pool)
      .await?;

  Ok(row.map(|(validity, last_uid)| MailMark {
    validity,
    last_uid: u32::try_from(last_uid).unwrap_or(0),
  }))
}

async fn write_mark(
  pool: &SqlitePool,
  account: &str,
  folder: &str,
  mark: &MailMark,
) -> Result<(), MailFault> {
  sqlx::query(
    "INSERT INTO marks (account, folder, uid_validity, last_uid) VALUES (?, ?, ?, ?)
     ON CONFLICT (account, folder) DO UPDATE SET uid_validity = excluded.uid_validity, last_uid = excluded.last_uid",
  )
  .bind(account)
  .bind(folder)
  .bind(&mark.validity)
  .bind(i64::from(mark.last_uid))
  .execute(pool)
  .await?;

  Ok(())
}

/// Where handling starts: the stored mark where it was taken under the folder's present UID validity;
/// otherwise — a first start, or a folder the server rebuilt — at the newest message, handling nothing
/// older. The second answer says which.
pub fn starting_mark(stored: Option<MailMark>, validity: &str, uid_next: u32) -> (MailMark, bool) {
  match stored {
    Some(mark) if mark.validity == validity => (mark, false),
    _ => (
      MailMark {
        validity: validity.to_owned(),
        last_uid: uid_next.saturating_sub(1),
      },
      true,
    ),
  }
}

/// The wait before the next try after this many failures in a row: five seconds, doubling to a minute.
pub fn retry_wait(failures: u32) -> Duration {
  RETRY.saturating_mul(2u32.saturating_pow(failures.saturating_sub(1))).min(RETRY_CAP)
}

/// One account's watcher: the account, its mailbox the events are read through, and the server and login
/// its own connection is opened with.
pub struct Watcher {
  pub standing: Arc<Standing>,
  pub account: Account,
  pub mailbox: Arc<dyn Mailbox>,
  pub endpoint: MailEndpoint,
  pub login: MailAccount,
}

impl Watcher {
  fn log(&self, line: &str) {
    eprintln!("mail watcher {}: {line}", self.account.address);
  }

  /// Says on the event stream that the inbox's counts may have moved.
  async fn announce_inbox(&self) {
    folder_now(&self.standing, &*self.mailbox, &self.account, INBOX).await;
  }

  /// Takes every message above the mark, oldest first, and says what it found.
  async fn handle_new(&self, connection: &mut Connection) -> Result<(), MailFault> {
    let pool = self.standing.store.pool();
    let key = &self.account.store_key();
    let address = &self.login.address;
    let stored = read_mark(pool, key, INBOX).await?;
    let (mut mark, fresh) = starting_mark(stored, &connection.uid_validity(), connection.uid_next());

    if fresh {
      return write_mark(pool, key, INBOX, &mark).await;
    }

    // A move into this inbox still under way is noted first, so what it brings is not taken as mail.
    moves_settled(address).await;

    let mut uids: Vec<u32> = uid_search(connection.session(), &format!("UID {}:*", mark.last_uid + 1))
      .await?
      .into_iter()
      .filter(|uid| *uid > mark.last_uid)
      .collect();

    uids.sort_unstable();

    if uids.is_empty() {
      return Ok(());
    }

    let (rules, spam_senders) = rule_context(pool, key).await?;
    let mut fetched = uid_fetch(connection.session(), &uids, "(UID BODY.PEEK[HEADER])").await?;
    let mut arrived = Vec::new();

    fetched.sort_by_key(|fetch| fetch.uid);

    for fetch in &fetched {
      let Some(uid) = fetch.uid else {
        continue;
      };

      mark.last_uid = uid;

      if moved_in(address, &mark.validity, uid) {
        write_mark(pool, key, INBOX, &mark).await?;
        continue;
      }

      let plan = plan_rules(&rules, &spam_senders, &message_facts(fetch.header()));
      let acts = !plan.flags.is_empty() || plan.destination.is_some();
      let moved = acts && apply_plan(connection, uid, &plan).await?;

      write_mark(pool, key, INBOX, &mark).await?;

      let read = heads(fetch.header());

      record_addresses(pool, key, address, &read.from, chrono::Utc::now().timestamp_millis()).await?;

      if !moved {
        arrived.push(uid);
      }
    }

    for uid in arrived {
      let at = MailRef {
        folder: INBOX.to_owned(),
        uid,
      };

      message_now(&self.standing, &*self.mailbox, &self.account, MESSAGE_CREATED, &at).await;
    }

    self.announce_inbox().await;

    Ok(())
  }

  /// Handles new mail, saying so in the log where it fails; the watcher goes on either way.
  async fn handle(&self, connection: &mut Connection) -> Result<(), MailFault> {
    match self.handle_new(connection).await {
      Err(MailFault::Asked(asked)) => {
        self.log(&format!("handling new mail failed ({})", asked.error));
        Ok(())
      }
      done => done,
    }
  }

  /// One connection's life: open, take whatever came while nobody watched, then stand in `IDLE` (or ask
  /// every thirty seconds where the server has none) until the connection drops.
  async fn watch_once(&self, failures: &mut u32) -> Result<(), MailFault> {
    let session = open(&self.endpoint, &self.login).await?;
    let mut connection = Connection::alone(session).await;

    connection.select(INBOX).await?;

    let idles = connection.session().capabilities().await.map_err(MailFault::server)?.has_str("IDLE");

    *failures = 0;
    self.handle(&mut connection).await?;

    loop {
      let woke = if idles {
        connection.idle(POLL).await?
      } else {
        tokio::time::sleep(POLL).await;
        connection.poll().await?
      };

      match woke {
        Woke::Grown => self.handle(&mut connection).await?,
        Woke::Changed => self.announce_inbox().await,
        Woke::Quiet => {
          if !idles {
            self.handle(&mut connection).await?;
          }
        }
      }
    }
  }

  /// Watches until told to stop, opening the connection again after a wait each time it drops.
  pub async fn run(self, mut stop: watch::Receiver<bool>) {
    let mut failures = 0;

    loop {
      tokio::select! {
        () = stopped(&mut stop) => return,
        dropped = self.watch_once(&mut failures) => {
          if let Err(MailFault::Server(why)) = dropped {
            self.log(&format!("could not watch the inbox ({why}); trying again"));
          }
        }
      }

      failures += 1;

      tokio::select! {
        () = stopped(&mut stop) => return,
        () = tokio::time::sleep(retry_wait(failures)) => {}
      }
    }
  }
}
