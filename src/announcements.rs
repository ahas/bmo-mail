//! What the service says on its event stream about messages and folders (`docs/api.md`, 「The event
//! stream」), whether the watcher or a route saw the change. Each event carries the member as a GET answers
//! it, a message without its bodies, or its key alone where it is gone.

use serde_json::json;

use crate::http::Standing;
use crate::imap::messages::{folder_state, read_row_of};
use crate::imap::session::MailSession;
use crate::keys::message_id;
use crate::mail::MailRef;
use crate::members::{folder, message};

pub const MESSAGE_CREATED: &str = "message.created";
pub const MESSAGE_CHANGED: &str = "message.changed";
pub const MESSAGE_DELETED: &str = "message.deleted";
pub const FOLDER_CHANGED: &str = "folder.changed";

/// Says a message came or changed, as it now stands; a message that cannot be read again is said nothing of.
pub async fn message_now(
  standing: &Standing,
  session: &MailSession,
  account: &str,
  kind: &'static str,
  at: &MailRef,
) {
  match read_row_of(session, at).await {
    Ok(Some(read)) => standing.events.publish(kind, Some(account), &message(account, read)).await,
    Ok(None) => {}
    Err(why) => eprintln!("mail: a message's change could not be read for the event stream: {why:?}"),
  }
}

/// Says a message is gone from where its id named it.
pub async fn message_gone(standing: &Standing, account: &str, at: &MailRef) {
  let gone = json!({ "accountName": account, "id": message_id(at) });

  standing.events.publish(MESSAGE_DELETED, Some(account), &gone).await;
}

/// Says a folder's counts may have moved, with the folder as it now stands.
pub async fn folder_now(standing: &Standing, session: &MailSession, account: &str, path: &str) {
  match folder_state(session, path).await {
    Ok(Some((index, state))) => {
      standing
        .events
        .publish(FOLDER_CHANGED, Some(account), &folder(account, index, state))
        .await;
    }
    Ok(None) => {}
    Err(why) => eprintln!("mail: a folder's change could not be read for the event stream: {why:?}"),
  }
}
