//! The watchers (`docs/mail.md`, 「Connections and the watcher」): one per account of the accounts file,
//! each with a connection of its own standing in `IDLE` on the inbox for as long as the service runs. A
//! watcher takes every message above the high-water mark kept in the store, one at a time — rules first,
//! then the mark moved past it, then its sender recorded — and then says on the event stream that the
//! inbox changed and what stayed unread in it. A message the service itself moved into the inbox only has
//! the mark moved past it. A changed accounts file is read again within a minute and the watchers follow
//! it; the staging folder is swept every hour.

pub mod arrivals;
pub mod notice;

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use sqlx::SqlitePool;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::accounts::{MailAccount, MailConfig, MailEndpoint};
use crate::http::Standing;
use crate::imap::conversation::conversation_key;
use crate::imap::messages::heads;
use crate::imap::moved_in::{moved_in, moves_settled};
use crate::imap::session::{Connection, Woke, open};
use crate::imap::{flag_name, uid_fetch, uid_search};
use crate::routes::requests::MailFault;
use crate::rules::{apply_plan, message_facts, plan_rules, rule_context};
use crate::watcher::arrivals::{ArrivedMessage, MailArrival, announce_arrivals, announce_change};
use crate::writing::addresses::record_addresses;
use crate::writing::staging::sweep_staging;

/// The folder a watcher stands on.
const INBOX: &str = "INBOX";
/// The first wait before a dropped connection is opened again; it doubles with each failure in a row.
const RETRY: Duration = Duration::from_secs(5);
/// The longest wait before a dropped connection is opened again.
const RETRY_CAP: Duration = Duration::from_secs(60);
/// How long a watcher stands in `IDLE`, or waits between asking a server that has none, before it asks
/// again.
const POLL: Duration = Duration::from_secs(30);
/// How often the accounts file is read again.
const CONFIG_CHECK: Duration = Duration::from_secs(60);
/// How often the staging folder is swept.
const STAGING_SWEEP: Duration = Duration::from_secs(60 * 60);

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

/// Resolves once the word to stop is given, holding nothing of it after.
async fn stopped(stop: &mut watch::Receiver<bool>) {
  let _ = stop.wait_for(|stopped| *stopped).await;
}

/// One account's watcher.
struct Watcher {
  standing: Arc<Standing>,
  endpoint: MailEndpoint,
  account: MailAccount,
}

impl Watcher {
  fn log(&self, line: &str) {
    eprintln!("mail watcher {}: {line}", self.account.address);
  }

  /// Takes every message above the mark, oldest first, and says what it found.
  async fn handle_new(&self, connection: &mut Connection) -> Result<(), MailFault> {
    let pool = self.standing.store.pool();
    let address = &self.account.address;
    let stored = read_mark(pool, address, INBOX).await?;
    let (mut mark, fresh) = starting_mark(stored, &connection.uid_validity(), connection.uid_next());

    if fresh {
      return write_mark(pool, address, INBOX, &mark).await;
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

    let (rules, spam_senders) = rule_context(pool, address).await?;
    let mut fetched =
      uid_fetch(connection.session(), &uids, "(UID FLAGS INTERNALDATE BODY.PEEK[HEADER])").await?;
    let mut arrived = Vec::new();

    fetched.sort_by_key(|fetch| fetch.uid);

    for fetch in &fetched {
      let Some(uid) = fetch.uid else {
        continue;
      };

      mark.last_uid = uid;

      if moved_in(address, &mark.validity, uid) {
        write_mark(pool, address, INBOX, &mark).await?;
        continue;
      }

      let plan = plan_rules(&rules, &spam_senders, &message_facts(fetch.header()));
      let acts = !plan.flags.is_empty() || plan.destination.is_some();
      let moved = acts && apply_plan(connection, uid, &plan).await?;

      write_mark(pool, address, INBOX, &mark).await?;

      let read = heads(fetch.header());

      record_addresses(pool, address, &read.from, chrono::Utc::now().timestamp_millis()).await?;

      let seen = fetch.flags().any(|flag| flag_name(&flag) == "\\Seen")
        || plan.flags.iter().any(|flag| flag == "\\Seen");
      let from_self = read.from.iter().any(|one| &one.address == address);

      if !moved && !seen && !from_self {
        let key = conversation_key(read.message_id.as_deref(), &read.in_reply_to, &read.references)
          .unwrap_or_else(|| format!("{INBOX}\u{0}{uid}"));

        arrived.push(ArrivedMessage {
          folder: INBOX.to_owned(),
          uid,
          key,
          from: read.from,
          subject: read.subject,
          date: fetch
            .internal_date()
            .map_or_else(|| chrono::Utc::now().timestamp_millis(), |date| date.timestamp_millis()),
        });
      }
    }

    announce_change(&self.standing.events, &self.account.key);

    if !arrived.is_empty() {
      let arrival = MailArrival {
        address: address.clone(),
        account: self.account.key.clone(),
        messages: arrived,
      };

      announce_arrivals(&self.standing.events, &self.standing.wording, &arrival);
    }

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
    let session = open(&self.endpoint, &self.account).await?;
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
        Woke::Changed => announce_change(&self.standing.events, &self.account.key),
        Woke::Quiet => {
          if !idles {
            self.handle(&mut connection).await?;
          }
        }
      }
    }
  }

  /// Watches until told to stop, opening the connection again after a wait each time it drops.
  async fn run(self, mut stop: watch::Receiver<bool>) {
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

/// The watchers of the accounts the file names now, and the word that stops them.
struct Watching {
  config: Option<Arc<MailConfig>>,
  stop: watch::Sender<bool>,
  tasks: Vec<JoinHandle<()>>,
}

impl Watching {
  fn start(standing: &Arc<Standing>, config: Option<Arc<MailConfig>>) -> Self {
    let (stop, stopped) = watch::channel(false);
    let tasks = config
      .iter()
      .flat_map(|config| {
        config.accounts.iter().map(|account| {
          let watcher = Watcher {
            standing: Arc::clone(standing),
            endpoint: config.imap.clone(),
            account: account.clone(),
          };

          tokio::spawn(watcher.run(stopped.clone()))
        })
      })
      .collect();

    Self {
      config,
      stop,
      tasks,
    }
  }

  async fn stop(self) {
    let _ = self.stop.send(true);

    futures::future::join_all(self.tasks).await;
  }
}

/// Runs the watchers until told to stop: following the accounts file every minute, sweeping the staging
/// folder every hour, and letting every watcher go at the end.
pub async fn watch(standing: Arc<Standing>, mut stop: watch::Receiver<bool>) {
  let mut checking = tokio::time::interval(CONFIG_CHECK);
  let mut sweeping = tokio::time::interval(STAGING_SWEEP);
  let mut watching: Option<Watching> = None;
  let mut said = false;

  loop {
    tokio::select! {
      () = stopped(&mut stop) => break,
      _ = checking.tick() => {
        let config = standing.accounts.read();

        if watching.as_ref().is_some_and(|now| now.config == config) {
          continue;
        }

        if let Some(now) = watching.take() {
          now.stop().await;
        }

        if config.is_none() {
          if !said {
            eprintln!("mail: no mail accounts file, so no inbox is watched");
            said = true;
          }
        } else {
          said = false;
        }

        watching = Some(Watching::start(&standing, config));
      }
      _ = sweeping.tick() => {
        sweep_staging(&standing.settings.staging.dir, SystemTime::now());
      }
    }
  }

  if let Some(now) = watching {
    now.stop().await;
  }
}
