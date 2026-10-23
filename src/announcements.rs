//! What the service says on its event stream about accounts, messages and folders (`docs/api.md`, 「The event
//! stream」), whether the watcher or a route saw the change. Each event is its account's owner's, and carries
//! the member as a GET answers it, a message without its bodies, or its key alone where it is gone.

use serde_json::json;

use crate::accounts::Account;
use crate::adapters::Mailbox;
use crate::http::Standing;
use crate::keys::message_id;
use crate::mail::MailRef;
use crate::members::{folder, message};

pub const MESSAGE_CREATED: &str = "message.created";
pub const MESSAGE_CHANGED: &str = "message.changed";
pub const MESSAGE_DELETED: &str = "message.deleted";
pub const FOLDER_CHANGED: &str = "folder.changed";
pub const ACCOUNT_CREATED: &str = "account.created";
pub const ACCOUNT_CHANGED: &str = "account.changed";
pub const ACCOUNT_DELETED: &str = "account.deleted";

/// Says a message came or changed, as it now stands; a message that cannot be read again is said nothing of.
pub async fn message_now(
  standing: &Standing,
  mailbox: &dyn Mailbox,
  account: &Account,
  kind: &'static str,
  at: &MailRef,
) {
  match mailbox.row(at).await {
    Ok(Some(read)) => {
      let member = message(&account.name, read);

      standing.events.publish(kind, &account.owner, Some(&account.name), &member).await;
    }
    Ok(None) => {}
    Err(why) => eprintln!("mail: a message's change could not be read for the event stream: {why:?}"),
  }
}

/// Says a message is gone from where its id named it.
pub async fn message_gone(standing: &Standing, account: &Account, at: &MailRef) {
  let gone = json!({ "accountName": account.name, "id": message_id(at) });

  standing
    .events
    .publish(MESSAGE_DELETED, &account.owner, Some(&account.name), &gone)
    .await;
}

/// Says a folder's counts may have moved, with the folder as it now stands.
pub async fn folder_now(standing: &Standing, mailbox: &dyn Mailbox, account: &Account, path: &str) {
  match mailbox.folder_state(path).await {
    Ok(Some((index, state))) => {
      let member = folder(&account.name, index, state);

      standing
        .events
        .publish(FOLDER_CHANGED, &account.owner, Some(&account.name), &member)
        .await;
    }
    Ok(None) => {}
    Err(why) => eprintln!("mail: a folder's change could not be read for the event stream: {why:?}"),
  }
}
