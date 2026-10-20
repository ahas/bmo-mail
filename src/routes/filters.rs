//! Filter rules, their runs, the senders always sent to spam, and an account's settings (`docs/api.md`,
//! 「Filters, spam senders and settings」).

use axum::http::{Method, StatusCode};
use axum::response::Response;

use crate::account_settings::{MOST_UNDO_SEND_MS, read_settings, write_settings};
use crate::http::{Answer, Call, Operation, Refused, Route, answer, inline, nothing, schema};
use crate::imap::folders::folders_of;
use crate::members::{rule, settings};
use crate::routes::requests::{Asked, with_account};
use crate::rules::{
  add_spam_sender, checked_rule, create_rule, delete_rule, list_rules, list_spam_senders,
  remove_spam_sender, run_rules_over_folder, update_rule,
};
use crate::wire::{
  ListQuery, NewRule, NewRuleRun, Page, Rule, RulePatch, RuleRun, Settings, SettingsPatch, SpamSender,
};

const ACCOUNT: (&str, &str) = ("accountName", "The account's name: its address's local part.");
const RULE: (&str, &str) = ("ruleId", "The rule's id.");
const ADDRESS: (&str, &str) = ("address", "The sender's address.");

/// The longest signature an account may keep, in characters.
const SIGNATURE_LIMIT: usize = 200_000;

/// A rule's id from the path; one that is no number names no rule.
fn rule_id(text: &str) -> Result<i64, Asked> {
  text.parse::<i64>().map_err(|_| Asked::not_found("no rule of the account has that id"))
}

/// `GET /accounts/{accountName}/rules`.
pub struct Rules;

impl Route for Rules {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/accounts/{accountName}/rules";
  const OPERATION: Operation = Operation {
    id: "listAccountsRulesByAccountName",
    summary: "An account's rules in order.",
    tag: "accounts",
    keys: &[ACCOUNT],
    query: Some(inline::<ListQuery>),
    body: None,
    status: 200,
    answer: Answer::Json(schema::<Page<Rule>>),
    codes: &["not-found"],
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let query: ListQuery = call.query()?;

    with_account(&mut call, |mail| async move {
      let rules: Vec<Rule> = list_rules(mail.standing.store.pool(), &mail.account.address)
        .await?
        .into_iter()
        .map(rule)
        .collect();

      Ok(answer(StatusCode::OK, &Page::by_position(rules, |rule| rule.position, &query)))
    })
    .await
  }
}

crate::route!(Rules);

/// `POST /accounts/{accountName}/rules`.
pub struct CreateRule;

impl Route for CreateRule {
  const METHOD: Method = Method::POST;
  const PATH: &'static str = "/accounts/{accountName}/rules";
  const OPERATION: Operation = Operation {
    id: "createAccountsRulesByAccountName",
    summary: "Add a rule, placed after the others.",
    tag: "accounts",
    keys: &[ACCOUNT],
    query: None,
    body: Some(schema::<NewRule>),
    status: 201,
    answer: Answer::Json(schema::<Rule>),
    codes: &["not-found"],
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let new: NewRule = call.body().await?;

    with_account(&mut call, |mail| async move {
      let draft = checked_rule(&new.matching, &new.actions, new.on)?;
      let made = create_rule(mail.standing.store.pool(), &mail.account.address, draft).await?;

      Ok(answer(StatusCode::CREATED, &rule(made)))
    })
    .await
  }
}

crate::route!(CreateRule);

/// `PATCH /accounts/{accountName}/rules/{ruleId}`.
pub struct ChangeRule;

impl Route for ChangeRule {
  const METHOD: Method = Method::PATCH;
  const PATH: &'static str = "/accounts/{accountName}/rules/{ruleId}";
  const OPERATION: Operation = Operation {
    id: "changeAccountsRulesByAccountNameRuleId",
    summary: "Change a rule, its place in the order among its fields.",
    tag: "accounts",
    keys: &[ACCOUNT, RULE],
    query: None,
    body: Some(schema::<RulePatch>),
    status: 200,
    answer: Answer::Json(schema::<Rule>),
    codes: &["not-found"],
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let id = call.key("ruleId").await?;
    let patch: RulePatch = call.body().await?;

    with_account(&mut call, |mail| async move {
      let changed =
        update_rule(mail.standing.store.pool(), &mail.account.address, rule_id(&id)?, &patch).await?;

      Ok(answer(StatusCode::OK, &rule(changed)))
    })
    .await
  }
}

crate::route!(ChangeRule);

/// `DELETE /accounts/{accountName}/rules/{ruleId}`.
pub struct DeleteRule;

impl Route for DeleteRule {
  const METHOD: Method = Method::DELETE;
  const PATH: &'static str = "/accounts/{accountName}/rules/{ruleId}";
  const OPERATION: Operation = Operation {
    id: "deleteAccountsRulesByAccountNameRuleId",
    summary: "Remove a rule.",
    tag: "accounts",
    keys: &[ACCOUNT, RULE],
    query: None,
    body: None,
    status: 204,
    answer: Answer::Nothing,
    codes: &["not-found"],
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let id = call.key("ruleId").await?;

    with_account(&mut call, |mail| async move {
      delete_rule(mail.standing.store.pool(), &mail.account.address, rule_id(&id)?).await?;

      Ok(nothing())
    })
    .await
  }
}

crate::route!(DeleteRule);

/// `POST /accounts/{accountName}/rule-runs`: the rules run over a folder now.
pub struct CreateRuleRun;

impl Route for CreateRuleRun {
  const METHOD: Method = Method::POST;
  const PATH: &'static str = "/accounts/{accountName}/rule-runs";
  const OPERATION: Operation = Operation {
    id: "createAccountsRuleRunsByAccountName",
    summary: "Run the rules over a folder now.",
    tag: "accounts",
    keys: &[ACCOUNT],
    query: None,
    body: Some(schema::<NewRuleRun>),
    status: 201,
    answer: Answer::Json(schema::<RuleRun>),
    codes: &["not-found", "unknown-folder"],
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let new: NewRuleRun = call.body().await?;

    with_account(&mut call, |mail| async move {
      if !folders_of(&mail.session).await?.iter().any(|folder| folder.path == new.folder_name) {
        return Err(Asked::rule("unknown-folder", "the mailbox holds no folder of that name").into());
      }

      let pool = mail.standing.store.pool();
      let run = run_rules_over_folder(pool, &mail.session, &mail.account.address, &new.folder_name).await?;
      let made = RuleRun {
        folder_name: new.folder_name,
        moved_count: run.moved,
        labelled_count: run.labelled,
      };

      Ok(answer(StatusCode::CREATED, &made))
    })
    .await
  }
}

crate::route!(CreateRuleRun);

/// `PUT /accounts/{accountName}/spam-senders/{address}`.
pub struct PutSpamSender;

impl Route for PutSpamSender {
  const METHOD: Method = Method::PUT;
  const PATH: &'static str = "/accounts/{accountName}/spam-senders/{address}";
  const OPERATION: Operation = Operation {
    id: "putAccountsSpamSendersByAccountNameAddress",
    summary: "Mark a sender as spam: its mail always goes to spam.",
    tag: "accounts",
    keys: &[ACCOUNT, ADDRESS],
    query: None,
    body: None,
    status: 200,
    answer: Answer::Json(schema::<SpamSender>),
    codes: &["not-found"],
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let address = call.key("address").await?;

    with_account(&mut call, |mail| async move {
      let (address, added) =
        add_spam_sender(mail.standing.store.pool(), &mail.account.address, &address).await?;
      let status = if added { StatusCode::CREATED } else { StatusCode::OK };

      Ok(answer(status, &SpamSender { address }))
    })
    .await
  }
}

crate::route!(PutSpamSender);

/// `DELETE /accounts/{accountName}/spam-senders/{address}`.
pub struct DeleteSpamSender;

impl Route for DeleteSpamSender {
  const METHOD: Method = Method::DELETE;
  const PATH: &'static str = "/accounts/{accountName}/spam-senders/{address}";
  const OPERATION: Operation = Operation {
    id: "deleteAccountsSpamSendersByAccountNameAddress",
    summary: "Unmark a spam sender.",
    tag: "accounts",
    keys: &[ACCOUNT, ADDRESS],
    query: None,
    body: None,
    status: 204,
    answer: Answer::Nothing,
    codes: &["not-found"],
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let address = call.key("address").await?;

    with_account(&mut call, |mail| async move {
      remove_spam_sender(mail.standing.store.pool(), &mail.account.address, &address).await?;

      Ok(nothing())
    })
    .await
  }
}

crate::route!(DeleteSpamSender);

/// `GET /accounts/{accountName}/settings`.
pub struct ReadSettings;

impl Route for ReadSettings {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/accounts/{accountName}/settings";
  const OPERATION: Operation = Operation {
    id: "listAccountsSettingsByAccountName",
    summary: "An account's settings.",
    tag: "accounts",
    keys: &[ACCOUNT],
    query: None,
    body: None,
    status: 200,
    answer: Answer::Json(schema::<Settings>),
    codes: &["not-found"],
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    with_account(&mut call, |mail| async move {
      let pool = mail.standing.store.pool();
      let address = &mail.account.address;
      let kept = read_settings(pool, address).await?;

      Ok(answer(StatusCode::OK, &settings(kept, list_spam_senders(pool, address).await?)))
    })
    .await
  }
}

crate::route!(ReadSettings);

/// `PATCH /accounts/{accountName}/settings`.
pub struct ChangeSettings;

impl Route for ChangeSettings {
  const METHOD: Method = Method::PATCH;
  const PATH: &'static str = "/accounts/{accountName}/settings";
  const OPERATION: Operation = Operation {
    id: "changeAccountsSettingsByAccountName",
    summary: "Change an account's settings.",
    tag: "accounts",
    keys: &[ACCOUNT],
    query: None,
    body: Some(schema::<SettingsPatch>),
    status: 200,
    answer: Answer::Json(schema::<Settings>),
    codes: &["not-found"],
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let patch: SettingsPatch = call.body().await?;

    with_account(&mut call, |mail| async move {
      let pool = mail.standing.store.pool();
      let address = &mail.account.address;
      let mut kept = read_settings(pool, address).await?;

      if let Some(signature) = patch.signature {
        if signature.chars().count() > SIGNATURE_LIMIT {
          return Err(Asked::bad_request("the signature is past 200,000 characters").into());
        }

        kept.signature = signature;
      }

      if let Some(span) = patch.undo_send_ms {
        if !(0..=MOST_UNDO_SEND_MS).contains(&span) {
          return Err(Asked::bad_request("the undo span is not 0 to 30,000 milliseconds").into());
        }

        kept.undo_send_ms = span;
      }

      if let Some(asked) = patch.read_receipts {
        kept.receipt_default = asked;
      }

      if let Some(shown) = patch.show_images {
        kept.show_images = shown;
      }

      write_settings(pool, address, &kept).await?;

      Ok(answer(StatusCode::OK, &settings(kept, list_spam_senders(pool, address).await?)))
    })
    .await
  }
}

crate::route!(ChangeSettings);
