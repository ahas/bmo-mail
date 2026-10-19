//! The messages the service itself moved into an inbox (`docs/mail.md`, 「Connections and the watcher」).
//! Such a message is not new mail: the watcher passes it without rules, address or push, and the summary
//! marks it so no tab raises a notice. A move into an inbox is begun before it is sent and noted with the
//! UIDs the server answered, and the watcher waits for every move begun to be noted before it handles
//! anything — the move's answer and the watcher's word of a grown inbox come over separate connections, in
//! either order.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{LazyLock, Mutex};

use tokio::sync::watch;

use crate::imap::Landed;

/// How many landed messages an account keeps: more than a tab's summary looks at.
const KEPT: usize = 500;

#[derive(Default)]
struct AccountMoves {
  landed: HashSet<String>,
  order: VecDeque<String>,
  pending: HashMap<u64, watch::Receiver<bool>>,
  next: u64,
}

static ACCOUNTS: LazyLock<Mutex<HashMap<String, AccountMoves>>> = LazyLock::new(Mutex::default);

fn accounts() -> std::sync::MutexGuard<'static, HashMap<String, AccountMoves>> {
  ACCOUNTS.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn landing_key(validity: &str, uid: u32) -> String {
  format!("{validity}:{uid}")
}

/// A move into an account's inbox, begun. It is noted when it is let go of, whether or not it succeeded,
/// with where the messages landed where the server said.
pub struct MoveIn {
  account: String,
  id: u64,
  landed: Option<Landed>,
  done: watch::Sender<bool>,
}

impl MoveIn {
  /// Where the messages landed: the inbox's UID validity and their new UIDs.
  pub fn landed(&mut self, landed: Option<Landed>) {
    self.landed = landed;
  }
}

impl Drop for MoveIn {
  fn drop(&mut self) {
    let mut accounts = accounts();
    let moves = accounts.entry(self.account.clone()).or_default();

    for uid in self.landed.as_ref().map(|landed| landed.uids.as_slice()).unwrap_or_default() {
      let key = landing_key(&self.landed.as_ref().expect("landed uids have their landing").validity, *uid);

      if moves.landed.insert(key.clone()) {
        moves.order.push_back(key);
      }
    }

    while moves.order.len() > KEPT {
      if let Some(oldest) = moves.order.pop_front() {
        moves.landed.remove(&oldest);
      }
    }

    moves.pending.remove(&self.id);
    let _ = self.done.send(true);
  }
}

/// Begins a move into an account's inbox.
pub fn begin_move_in(account: &str) -> MoveIn {
  let mut accounts = accounts();
  let moves = accounts.entry(account.to_owned()).or_default();
  let (done, waiting) = watch::channel(false);
  let id = moves.next;

  moves.next += 1;
  moves.pending.insert(id, waiting);

  MoveIn {
    account: account.to_owned(),
    id,
    landed: None,
    done,
  }
}

/// Waits until every move into the account's inbox begun so far has been noted.
pub async fn moves_settled(account: &str) {
  let pending: Vec<watch::Receiver<bool>> = accounts()
    .get(account)
    .map(|moves| moves.pending.values().cloned().collect())
    .unwrap_or_default();

  for mut waiting in pending {
    let _ = waiting.wait_for(|done| *done).await;
  }
}

/// Whether the service moved the message into the account's inbox.
pub fn moved_in(account: &str, validity: &str, uid: u32) -> bool {
  accounts()
    .get(account)
    .is_some_and(|moves| moves.landed.contains(&landing_key(validity, uid)))
}
