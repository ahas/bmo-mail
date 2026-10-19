//! New mail the watcher found and left unread in an inbox, said on the event stream (`docs/api.md`, 「The
//! event stream」) with the notification it makes, without the watcher knowing who listens.

use serde::Serialize;
use serde_json::json;

use crate::events::Events;
use crate::watcher::notice::mail_push;
use crate::wire::MailAddress;
use crate::wording::Wording;

/// One message that arrived.
#[derive(Clone, Debug, Serialize)]
pub struct ArrivedMessage {
  pub folder: String,
  pub uid: u32,
  /// The conversation the message belongs to, for the address a notification opens.
  pub key: String,
  pub from: Vec<MailAddress>,
  pub subject: String,
  pub date: i64,
}

/// What arrived in one account's inbox at once.
#[derive(Clone, Debug)]
pub struct MailArrival {
  /// The account by its address.
  pub address: String,
  /// The account by the key the screen's addresses use.
  pub account: String,
  pub messages: Vec<ArrivedMessage>,
}

/// Says that an account's inbox changed.
pub fn announce_change(events: &Events, account: &str) {
  events.publish("mail", json!({ "account": account }));
}

/// Says what arrived, with the notification it makes.
pub fn announce_arrivals(events: &Events, wording: &Wording, arrival: &MailArrival) {
  events.publish(
    "arrival",
    json!({
      "account": arrival.account,
      "address": arrival.address,
      "messages": arrival.messages,
      "push": mail_push(arrival, wording),
    }),
  );
}
