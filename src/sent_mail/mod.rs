//! A person's sent mail, read on their own machine (`docs/mail.md`, 「A person's sent mail, read on their own
//! machine」): the caller's own account and its own node read from agency, then the day's sent mail read on the
//! Outlook road, else on the Graph road, each message's subject, recipients and sending time alone.

pub mod call;
pub mod graph;
pub mod outlook;

use chrono::{DateTime, NaiveDate, NaiveTime, TimeDelta, Utc};
use chrono_tz::Tz;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::clients::accounts::Accounts;
use crate::clients::agency::Agency;
use crate::clients::receiver::Receiver;
use crate::settings::Settings;

/// What the tool is asked.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "mail_sent input")]
pub struct SentMailAsk {
  /// The calendar date to read, `YYYY-MM-DD`, in the service's time zone; today there where it is left out.
  #[serde(default)]
  pub day: Option<String>,
}

/// The road a day's sent mail was read on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Road {
  /// The running classic Outlook on the person's machine.
  Outlook,
  /// Microsoft Graph, with the token the person's own Teams page holds.
  Graph,
}

impl std::fmt::Display for Road {
  fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    out.write_str(match self {
      Self::Outlook => "Outlook",
      Self::Graph => "Graph",
    })
  }
}

/// Whether a recipient was written to, copied or blind-copied.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum RecipientKind {
  To,
  Cc,
  Bcc,
}

impl std::fmt::Display for RecipientKind {
  fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    out.write_str(match self {
      Self::To => "to",
      Self::Cc => "cc",
      Self::Bcc => "bcc",
    })
  }
}

/// One recipient of a sent message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Recipient {
  pub name: String,
  /// The address, an Exchange recipient's SMTP address where one is held; none where the road gave none.
  pub address: Option<String>,
  pub kind: RecipientKind,
}

/// One message sent: its subject, when it was sent and to whom; never its body or its attachments.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SentMessage {
  pub subject: String,
  /// When it was sent, in epoch milliseconds.
  pub sent_at: i64,
  pub recipients: Vec<Recipient>,
}

/// A day's sent mail of the caller's own person.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[schemars(title = "mail_sent output")]
pub struct SentMail {
  /// The day read, `YYYY-MM-DD`.
  pub day: String,
  pub road: Road,
  /// The person's node it was read on.
  pub node: String,
  /// The messages, in the order they were sent.
  pub messages: Vec<SentMessage>,
}

/// The tool's input and output schemas, as its folder keeps them in `input.json` and `output.json`.
pub fn schemas() -> (String, String) {
  let written = |schema: schemars::Schema| {
    let mut text = serde_json::to_string_pretty(&schema).unwrap_or_default();
    text.push('\n');
    text
  };

  (written(schemars::schema_for!(SentMailAsk)), written(schemars::schema_for!(SentMail)))
}

/// A calendar day in a time zone, from its midnight to the next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Day {
  pub date: NaiveDate,
  pub from: DateTime<Utc>,
  pub to: DateTime<Utc>,
}

impl Day {
  /// The day `asked` names in `zone`, or the day `now` falls on there where none is asked.
  pub fn of(asked: Option<&str>, zone: Tz, now: DateTime<Utc>) -> Result<Self, String> {
    let date = match asked {
      Some(asked) => NaiveDate::parse_from_str(asked, "%Y-%m-%d")
        .map_err(|_| format!("the day {asked} is not a date written YYYY-MM-DD"))?,
      None => now.with_timezone(&zone).date_naive(),
    };
    let next = date.succ_opt().ok_or_else(|| format!("the day {date} has no next day"))?;

    Ok(Self {
      date,
      from: midnight(date, zone),
      to: midnight(next, zone),
    })
  }
}

/// The moment `date` begins in `zone`: its midnight, or the first moment after a midnight the zone skips.
fn midnight(date: NaiveDate, zone: Tz) -> DateTime<Utc> {
  let start = date.and_time(NaiveTime::MIN);

  (0..=24)
    .find_map(|hours| (start + TimeDelta::hours(hours)).and_local_timezone(zone).earliest())
    .map_or_else(|| start.and_utc(), |moment| moment.with_timezone(&Utc))
}

/// The owner's account: the account the account service links to the receiver's owner sender; none where it is
/// linked to none.
async fn owner_account(settings: &Settings, http: &reqwest::Client) -> Result<Option<String>, String> {
  let receiver = Receiver::new(&settings.receiver, http.clone())
    .map_err(|why| format!("the receiver could not be asked: {why}"))?;
  let accounts = Accounts::new(&settings.account_service, http.clone())
    .map_err(|why| format!("the account service could not be asked: {why}"))?;
  let sender = receiver
    .owner_sender()
    .await
    .map_err(|why| format!("the receiver could not be asked: {why}"))?;

  accounts
    .user_of(sender)
    .await
    .map_err(|why| format!("the account service could not be asked: {why}"))
}

/// Where an account's sent mail is read: its own node, and the account agency's node records, the node service
/// and the Teams page service know it by.
struct Place {
  node: String,
  account: String,
}

/// The account's place. The owner's account is known by the account `outlook.owner_account` names and reads on
/// the node `outlook.owner_node` names, each where it names one; every account's node is otherwise the one
/// agency's record names under the account it is known by, where the record is that account's alone. `owner` is
/// the owner's account where it was read already.
async fn place(
  settings: &Settings,
  http: &reqwest::Client,
  agency: &Agency,
  account: &str,
  owner: Option<&str>,
) -> Result<Place, String> {
  let named_node = settings.outlook.owner_node.trim();
  let named_account = settings.outlook.owner_account.trim();
  let is_owner = if named_node.is_empty() && named_account.is_empty() {
    false
  } else {
    match owner {
      Some(owner) => owner == account,
      None => owner_account(settings, http).await?.as_deref() == Some(account),
    }
  };
  let known_by = if is_owner && !named_account.is_empty() { named_account } else { account }.to_owned();

  if is_owner && !named_node.is_empty() {
    return Ok(Place {
      node: named_node.to_owned(),
      account: known_by,
    });
  }

  match agency
    .person_node(&known_by)
    .await
    .map_err(|why| format!("agency could not be asked: {why}"))?
  {
    None => Err(format!("the account {known_by} has no machine of its own")),
    Some(record) if record.shared => Err(format!(
      "the account {known_by}'s node {} is shared with other accounts, so it is no machine of its own",
      record.node
    )),
    Some(record) => Ok(Place {
      node: record.node,
      account: known_by,
    }),
  }
}

/// The day's sent mail of the person `agent` acts for, read on their own node; refused with an English sentence.
pub async fn read(
  settings: &Settings,
  agent: &str,
  ask: &SentMailAsk,
  now: DateTime<Utc>,
) -> Result<SentMail, String> {
  let zone: Tz = settings
    .wording
    .time_zone
    .parse()
    .map_err(|_| format!("the time zone {} is none the tool knows", settings.wording.time_zone))?;
  let day = Day::of(ask.day.as_deref(), zone, now)?;
  let http = reqwest::Client::new();
  let unasked = |why| format!("agency could not be asked: {why}");
  let agency = Agency::new(&settings.agency, http.clone()).map_err(unasked)?;
  let (account, owner) = match agency.account_of(agent).await.map_err(unasked)? {
    Some(account) => (account, None),
    None => {
      let owner = owner_account(settings, &http).await?.ok_or_else(|| {
        format!("{agent} acts for the owner's account, which the owner's sender is linked to none of")
      })?;

      (owner.clone(), Some(owner))
    }
  };
  let Place { node, account } = place(settings, &http, &agency, &account, owner.as_deref()).await?;

  let (road, mut messages) = match outlook::read(&settings.node, &http, &node, &account, &day).await {
    Ok(messages) => (Road::Outlook, messages),
    Err(outlook) => {
      match graph::read(&settings.ms_teams, &settings.graph.address, &http, &account, &day).await {
        Ok(messages) => (Road::Graph, messages),
        Err(graph) => {
          return Err(format!("neither road stood on {node}: Outlook: {outlook}; Graph: {graph}"));
        }
      }
    }
  };
  messages.sort_by_key(|message| message.sent_at);

  Ok(SentMail {
    day: day.date.to_string(),
    road,
    node,
    messages,
  })
}
