//! The IMAP/SMTP adapter's watcher (`docs/mail.md`, 「Connections and the watcher」): one per account, with a
//! connection of its own standing in `IDLE` on the inbox for as long as the account stands. It takes every
//! message above the high-water mark kept in the store, one at a time — rules first, then the mark moved past
//! it, then its sender recorded — and then says on the event stream which messages stayed in the inbox and
//! that its counts moved. A message the service itself moved into the inbox only has the mark moved past it.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;

use crate::accounts::Account;
use crate::adapters::Mailbox;
use crate::adapters::servers::{MailAccount, MailEndpoint};
use crate::announcements::{MESSAGE_CREATED, folder_now, message_now};
use crate::http::Standing;
use crate::imap::messages::heads;
use crate::imap::moved_in::{moved_in, moves_settled};
use crate::imap::session::{Connection, Woke, open};
use crate::imap::{uid_fetch, uid_search};
use crate::mail::MailRef;
use crate::routes::requests::MailFault;
use crate::rules::{apply_plan, message_facts, plan_rules, rule_context};
use crate::watcher::marks::{MailMark, owe, owed, read_mark, settled, write_mark};
use crate::writing::addresses::record_addresses;

/// The folder a watcher stands on.
const INBOX: &str = "INBOX";
/// The longest wait before a dropped connection is opened again.
const RETRY_CAP: Duration = Duration::from_secs(60);
/// How long a watcher stands in `IDLE`, or waits between asking a server that has none, before it asks
/// again.
const POLL: Duration = Duration::from_secs(30);

/// Resolves once the word to stop is given, holding nothing of it after.
async fn stopped(stop: &mut watch::Receiver<bool>) {
  let _ = stop.wait_for(|stopped| *stopped).await;
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

/// The wait before the next try after this many failures in a row: the first wait, doubling to a minute.
pub fn retry_wait(first: Duration, failures: u32) -> Duration {
  first.saturating_mul(2u32.saturating_pow(failures.saturating_sub(1))).min(RETRY_CAP)
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

  /// The wait before the next try after this many failures in a row, from the first wait the settings name.
  fn retry_wait(&self, failures: u32) -> Duration {
    retry_wait(self.standing.settings.watcher.retry(), failures)
  }

  /// Says on the event stream that the inbox's counts may have moved.
  async fn announce_inbox(&self) {
    folder_now(&self.standing, &*self.mailbox, &self.account, INBOX).await;
  }

  /// Says every arrival still owed its event, letting each go once said; one whose message could not be read
  /// stays owed for the next handling.
  async fn say_owed(&self, validity: &str) -> Result<(), MailFault> {
    let pool = self.standing.store.pool();
    let key = &self.account.store_key();

    for uid in owed(pool, key, INBOX, validity).await? {
      let at = MailRef {
        folder: INBOX.to_owned(),
        uid,
      };

      if message_now(&self.standing, &*self.mailbox, &self.account, MESSAGE_CREATED, &at).await {
        settled(pool, key, INBOX, validity, uid).await?;
      } else {
        self.log(&format!("message {uid} stays owed its arrival event; it is said at the next handling"));
      }
    }

    Ok(())
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
      return self.say_owed(&mark.validity).await;
    }

    let (rules, spam_senders) = rule_context(pool, key).await?;
    let mut fetched = uid_fetch(connection.session(), &uids, "(UID BODY.PEEK[HEADER])").await?;
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

      if !moved {
        owe(pool, key, INBOX, &mark.validity, uid).await?;
      }

      write_mark(pool, key, INBOX, &mark).await?;

      let read = heads(fetch.header());

      record_addresses(pool, key, address, &read.from, chrono::Utc::now().timestamp_millis()).await?;
    }

    self.say_owed(&mark.validity).await?;
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

    if *failures > 0 {
      self.log(&format!(
        "watching the inbox again after {} dropped connection(s); taking what arrived since",
        *failures
      ));
    }

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
          let why = match dropped {
            Err(MailFault::Server(why)) => why,
            Err(MailFault::Asked(asked)) => asked.error,
            Ok(()) => "the connection ended".to_owned(),
          };

          self.log(&format!(
            "could not watch the inbox ({why}); opening the connection again in {} ms",
            self.retry_wait(failures + 1).as_millis()
          ));
        }
      }

      failures += 1;

      tokio::select! {
        () = stopped(&mut stop) => return,
        () = tokio::time::sleep(self.retry_wait(failures)) => {}
      }
    }
  }
}
