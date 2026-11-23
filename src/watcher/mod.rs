//! The watchers (`docs/mail.md`, 「Connections and the watcher」): one for each account in the store, each the
//! account's mailbox watching its own inbox for as long as the service runs and the account stands as it was.
//! An account added, changed or removed starts, restarts or stops its watcher at once, and the store is read
//! again every minute besides; the staging folder is swept every hour.

pub mod marks;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::accounts::Account;
use crate::adapters::Watch;
use crate::http::Standing;
use crate::writing::staging::sweep_staging;

/// How often the store's accounts are read again, beside the word that one changed.
const ACCOUNTS_CHECK: Duration = Duration::from_secs(60);
/// How often the staging folder is swept.
const STAGING_SWEEP: Duration = Duration::from_secs(60 * 60);

/// Resolves once the word to stop is given, holding nothing of it after.
async fn stopped(stop: &mut watch::Receiver<bool>) {
  let _ = stop.wait_for(|stopped| *stopped).await;
}

/// One account's watcher: the account as it was started on, and the word that stops it.
struct Running {
  account: Account,
  stop: watch::Sender<bool>,
  task: JoinHandle<()>,
}

impl Running {
  async fn stop(self) {
    let _ = self.stop.send(true);
    let _ = self.task.await;
  }
}

/// Starts the watcher of an account through its mailbox; none where its mailbox does not open.
fn start(standing: &Arc<Standing>, account: Account) -> Option<Running> {
  let mailbox = match standing.mailboxes.of(&account, &standing.context()) {
    Ok(mailbox) => mailbox,
    Err(why) => {
      eprintln!("mail: the account {} is not watched: {why:?}", account.address);
      return None;
    }
  };
  let (stop, stopping) = watch::channel(false);
  let task = tokio::spawn(mailbox.watch(Watch {
    standing: Arc::clone(standing),
    account: account.clone(),
    stop: stopping,
  }));

  Some(Running {
    account,
    stop,
    task,
  })
}

/// Brings the watchers in line with the accounts that stand now: a removed or changed account's watcher
/// stops, and a new or changed one's starts.
async fn follow(standing: &Arc<Standing>, running: &mut HashMap<i64, Running>) {
  let accounts = match standing.accounts.all().await {
    Ok(accounts) => accounts,
    Err(why) => {
      eprintln!("mail: the accounts could not be read for the watchers: {why:?}");
      return;
    }
  };
  let standing_now: HashMap<i64, Account> =
    accounts.into_iter().map(|account| (account.id, account)).collect();
  let stale: Vec<i64> = running
    .iter()
    .filter(|(id, one)| standing_now.get(id) != Some(&one.account))
    .map(|(id, _)| *id)
    .collect();

  for id in stale {
    if let Some(one) = running.remove(&id) {
      one.stop().await;
    }

    if !standing_now.contains_key(&id) {
      standing.mailboxes.forget(id).await;
    }
  }

  for (id, account) in standing_now {
    if running.contains_key(&id) {
      continue;
    }

    if let Some(one) = start(standing, account) {
      running.insert(id, one);
    }
  }
}

/// Runs the watchers until told to stop: following the accounts as they change and every minute, sweeping
/// the staging folder every hour, and letting every watcher go at the end.
pub async fn watch(standing: Arc<Standing>, mut stop: watch::Receiver<bool>) {
  let mut checking = tokio::time::interval(ACCOUNTS_CHECK);
  let mut sweeping = tokio::time::interval(STAGING_SWEEP);
  let mut running: HashMap<i64, Running> = HashMap::new();

  loop {
    tokio::select! {
      () = stopped(&mut stop) => break,
      () = standing.accounts.changed() => follow(&standing, &mut running).await,
      _ = checking.tick() => follow(&standing, &mut running).await,
      _ = sweeping.tick() => {
        sweep_staging(&standing.settings.staging.dir, SystemTime::now());
      }
    }
  }

  futures::future::join_all(running.into_values().map(Running::stop)).await;
}
