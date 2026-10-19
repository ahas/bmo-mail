//! Filter rules and the senders always sent to spam (`docs/mail.md`, 「Filters and spam」). A rule is
//! conditions joined by all or any, and actions; rules run in their order, and one that moves the message
//! ends the run. The senders always sent to spam are asked after the rules, of a message the rules left
//! where it was.

use std::collections::HashMap;

use mail_parser::{Address, HeaderName, MessageParser};
use serde_json::Value;
use sqlx::SqlitePool;

use crate::imap::folders::{ensure_role, list_folders};
use crate::imap::messages::move_uids;
use crate::imap::session::{Connection, MailSession};
use crate::imap::{store_flags, uid_fetch, uid_search};
use crate::labels::{label_keyword, list_labels};
use crate::routes::requests::MailFault;
use crate::wire::{FolderRole, MailRule, RuleAction, RuleCondition, RuleDraft, RuleField, RuleMatch};

/// What a rule is judged against: a message's senders and recipients (addresses and names), its subject
/// and its headers.
#[derive(Debug, Default)]
pub struct MessageFacts {
  pub from: Vec<String>,
  pub to: Vec<String>,
  pub subject: String,
  /// Each header's first value, by its lower-case name.
  pub headers: HashMap<String, String>,
}

/// Where a message goes when a rule or a spam sender moves it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuleDestination {
  Spam,
  Trash,
  Folder(String),
}

/// What the rules and the spam senders do to one message.
#[derive(Debug, Default)]
pub struct RulePlan {
  /// Flags and keywords to set.
  pub flags: Vec<String>,
  pub destination: Option<RuleDestination>,
  /// The rules that matched, by id.
  pub matched: Vec<i64>,
  /// Whether an always-spam sender did.
  pub spam_sender: bool,
}

/// The rules the settings offer ready-made: the verdicts a spam filter on the server writes into a header,
/// so a filter installed there later reaches the screen with no change here.
pub fn spam_header_rules() -> Vec<RuleDraft> {
  [("X-Spam-Flag", "YES"), ("X-Bogosity", "Spam")]
    .into_iter()
    .map(|(header, contains)| RuleDraft {
      enabled: true,
      matching: RuleMatch::Any,
      conditions: vec![RuleCondition {
        field: RuleField::Header,
        header: Some(header.to_owned()),
        contains: contains.to_owned(),
      }],
      actions: vec![RuleAction::Spam],
    })
    .collect()
}

impl RuleCondition {
  /// Whether the message holds the condition's text in the field it reads, without regard to case.
  fn holds(&self, facts: &MessageFacts) -> bool {
    let wanted = self.contains.trim().to_lowercase();

    if wanted.is_empty() {
      return false;
    }

    let has = |text: &str| text.to_lowercase().contains(&wanted);

    match self.field {
      RuleField::From => facts.from.iter().any(|one| has(one)),
      RuleField::To => facts.to.iter().any(|one| has(one)),
      RuleField::Subject => has(&facts.subject),
      RuleField::Header => {
        let name = self.header.as_deref().unwrap_or("").trim().to_lowercase();

        has(facts.headers.get(&name).map_or("", String::as_str))
      }
    }
  }
}

/// Whether a rule takes the message.
pub fn rule_matches(rule: &MailRule, facts: &MessageFacts) -> bool {
  let draft = &rule.rule;

  if !draft.enabled || draft.conditions.is_empty() {
    return false;
  }

  match draft.matching {
    RuleMatch::All => draft.conditions.iter().all(|condition| condition.holds(facts)),
    RuleMatch::Any => draft.conditions.iter().any(|condition| condition.holds(facts)),
  }
}

impl RuleAction {
  /// What the action adds to a plan: a flag, or where the message goes.
  fn plan(&self, plan: &mut RulePlan) {
    let mut add = |flag: String| {
      if !plan.flags.contains(&flag) {
        plan.flags.push(flag);
      }
    };

    match self {
      Self::Label { label } => add(label_keyword(label)),
      Self::Star => add("\\Flagged".to_owned()),
      Self::Read => add("\\Seen".to_owned()),
      Self::Move { folder } => plan.destination = Some(RuleDestination::Folder(folder.clone())),
      Self::Spam => plan.destination = Some(RuleDestination::Spam),
      Self::Trash => plan.destination = Some(RuleDestination::Trash),
    }
  }
}

/// What the rules and the spam senders do to one message.
pub fn plan_rules(rules: &[MailRule], spam_senders: &[String], facts: &MessageFacts) -> RulePlan {
  let mut plan = RulePlan::default();
  let mut ordered: Vec<&MailRule> = rules.iter().collect();

  ordered.sort_by_key(|rule| rule.position);

  for rule in ordered {
    if !rule_matches(rule, facts) {
      continue;
    }

    plan.matched.push(rule.id);

    for action in &rule.rule.actions {
      action.plan(&mut plan);
    }

    if plan.destination.is_some() {
      return plan;
    }
  }

  if facts
    .from
    .iter()
    .any(|one| spam_senders.iter().any(|sender| sender.eq_ignore_ascii_case(one)))
  {
    plan.spam_sender = true;
    plan.destination = Some(RuleDestination::Spam);
  }

  plan
}

fn people(address: Option<&Address<'_>>) -> Vec<String> {
  address
    .map(|address| {
      address
        .iter()
        .flat_map(|one| [one.address().map(str::to_lowercase), one.name().map(str::to_owned)])
        .flatten()
        .filter(|part| !part.is_empty())
        .collect()
    })
    .unwrap_or_default()
}

/// A message's facts from its whole header.
pub fn message_facts(header: Option<&[u8]>) -> MessageFacts {
  let Some(header) = header else {
    return MessageFacts::default();
  };
  let parser = MessageParser::new()
    .with_address_headers()
    .header_text(HeaderName::Subject)
    .default_header_text();
  let Some(message) = parser.parse_headers(header) else {
    return MessageFacts::default();
  };
  let mut headers = HashMap::new();

  for one in message.root_part().headers() {
    let name = one.name().to_lowercase();

    if headers.contains_key(&name) {
      continue;
    }

    let value = one
      .value()
      .as_text()
      .map(str::to_owned)
      .or_else(|| message.header_raw(one.name()).map(str::to_owned));

    headers.insert(name, value.unwrap_or_default().trim().to_owned());
  }

  MessageFacts {
    from: people(message.from()),
    to: people(message.to()).into_iter().chain(people(message.cc())).collect(),
    subject: message.subject().unwrap_or("").to_owned(),
    headers,
  }
}

/// Carries a plan out on a message of the folder the connection has selected. Answers whether the message
/// was moved out of it.
pub async fn apply_plan(connection: &mut Connection, uid: u32, plan: &RulePlan) -> Result<bool, MailFault> {
  let mut flags = plan.flags.clone();

  if plan.destination == Some(RuleDestination::Spam) {
    flags.push("$Junk".to_owned());
  }

  store_flags(connection.session(), &[uid], "+", &flags).await?;

  let path = match &plan.destination {
    None => return Ok(false),
    Some(RuleDestination::Folder(path)) => {
      // A rule naming a folder that has since gone moves nothing.
      if !list_folders(connection).await?.iter().any(|folder| &folder.path == path) {
        return Ok(false);
      }

      path.clone()
    }
    Some(RuleDestination::Spam) => ensure_role(connection, FolderRole::Junk).await?,
    Some(RuleDestination::Trash) => ensure_role(connection, FolderRole::Trash).await?,
  };

  if connection.selected().as_deref() == Some(path.as_str()) {
    return Ok(false);
  }

  move_uids(connection, &[uid], &path).await?;

  Ok(true)
}

/// An account's rules as they stand now, and its always-spam senders. A label deleted since a rule was
/// written is no action at all.
pub async fn rule_context(
  pool: &SqlitePool,
  account: &str,
) -> Result<(Vec<MailRule>, Vec<String>), MailFault> {
  let labels: Vec<String> = list_labels(pool, account).await?.into_iter().map(|label| label.id).collect();
  let rules = list_rules(pool, account)
    .await?
    .into_iter()
    .map(|mut rule| {
      rule.rule.actions.retain(|action| match action {
        RuleAction::Label { label } => labels.contains(label),
        _ => true,
      });
      rule
    })
    .collect();

  Ok((rules, list_spam_senders(pool, account).await?))
}

/// What rules do to messages of the selected folder: each one's facts read, its plan made and carried out.
/// Answers which messages stayed and which were moved.
pub async fn run_rules_in_selected(
  connection: &mut Connection,
  pool: &SqlitePool,
  account: &str,
  uids: &[u32],
) -> Result<(Vec<u32>, Vec<u32>), MailFault> {
  let mut kept = Vec::new();
  let mut moved = Vec::new();

  if uids.is_empty() {
    return Ok((kept, moved));
  }

  let (rules, spam_senders) = rule_context(pool, account).await?;
  let mut fetched = uid_fetch(connection.session(), uids, "(UID BODY.PEEK[HEADER])").await?;

  fetched.sort_by_key(|fetch| fetch.uid);

  for fetch in &fetched {
    let Some(uid) = fetch.uid else {
      continue;
    };
    let plan = plan_rules(&rules, &spam_senders, &message_facts(fetch.header()));
    let acts = !plan.flags.is_empty() || plan.destination.is_some();

    if acts && apply_plan(connection, uid, &plan).await? {
      moved.push(uid);
    } else {
      kept.push(uid);
    }
  }

  Ok((kept, moved))
}

/// A rule run over a folder, for the mail that came before the rule existed: how many messages stayed and
/// how many were moved.
pub async fn run_rules_over_folder(
  pool: &SqlitePool,
  session: &MailSession,
  account: &str,
  folder: &str,
) -> Result<(usize, usize), MailFault> {
  let mut connection = session.folder(folder).await?;
  let done = async {
    let uids = uid_search(connection.session(), "ALL").await?;
    let (kept, moved) = run_rules_in_selected(&mut connection, pool, account, &uids).await?;

    Ok((kept.len(), moved.len()))
  }
  .await;

  connection.settle(done)
}

/// The account's rules, in their order.
pub async fn list_rules(pool: &SqlitePool, account: &str) -> Result<Vec<MailRule>, MailFault> {
  let rows: Vec<(i64, i64, i64, String, String, String)> = sqlx::query_as(
    "SELECT id, position, enabled, match, conditions, actions FROM rules WHERE account = ? ORDER BY position, id",
  )
  .bind(account)
  .fetch_all(pool)
  .await?;

  rows
    .into_iter()
    .map(|(id, position, enabled, matching, conditions, actions)| {
      Ok(MailRule {
        id,
        position,
        rule: RuleDraft {
          enabled: enabled == 1,
          matching: if matching == "all" { RuleMatch::All } else { RuleMatch::Any },
          conditions: serde_json::from_str(&conditions).map_err(MailFault::server)?,
          actions: serde_json::from_str(&actions).map_err(MailFault::server)?,
        },
      })
    })
    .collect()
}

fn refused(code: &'static str, error: &str) -> MailFault {
  MailFault::asked(code, error)
}

/// A rule as the screen sent it, checked: every condition complete, every action one the engine knows.
pub fn checked_rule(value: &Value) -> Result<RuleDraft, MailFault> {
  let matching = match value.get("match").and_then(Value::as_str) {
    Some("all") => RuleMatch::All,
    Some("any") => RuleMatch::Any,
    _ => return Err(refused("bad_rule_match", "the match is neither all nor any")),
  };
  let empty = Vec::new();
  let conditions = value.get("conditions").and_then(Value::as_array).unwrap_or(&empty);
  let actions = value.get("actions").and_then(Value::as_array).unwrap_or(&empty);

  if conditions.is_empty() || actions.is_empty() {
    return Err(refused("rule_empty", "a rule has no condition or no action"));
  }

  let conditions = conditions
    .iter()
    .map(|condition| {
      let field = condition
        .get("field")
        .and_then(|field| serde_json::from_value::<RuleField>(field.clone()).ok());
      let contains =
        condition.get("contains").and_then(Value::as_str).filter(|text| !text.trim().is_empty());
      let (Some(field), Some(contains)) = (field, contains) else {
        return Err(refused(
          "bad_condition",
          "a condition's field is none of the kinds, or it contains nothing",
        ));
      };
      let header = condition.get("header").and_then(Value::as_str).map(str::to_owned);

      if field == RuleField::Header && header.as_deref().unwrap_or("").trim().is_empty() {
        return Err(refused("header_name_needed", "a header condition names no header"));
      }

      Ok(RuleCondition {
        field,
        header,
        contains: contains.to_owned(),
      })
    })
    .collect::<Result<Vec<_>, _>>()?;
  let actions = actions
    .iter()
    .map(|action| {
      let target = |name: &str| {
        action
          .get(name)
          .and_then(Value::as_str)
          .filter(|target| !target.is_empty())
          .map(str::to_owned)
      };
      let needed = || refused("rule_target_needed", "a label action names no label, or a move no folder");

      match action.get("kind").and_then(Value::as_str) {
        Some("label") => Ok(RuleAction::Label {
          label: target("label").ok_or_else(needed)?,
        }),
        Some("star") => Ok(RuleAction::Star),
        Some("read") => Ok(RuleAction::Read),
        Some("move") => Ok(RuleAction::Move {
          folder: target("folder").ok_or_else(needed)?,
        }),
        Some("spam") => Ok(RuleAction::Spam),
        Some("trash") => Ok(RuleAction::Trash),
        _ => Err(refused("bad_rule_action", "an action is none of the kinds")),
      }
    })
    .collect::<Result<Vec<_>, _>>()?;

  Ok(RuleDraft {
    enabled: value.get("enabled") != Some(&Value::Bool(false)),
    matching,
    conditions,
    actions,
  })
}

fn matching_name(matching: RuleMatch) -> &'static str {
  match matching {
    RuleMatch::All => "all",
    RuleMatch::Any => "any",
  }
}

/// A new rule, placed after the others.
pub async fn create_rule(pool: &SqlitePool, account: &str, value: &Value) -> Result<MailRule, MailFault> {
  let rule = checked_rule(value)?;
  let top: Option<i64> = sqlx::query_scalar("SELECT MAX(position) FROM rules WHERE account = ?")
    .bind(account)
    .fetch_one(pool)
    .await?;
  let position = top.unwrap_or(-1) + 1;
  let id: i64 = sqlx::query_scalar(
    "INSERT INTO rules (account, position, enabled, match, conditions, actions, created_at) VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING id",
  )
  .bind(account)
  .bind(position)
  .bind(i64::from(rule.enabled))
  .bind(matching_name(rule.matching))
  .bind(serde_json::to_string(&rule.conditions).map_err(MailFault::server)?)
  .bind(serde_json::to_string(&rule.actions).map_err(MailFault::server)?)
  .bind(chrono::Utc::now().timestamp_millis())
  .fetch_one(pool)
  .await?;

  Ok(MailRule { id, position, rule })
}

/// A rule written again: its conditions, its actions, on or off.
pub async fn update_rule(
  pool: &SqlitePool,
  account: &str,
  id: i64,
  value: &Value,
) -> Result<(), MailFault> {
  let rule = checked_rule(value)?;
  let changed = sqlx::query(
    "UPDATE rules SET enabled = ?, match = ?, conditions = ?, actions = ? WHERE account = ? AND id = ?",
  )
  .bind(i64::from(rule.enabled))
  .bind(matching_name(rule.matching))
  .bind(serde_json::to_string(&rule.conditions).map_err(MailFault::server)?)
  .bind(serde_json::to_string(&rule.actions).map_err(MailFault::server)?)
  .bind(account)
  .bind(id)
  .execute(pool)
  .await?;

  if changed.rows_affected() == 0 {
    return Err(refused("rule_not_found", "no rule of the account has that id"));
  }

  Ok(())
}

/// A rule deleted.
pub async fn delete_rule(pool: &SqlitePool, account: &str, id: i64) -> Result<(), MailFault> {
  sqlx::query("DELETE FROM rules WHERE account = ? AND id = ?")
    .bind(account)
    .bind(id)
    .execute(pool)
    .await?;

  Ok(())
}

/// Puts the rules in the order given; rules left out keep their places after them.
pub async fn order_rules(pool: &SqlitePool, account: &str, ids: &[i64]) -> Result<(), MailFault> {
  let rest: Vec<i64> = list_rules(pool, account)
    .await?
    .into_iter()
    .map(|rule| rule.id)
    .filter(|id| !ids.contains(id))
    .collect();
  let mut transaction = pool.begin().await?;

  for (position, id) in ids.iter().chain(&rest).enumerate() {
    sqlx::query("UPDATE rules SET position = ? WHERE account = ? AND id = ?")
      .bind(position as i64)
      .bind(account)
      .bind(id)
      .execute(&mut *transaction)
      .await?;
  }

  transaction.commit().await?;

  Ok(())
}

/// The account's always-spam senders, in order.
pub async fn list_spam_senders(pool: &SqlitePool, account: &str) -> Result<Vec<String>, MailFault> {
  Ok(
    sqlx::query_scalar("SELECT address FROM spam_senders WHERE account = ? ORDER BY address")
      .bind(account)
      .fetch_all(pool)
      .await?,
  )
}

/// Whether a text has the shape `<local>@<domain>`, neither part holding `@` or a space.
fn address_shaped(address: &str) -> bool {
  let plain = |part: &str| !part.is_empty() && !part.chars().any(|c| c == '@' || c.is_whitespace());

  address.split_once('@').is_some_and(|(local, domain)| plain(local) && plain(domain))
}

/// Senders whose mail always goes to spam; a text that is not an address is passed over.
pub async fn add_spam_senders(
  pool: &SqlitePool,
  account: &str,
  addresses: &[String],
) -> Result<(), MailFault> {
  let mut transaction = pool.begin().await?;

  for address in addresses {
    let clean = address.trim().to_lowercase();

    if address_shaped(&clean) {
      sqlx::query(
        "INSERT INTO spam_senders (account, address, created_at) VALUES (?, ?, ?) ON CONFLICT DO NOTHING",
      )
      .bind(account)
      .bind(&clean)
      .bind(chrono::Utc::now().timestamp_millis())
      .execute(&mut *transaction)
      .await?;
    }
  }

  transaction.commit().await?;

  Ok(())
}

/// A sender taken off the always-spam list.
pub async fn remove_spam_sender(pool: &SqlitePool, account: &str, address: &str) -> Result<(), MailFault> {
  sqlx::query("DELETE FROM spam_senders WHERE account = ? AND address = ?")
    .bind(account)
    .bind(address.trim().to_lowercase())
    .execute(pool)
    .await?;

  Ok(())
}
