//! One call of the hosted tool, in the tool host's call protocol 1 (`docs/api.md`, 「The hosted tool」): the call
//! and its session record read from standard input, the deployment's settings from the data root the record
//! names, and one tool result answered.

use std::collections::BTreeMap;
use std::path::Path;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Recipient, RecipientKind, SentMail, SentMailAsk};
use crate::settings::Settings;

/// The entry this tool answers.
pub const ENTRY: &str = "bmo/mail_sent";
/// The record's variable naming the data root.
pub const DATA_ROOT_VARIABLE: &str = "BMO_DATA_DIR";

#[derive(Deserialize)]
struct Call {
  protocol: u32,
  kind: String,
  #[serde(default)]
  entry: Option<String>,
  #[serde(default)]
  arguments: Option<Value>,
  session: Session,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Session {
  holder_name: String,
  #[serde(default)]
  variables: BTreeMap<String, String>,
}

/// A tool result a model reads as a refusal.
fn refusal(why: &str) -> Value {
  json!({ "content": [{ "type": "text", "text": why }], "isError": true })
}

/// The tool result answering `input`, the call as the host hands it, with `now` the moment a day left out is
/// read from.
pub async fn answer(input: &str, now: DateTime<Utc>) -> Value {
  let call: Call = match serde_json::from_str(input) {
    Ok(call) => call,
    Err(why) => return refusal(&format!("ERROR: the call does not read: {why}")),
  };

  if call.protocol != 1 || call.kind != "call" || call.entry.as_deref() != Some(ENTRY) {
    return refusal(&format!("ERROR: this tool answers a protocol 1 call of {ENTRY} alone"));
  }

  let ask: SentMailAsk = match serde_json::from_value(call.arguments.unwrap_or_else(|| json!({}))) {
    Ok(ask) => ask,
    Err(why) => return refusal(&format!("The arguments are not the tool's: {why}")),
  };
  let Some(data_root) = call.session.variables.get(DATA_ROOT_VARIABLE) else {
    return refusal(&format!("ERROR: the session record names no data root in {DATA_ROOT_VARIABLE}"));
  };
  let settings = match Settings::deployed(Path::new(data_root)) {
    Ok(settings) => settings,
    Err(why) => return refusal(&format!("ERROR: {why}")),
  };

  match super::read(&settings, &call.session.holder_name, &ask, now).await {
    Ok(sent) => {
      let zone: Tz = settings.wording.time_zone.parse().unwrap_or(Tz::UTC);

      json!({
        "content": [{ "type": "text", "text": reading(&sent, zone) }],
        "isError": false,
        "structuredContent": sent,
      })
    }
    Err(why) => refusal(&why),
  }
}

/// The answer as a person reads it: a line for the day, then a line a message, its time in `zone`.
pub fn reading(sent: &SentMail, zone: Tz) -> String {
  let head = format!(
    "{} messages sent on {}, read from {} on {}.",
    sent.messages.len(),
    sent.day,
    sent.road,
    sent.node
  );
  let lines = sent.messages.iter().map(|message| {
    let at = DateTime::from_timestamp_millis(message.sent_at)
      .map(|at| at.with_timezone(&zone).format("%H:%M").to_string())
      .unwrap_or_default();
    let to = [RecipientKind::To, RecipientKind::Cc, RecipientKind::Bcc]
      .into_iter()
      .filter_map(|kind| {
        let named: Vec<String> = message
          .recipients
          .iter()
          .filter(|recipient| recipient.kind == kind)
          .map(written)
          .collect();
        (!named.is_empty()).then(|| format!("{kind}: {}", named.join(", ")))
      })
      .collect::<Vec<_>>()
      .join("; ");

    format!("{at} {} ({to})", message.subject)
  });

  std::iter::once(head).chain(lines).collect::<Vec<_>>().join("\n")
}

/// A recipient as a line writes it: the name and the address in angle brackets, either alone where the other is
/// missing.
fn written(recipient: &Recipient) -> String {
  match (recipient.name.trim(), recipient.address.as_deref()) {
    ("", Some(address)) => address.to_owned(),
    (name, Some(address)) if name != address => format!("{name} <{address}>"),
    (name, _) => name.to_owned(),
  }
}
