//! The members the routes answer and take (`docs/api.md`), in the common wire format: camelCase keys, times in
//! epoch milliseconds, a list as a page and a failure as its code and sentence. Each carries its JSON Schema,
//! which the OpenAPI document, the client and the MCP server's tools are written from.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::mail::{DraftKind, FolderRole, RuleAction, RuleCondition, RuleMatch};

/// A failure, in the common error shape.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WireError {
  /// The kebab-case code a caller matches on.
  pub code: String,
  /// The English sentence, for a log.
  pub error: String,
}

/// A page of a list.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[schemars(rename = "{T}Page")]
pub struct Page<T> {
  pub items: Vec<T>,
  /// Whether anything stands past the last item.
  pub more: bool,
}

impl<T> Page<T> {
  /// A small list paged by position: the items past the position `after` names, at most `limit` of them.
  pub fn by_position(items: Vec<T>, position: impl Fn(&T) -> i64, query: &ListQuery) -> Self {
    let after = query.after.unwrap_or(0);
    let mut past = items.into_iter().filter(|item| position(item) > after).peekable();
    let limit = query.limit.map_or(usize::MAX, |limit| limit as usize);
    let items: Vec<T> = past.by_ref().take(limit).collect();
    let more = past.peek().is_some();

    Self { items, more }
  }
}

/// Readiness and version.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Health {
  pub ready: bool,
  /// Why the service is not ready; null while it is.
  pub reason: Option<String>,
  pub version: String,
}

/// One event of the stream.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Event {
  /// The event's number, which only grows.
  pub id: i64,
  /// The resource and its change: `message.created`.
  pub kind: String,
  pub at: i64,
  /// The member as a GET answers it, or its key alone for a deletion.
  pub data: serde_json::Value,
}

/// What a reader of the event stream asks.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EventQuery {
  /// The kinds to read, comma-separated; every kind where none is given.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub kinds: Option<String>,
  /// The number of the last event read; the events after it.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub after: Option<i64>,
  /// 1 to 1,000; 100 where none is given.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub limit: Option<u32>,
  /// One account's events.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub account_name: Option<String>,
}

/// A small list's paging: by the position of the last item read.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListQuery {
  /// The position of the last item read; the items after it.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub after: Option<i64>,
  /// The most items the page holds; the whole list where none is given.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub limit: Option<u32>,
}

/// A kind's or an account's fields, by name, each a string.
pub type Fields = BTreeMap<String, String>;

/// How an account of a kind signs in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SignIn {
  /// From the fields a person writes, a secret among them.
  Password,
  /// Through the provider's own page, which hands back the tokens kept as the account's secrets.
  Authorization,
}

/// What a field of a kind holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FieldKind {
  Text,
  /// A port, 1 to 65535.
  Port,
  /// Kept encrypted and never answered.
  Secret,
}

/// One field a person writes to add an account of a kind.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct KindField {
  /// The key it stands under in an account's `settings`, or its `secrets` where it is secret.
  pub name: String,
  pub kind: FieldKind,
  pub required: bool,
  /// The value a form starts from; null where it starts empty.
  pub default: Option<String>,
}

/// A kind of account: the adapter that reaches its mailbox, as that adapter declares it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AccountKind {
  /// The adapter's name, which an account names its kind by.
  pub name: String,
  pub sign_in: SignIn,
  /// What a person writes to add an account of it, in the order a form shows them.
  pub fields: Vec<KindField>,
}

/// An account of the owner, as much of it as is answered: never its secrets.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Account {
  /// Unique among the owner's accounts: the key the routes name it by.
  pub name: String,
  /// The adapter's name.
  pub kind: String,
  pub address: String,
  /// Its place in its owner's list, from 1.
  pub position: i64,
  /// Its fields that are not secret.
  pub settings: Fields,
  /// The unread messages of its inbox.
  pub unread_count: u32,
  /// Whether its server answered; where it did not, the count is 0.
  pub reachable: bool,
}

/// The answer of a provider's page, which finishes adding an account by authorization.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AuthorizationAnswer {
  /// The authorization's id.
  pub id: String,
  /// The query the provider sent the browser back with.
  pub answer: Fields,
}

/// An account to add: by password from its fields, or by an authorization's answer.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewAccount {
  pub kind: String,
  /// The address's local part where none is given.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub name: Option<String>,
  /// Required for a kind that signs in by password.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub address: Option<String>,
  /// Its fields that are not secret, by name.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub settings: Option<Fields>,
  /// Its secret fields, by name.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub secrets: Option<Fields>,
  /// For a kind that signs in by authorization: the authorization and what the provider answered.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub authorization: Option<AuthorizationAnswer>,
}

/// An account's change.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccountPatch {
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub name: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub position: Option<i64>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub address: Option<String>,
  /// Its fields that are not secret, replaced whole.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub settings: Option<Fields>,
  /// The secret fields to replace; the others are kept.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub secrets: Option<Fields>,
}

/// An authorization to start.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewAuthorization {
  pub kind: String,
  /// Where the provider sends the browser back to.
  pub return_to: String,
}

/// An authorization started: where to send the browser.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Authorization {
  pub id: String,
  pub kind: String,
  /// The provider's page.
  pub url: String,
  pub expires_at: i64,
}

/// Unread counts across the accounts.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
  pub accounts: Vec<Account>,
  /// The unread messages of every reachable inbox together.
  pub unread_count: u32,
}

/// A folder of an account's mailbox.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
  pub account_name: String,
  /// The folder's full name on the server: its key.
  pub name: String,
  /// The last step of the name.
  pub short_name: String,
  pub role: Option<FolderRole>,
  pub position: i64,
  pub unread_count: u32,
  pub total_count: u32,
}

/// A folder's change: every message marked read, or every one unread.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FolderPatch {
  pub read: bool,
}

/// A name and an address.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Mailbox {
  /// The display name; null where none is given.
  #[serde(default)]
  pub name: Option<String>,
  pub address: String,
}

/// What a part of a message is to the message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PartKind {
  /// The plain-text body.
  Text,
  /// The HTML body.
  Html,
  Attachment,
  /// A picture an HTML body shows by its content id.
  Inline,
}

/// A part of a message.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Part {
  /// Its number in the message: `1.2`.
  pub part_id: String,
  pub kind: PartKind,
  pub media_type: String,
  pub charset: Option<String>,
  pub encoding: Option<String>,
  pub file_name: Option<String>,
  /// The encoded size the server reports.
  pub size: u64,
  /// The content id, without angle brackets.
  pub content_id: Option<String>,
}

/// A message.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Message {
  /// Opaque: names the message's folder and its UID there.
  pub id: String,
  pub account_name: String,
  /// Opaque: the same for every message of the conversation.
  pub thread_id: String,
  pub folder_name: String,
  /// Its `Message-ID`, without angle brackets.
  pub internet_message_id: Option<String>,
  pub in_reply_to: Option<String>,
  pub references: Vec<String>,
  pub subject: String,
  pub from: Option<Mailbox>,
  pub to: Vec<Mailbox>,
  pub cc: Vec<Mailbox>,
  pub bcc: Vec<Mailbox>,
  pub reply_to: Vec<Mailbox>,
  /// When it reached its folder, else its date.
  pub at: i64,
  /// Every flag it carries but the labels' keywords.
  pub flags: Vec<String>,
  pub label_ids: Vec<String>,
  /// Where a read receipt is asked to go.
  pub receipt_to: Option<String>,
  pub snippet: String,
  /// The decoded plain-text body; null in a page, and where it has none.
  pub text: Option<String>,
  /// The decoded HTML body; null in a page, and where it has none.
  pub html: Option<String>,
  pub parts: Vec<Part>,
}

/// What a page of messages reads and where it starts.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MessageQuery {
  /// The folder to read; the inbox where neither it, `q` nor `labelId` is given.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub folder_name: Option<String>,
  /// The search box's words.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub q: Option<String>,
  /// One label's messages.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub label_id: Option<String>,
  /// A message's `at`: the messages older than it.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub before: Option<i64>,
  /// A message's `at`: the messages newer than it, the oldest of them.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub after: Option<i64>,
  /// 1 to 100; 50 where none is given.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub limit: Option<u32>,
}

/// A message's change: the whole of the flags and labels it should carry, and the folder it should be in.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MessagePatch {
  /// The flags it should hold among those a request may set.
  pub flags: Vec<String>,
  /// Every label it should carry.
  pub label_ids: Vec<String>,
  /// The folder to move it to.
  #[serde(default)]
  pub folder_name: Option<String>,
}

/// What a thread is read from beside the inbox and the sent folders.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ThreadQuery {
  /// The folder the thread was opened from.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub folder_name: Option<String>,
}

/// A conversation and its messages.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
  pub id: String,
  pub account_name: String,
  pub subject: String,
  /// Oldest first, each with its bodies.
  pub messages: Vec<Message>,
}

/// A label of an account.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Label {
  pub id: String,
  /// 1 to 60 characters, one per name in an account.
  pub name: String,
  /// `#` and six hex digits, in lower case.
  pub colour: String,
  pub position: i64,
}

/// A label to make.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NewLabel {
  pub name: String,
  pub colour: String,
}

/// A label's change.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LabelPatch {
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub name: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub colour: Option<String>,
  /// Its new place among the labels.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub position: Option<i64>,
}

/// An address written to or heard from.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Address {
  pub address: String,
  pub name: Option<String>,
  pub used_count: i64,
  pub last_used_at: i64,
  pub position: i64,
}

/// What the address completion matches.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddressQuery {
  /// Matched against the address and the name.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub q: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub after: Option<i64>,
  /// The most items the page holds; 10 where none is given.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub limit: Option<u32>,
}

/// An attachment of a draft: a part of the saved draft.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DraftAttachment {
  pub part_id: String,
  #[serde(default)]
  pub name: String,
  #[serde(default)]
  pub media_type: String,
  #[serde(default)]
  pub size: u64,
}

/// A message being written, saved in the drafts folder.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
  /// The `Message-ID` it is saved under, without angle brackets.
  pub id: String,
  pub account_name: String,
  pub kind: DraftKind,
  pub from: Mailbox,
  pub to: Vec<Mailbox>,
  pub cc: Vec<Mailbox>,
  pub bcc: Vec<Mailbox>,
  pub subject: String,
  /// The HTML it is written in.
  pub body: String,
  pub attachments: Vec<DraftAttachment>,
  pub receipt_requested: bool,
  pub in_reply_to: Option<String>,
  pub references: Vec<String>,
  /// The message ID of the message it answers or carries.
  pub original_id: Option<String>,
  pub changed_at: i64,
}

/// A draft's start.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewDraft {
  /// The message it answers, by its id.
  #[serde(default)]
  pub reply_to: Option<String>,
  /// Whether it answers every recipient of `replyTo`.
  #[serde(default)]
  pub reply_all: bool,
  /// The message it carries, by its id.
  #[serde(default)]
  pub forward: Option<String>,
}

/// A draft's change: each field given replaces the draft's own.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DraftPatch {
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub to: Option<Vec<Mailbox>>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub cc: Option<Vec<Mailbox>>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub bcc: Option<Vec<Mailbox>>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub subject: Option<String>,
  /// HTML.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub body: Option<String>,
  /// The attachments to keep, by `partId`.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub attachments: Option<Vec<DraftAttachment>>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub receipt_requested: Option<bool>,
}

/// A file to attach to a draft.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewAttachment {
  pub name: String,
  pub media_type: String,
  /// The file's bytes, in base64.
  pub content: String,
}

/// Where a send stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SendState {
  Waiting,
  Sent,
  Cancelled,
  Failed,
}

/// A draft being sent.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Send {
  pub id: String,
  pub account_name: String,
  pub draft_id: String,
  pub state: SendState,
  pub send_at: i64,
  /// Why it failed, in English; null unless it did.
  pub failure: Option<String>,
}

/// A send to make.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewSend {
  pub draft_id: String,
  /// When it goes; after the account's undo span where null.
  #[serde(default)]
  pub send_at: Option<i64>,
}

/// A read receipt to send.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewReceipt {
  pub message_id: String,
}

/// A read receipt sent.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Receipt {
  pub message_id: String,
  pub sent: bool,
}

/// Whether a rule needs all of its conditions or any one, and the conditions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuleMatching {
  pub kind: RuleMatch,
  pub conditions: Vec<RuleCondition>,
}

/// A filter rule.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Rule {
  pub id: i64,
  pub position: i64,
  #[serde(rename = "match")]
  pub matching: RuleMatching,
  pub actions: Vec<RuleAction>,
  pub on: bool,
}

/// A rule to make.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NewRule {
  #[serde(rename = "match")]
  pub matching: RuleMatching,
  pub actions: Vec<RuleAction>,
  #[serde(default = "on")]
  pub on: bool,
}

fn on() -> bool {
  true
}

/// A rule's change.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RulePatch {
  /// Its new place among the rules.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub position: Option<i64>,
  #[serde(default, rename = "match", skip_serializing_if = "Option::is_none")]
  pub matching: Option<RuleMatching>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub actions: Option<Vec<RuleAction>>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub on: Option<bool>,
}

/// A run of the rules over a folder to make.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewRuleRun {
  pub folder_name: String,
}

/// What a run of the rules did.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuleRun {
  pub folder_name: String,
  pub moved_count: u32,
  pub labelled_count: u32,
}

/// A sender always sent to spam.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SpamSender {
  pub address: String,
}

/// An account's settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
  /// Markup, at most 200,000 characters.
  pub signature: String,
  /// How long a send waits before it goes: 0 to 30,000.
  pub undo_send_ms: i64,
  /// Whether a new draft asks for a read receipt.
  pub read_receipts: bool,
  /// Whether a message's outside pictures are shown as it opens.
  pub show_images: bool,
  /// Every always-spam sender; changed through its own route alone.
  pub spam_senders: Vec<String>,
}

/// A change of an account's settings.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsPatch {
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub signature: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub undo_send_ms: Option<i64>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub read_receipts: Option<bool>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub show_images: Option<bool>,
}
