//! Sending (`docs/mail.md`, 「Writing」): a send waits in the service for the account's undo span while it can
//! still be taken back, then the account's mailbox sends the draft and the recipients become known addresses.
//! A service that stops while a send waits leaves the draft where it was, never a message half sent.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rand::RngCore;
use tokio::task::AbortHandle;

use crate::accounts::Account;
use crate::adapters::Mailbox;
use crate::http::Standing;
use crate::routes::requests::{Asked, MailFault};
use crate::wire::{Send, SendState};
use crate::writing::addresses::record_addresses;

/// The kind of the event a send's change makes.
pub const SEND_CHANGED: &str = "send.changed";
/// How long a settled send is kept for a reader to ask about.
const SETTLED_KEPT: Duration = Duration::from_secs(60);

/// A send kept in the service: the account it is of, where it stands, and its timer while it still waits.
struct Held {
  account: i64,
  send: Send,
  timer: Option<AbortHandle>,
}

/// The sends kept.
#[derive(Default)]
pub struct Sends {
  held: Mutex<HashMap<String, Held>>,
}

impl std::fmt::Debug for Sends {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter.debug_struct("Sends").finish_non_exhaustive()
  }
}

fn held(sends: &Sends) -> std::sync::MutexGuard<'_, HashMap<String, Held>> {
  sends.held.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Sends {
  /// A send of the account as it stands; none once it has long ended.
  pub fn send(&self, account: i64, id: &str) -> Option<Send> {
    held(self)
      .get(id)
      .filter(|entry| entry.account == account)
      .map(|entry| entry.send.clone())
  }

  /// Takes a send back while it waits: the send as it now stands, cancelled; a conflict where it has already
  /// gone, and none where the account has no such send.
  pub fn cancel(&self, account: i64, id: &str) -> Option<Result<Send, Asked>> {
    let mut held = held(self);
    let entry = held.get_mut(id).filter(|entry| entry.account == account)?;
    let Some(timer) = entry.timer.take() else {
      return Some(Err(Asked::conflict("the send has already gone")));
    };

    timer.abort();
    entry.send.state = SendState::Cancelled;

    Some(Ok(entry.send.clone()))
  }

  /// Takes back every send of the account still waiting, and forgets every send of it, as the account is
  /// removed; their drafts stay.
  pub fn cancel_account(&self, account: i64) {
    let mut held = held(self);

    for entry in held.values_mut().filter(|entry| entry.account == account) {
      if let Some(timer) = entry.timer.take() {
        timer.abort();
      }
    }

    held.retain(|_, entry| entry.account != account);
  }

  /// Moves a send to where it now stands, and answers it.
  fn settle(&self, id: &str, state: SendState, failure: Option<String>) -> Option<Send> {
    let mut held = held(self);
    let entry = held.get_mut(id)?;

    entry.send.state = state;
    entry.send.failure = failure;

    Some(entry.send.clone())
  }
}

fn random_hex(bytes: usize) -> String {
  let mut random = vec![0u8; bytes];

  rand::rng().fill_bytes(&mut random);
  random.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Why a send failed, in words for the log and the send's `failure`.
fn failure(fault: MailFault) -> String {
  match fault {
    MailFault::Asked(asked) => asked.error,
    MailFault::Server(why) => why,
  }
}

/// A send of the draft a `Message-ID` names: it waits until `send_at`, and then goes. A draft that is not there,
/// or has no recipient, is refused before anything waits.
pub async fn queue_send(
  standing: Arc<Standing>,
  mailbox: Arc<dyn Mailbox>,
  account: &Account,
  draft_id: &str,
  send_at: i64,
) -> Result<Send, MailFault> {
  let draft = mailbox.draft(draft_id).await?;

  if draft.to.len() + draft.cc.len() + draft.bcc.len() == 0 {
    return Err(Asked::rule("no-recipients", "a message to send has no recipient").into());
  }

  let id = random_hex(9);
  let send = Send {
    id: id.clone(),
    account_name: account.name.clone(),
    draft_id: draft_id.to_owned(),
    state: SendState::Waiting,
    send_at,
    failure: None,
  };

  held(&standing.sends).insert(
    id.clone(),
    Held {
      account: account.id,
      send: send.clone(),
      timer: None,
    },
  );

  let task = {
    let standing = Arc::clone(&standing);
    let id = id.clone();
    let draft_id = draft_id.to_owned();
    let account = account.clone();
    let wait =
      Duration::from_millis(u64::try_from(send_at - chrono::Utc::now().timestamp_millis()).unwrap_or(0));

    tokio::spawn(async move {
      tokio::time::sleep(wait).await;

      // The wait has ended: from here the send can no longer be taken back.
      let fired = held(&standing.sends).get_mut(&id).and_then(|entry| entry.timer.take()).is_some();

      if !fired {
        return;
      }

      let sent = match mailbox.send_draft(&draft_id).await {
        Ok(recipients) => {
          let now = chrono::Utc::now().timestamp_millis();

          record_addresses(standing.store.pool(), &account.store_key(), &account.address, &recipients, now)
            .await
        }
        Err(fault) => Err(fault),
      };
      let settled = match sent {
        Ok(()) => standing.sends.settle(&id, SendState::Sent, None),
        Err(fault) => standing.sends.settle(&id, SendState::Failed, Some(failure(fault))),
      };

      if let Some(send) = settled {
        standing.events.publish(SEND_CHANGED, &account.owner, Some(&account.name), &send).await;
      }

      tokio::time::sleep(SETTLED_KEPT).await;
      held(&standing.sends).remove(&id);
    })
  };

  if let Some(entry) = held(&standing.sends).get_mut(&id) {
    entry.timer = Some(task.abort_handle());
  }

  Ok(send)
}

/// Forgets a cancelled send once a reader has had its while to ask about it.
pub fn forget_later(standing: Arc<Standing>, id: String) {
  tokio::spawn(async move {
    tokio::time::sleep(SETTLED_KEPT).await;
    held(&standing.sends).remove(&id);
  });
}
