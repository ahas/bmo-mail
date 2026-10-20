//! Filter rules and the senders always sent to spam (`docs/mail.md`, 「Filters and spam」). A rule is
//! conditions joined by all or any, and actions; rules run in their order, and one that moves the message
//! ends the run. The senders always sent to spam are asked after the rules, of a message the rules left
//! where it was.

use std::collections::HashMap;

use mail_parser::{Address, HeaderName, MessageParser};
use sqlx::SqlitePool;

use crate::imap::folders::{ensure_role, list_folders};
use crate::imap::messages::move_uids;
use crate::imap::session::{Connection, MailSession};
use crate::imap::{store_flags, uid_fetch, uid_search};
use crate::labels::{label_id_of, label_keyword, list_labels};
use crate::mail::{FolderRole, MailRule, RuleAction, RuleCondition, RuleDraft, RuleField, RuleMatch};
use crate::routes::requests::{Asked, MailFault, address_shaped};
use crate::wire::{RuleMatching, RulePatch};

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
      Self::Label { label_id } => add(label_keyword(label_id)),
      Self::Star => add("\\Flagged".to_owned()),
      Self::Read => add("\\Seen".to_owned()),
      Self::Move { folder_name } => plan.destination = Some(RuleDestination::Folder(folder_name.clone())),
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
        RuleAction::Label { label_id } => labels.contains(label_id),
        _ => true,
      });
      rule
    })
    .collect();

  Ok((rules, list_spam_senders(pool, account).await?))
}

/// What a run of the rules over a folder did: how many messages it moved and how many it labelled.
#[derive(Debug, Default)]
pub struct RulesRun {
  pub moved: u32,
  pub labelled: u32,
}

/// A run of the rules over a folder, for the mail that came before a rule existed: each message's facts read,
/// its plan made and carried out.
pub async fn run_rules_over_folder(
  pool: &SqlitePool,
  session: &MailSession,
  account: &str,
  folder: &str,
) -> Result<RulesRun, MailFault> {
  let (rules, spam_senders) = rule_context(pool, account).await?;
  let mut connection = session.folder(folder).await?;
  let done = async {
    let uids = uid_search(connection.session(), "ALL").await?;
    let mut run = RulesRun::default();

    if uids.is_empty() {
      return Ok(run);
    }

    let mut fetched = uid_fetch(connection.session(), &uids, "(UID BODY.PEEK[HEADER])").await?;

    fetched.sort_by_key(|fetch| fetch.uid);

    for fetch in &fetched {
      let Some(uid) = fetch.uid else {
        continue;
      };
      let plan = plan_rules(&rules, &spam_senders, &message_facts(fetch.header()));

      if plan.flags.is_empty() && plan.destination.is_none() {
        continue;
      }

      if plan.flags.iter().any(|flag| label_id_of(flag).is_some()) {
        run.labelled += 1;
      }

      if apply_plan(&mut connection, uid, &plan).await? {
        run.moved += 1;
      }
    }

    Ok(run)
  }
  .await;

  connection.settle(done)
}

/// The account's rules, in their order, each at its place in it from 1.
pub async fn list_rules(pool: &SqlitePool, account: &str) -> Result<Vec<MailRule>, MailFault> {
  let rows: Vec<(i64, i64, String, String, String)> = sqlx::query_as(
    "SELECT id, enabled, match, conditions, actions FROM rules WHERE account = ? ORDER BY position, id",
  )
  .bind(account)
  .fetch_all(pool)
  .await?;

  rows
    .into_iter()
    .enumerate()
    .map(|(index, (id, enabled, matching, conditions, actions))| {
      Ok(MailRule {
        id,
        position: index as i64 + 1,
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

/// A rule's match and actions, checked: at least one of each, every condition complete, every action naming
/// what it acts with.
pub fn checked_rule(
  matching: &RuleMatching,
  actions: &[RuleAction],
  on: bool,
) -> Result<RuleDraft, MailFault> {
  if matching.conditions.is_empty() || actions.is_empty() {
    return Err(Asked::bad_request("a rule has no condition or no action").into());
  }

  let conditions = matching
    .conditions
    .iter()
    .map(|condition| {
      if condition.contains.trim().is_empty() {
        return Err(Asked::bad_request("a condition contains nothing"));
      }

      let header = condition.header.as_deref().map(str::trim).filter(|header| !header.is_empty());

      if condition.field == RuleField::Header && header.is_none() {
        return Err(Asked::bad_request("a header condition names no header"));
      }

      Ok(RuleCondition {
        field: condition.field,
        header: if condition.field == RuleField::Header { header.map(str::to_owned) } else { None },
        contains: condition.contains.clone(),
      })
    })
    .collect::<Result<Vec<_>, _>>()?;

  for action in actions {
    let named = match action {
      RuleAction::Label { label_id } => !label_id.is_empty(),
      RuleAction::Move { folder_name } => !folder_name.is_empty(),
      _ => true,
    };

    if !named {
      return Err(Asked::bad_request("a label action names no label, or a move no folder").into());
    }
  }

  Ok(RuleDraft {
    enabled: on,
    matching: matching.kind,
    conditions,
    actions: actions.to_vec(),
  })
}

fn matching_name(matching: RuleMatch) -> &'static str {
  match matching {
    RuleMatch::All => "all",
    RuleMatch::Any => "any",
  }
}

/// Writes every rule of the account at its place in the order given.
async fn write_rules(pool: &SqlitePool, account: &str, rules: &[MailRule]) -> Result<(), MailFault> {
  let mut transaction = pool.begin().await?;

  for (index, rule) in rules.iter().enumerate() {
    sqlx::query(
      "UPDATE rules SET position = ?, enabled = ?, match = ?, conditions = ?, actions = ? WHERE account = ? AND id = ?",
    )
    .bind(index as i64 + 1)
    .bind(i64::from(rule.rule.enabled))
    .bind(matching_name(rule.rule.matching))
    .bind(serde_json::to_string(&rule.rule.conditions).map_err(MailFault::server)?)
    .bind(serde_json::to_string(&rule.rule.actions).map_err(MailFault::server)?)
    .bind(account)
    .bind(rule.id)
    .execute(&mut *transaction)
    .await?;
  }

  transaction.commit().await?;

  Ok(())
}

/// A new rule, placed after the others.
pub async fn create_rule(pool: &SqlitePool, account: &str, rule: RuleDraft) -> Result<MailRule, MailFault> {
  let position = list_rules(pool, account).await?.len() as i64 + 1;
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

/// A rule changed: the fields the patch names, and its place among the others, which close up around it; the
/// rule as it now stands.
pub async fn update_rule(
  pool: &SqlitePool,
  account: &str,
  id: i64,
  patch: &RulePatch,
) -> Result<MailRule, MailFault> {
  let mut rules = list_rules(pool, account).await?;
  let Some(at) = rules.iter().position(|rule| rule.id == id) else {
    return Err(Asked::not_found("no rule of the account has that id").into());
  };
  let mut rule = rules.remove(at);
  let matching = patch.matching.clone().unwrap_or_else(|| RuleMatching {
    kind: rule.rule.matching,
    conditions: rule.rule.conditions.clone(),
  });
  let actions = patch.actions.clone().unwrap_or_else(|| rule.rule.actions.clone());

  rule.rule = checked_rule(&matching, &actions, patch.on.unwrap_or(rule.rule.enabled))?;

  let place = patch.position.map_or(at, |position| (position.max(1) as usize - 1).min(rules.len()));

  rules.insert(place, rule);
  write_rules(pool, account, &rules).await?;

  let mut rule = rules.swap_remove(place);

  rule.position = place as i64 + 1;

  Ok(rule)
}

/// A rule deleted.
pub async fn delete_rule(pool: &SqlitePool, account: &str, id: i64) -> Result<(), MailFault> {
  let deleted = sqlx::query("DELETE FROM rules WHERE account = ? AND id = ?")
    .bind(account)
    .bind(id)
    .execute(pool)
    .await?;

  if deleted.rows_affected() == 0 {
    return Err(Asked::not_found("no rule of the account has that id").into());
  }

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

/// A sender whose mail always goes to spam, kept in lower case; a text that is not an address is refused.
/// Answers whether it was not one already.
pub async fn add_spam_sender(
  pool: &SqlitePool,
  account: &str,
  address: &str,
) -> Result<(String, bool), MailFault> {
  let clean = address.trim().to_lowercase();

  if !address_shaped(&clean) {
    return Err(Asked::bad_request("the sender is not an address").into());
  }

  let added = sqlx::query(
    "INSERT INTO spam_senders (account, address, created_at) VALUES (?, ?, ?) ON CONFLICT DO NOTHING",
  )
  .bind(account)
  .bind(&clean)
  .bind(chrono::Utc::now().timestamp_millis())
  .execute(pool)
  .await?;

  Ok((clean, added.rows_affected() > 0))
}

/// A sender taken off the always-spam list.
pub async fn remove_spam_sender(pool: &SqlitePool, account: &str, address: &str) -> Result<(), MailFault> {
  let removed = sqlx::query("DELETE FROM spam_senders WHERE account = ? AND address = ?")
    .bind(account)
    .bind(address.trim().to_lowercase())
    .execute(pool)
    .await?;

  if removed.rows_affected() == 0 {
    return Err(Asked::not_found("the sender is not on the always-spam list").into());
  }

  Ok(())
}
