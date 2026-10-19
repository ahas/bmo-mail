//! The shapes the routes answer and take (`docs/api.md`), in the field names the screen reads: a list, a
//! message, a draft and a rule mean the same thing on either side.

use serde::{Deserialize, Serialize};

/// A message on the server: its folder and its UID there.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MailRef {
  pub folder: String,
  pub uid: u32,
}

/// A name and an address, the name empty where none is given.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailAddress {
  pub name: String,
  pub address: String,
}

/// A message as a list shows it.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailListMessage {
  pub folder: String,
  pub uid: u32,
  /// The conversation it belongs to.
  pub key: String,
  pub message_id: Option<String>,
  pub subject: String,
  pub from: Vec<MailAddress>,
  pub to: Vec<MailAddress>,
  pub date: i64,
  pub seen: bool,
  pub flagged: bool,
  pub answered: bool,
  pub draft: bool,
  /// Whether a read receipt this message asked for has been answered, sent or declined.
  pub receipt_answered: bool,
  pub labels: Vec<String>,
  pub has_attachments: bool,
  pub snippet: String,
}

/// A conversation as a page of a list shows it.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailConversation {
  pub key: String,
  pub subject: String,
  pub date: i64,
  /// The senders of the conversation's messages on this page, newest first, each once.
  pub from: Vec<MailAddress>,
  pub count: usize,
  pub unread: bool,
  pub flagged: bool,
  pub labels: Vec<String>,
  pub has_attachments: bool,
  pub snippet: String,
  pub messages: Vec<MailRef>,
}

/// One page of a list.
#[derive(Clone, Debug, Serialize)]
pub struct MailPage {
  /// How many messages the view holds.
  pub total: usize,
  /// Where this page starts among them.
  pub offset: usize,
  pub conversations: Vec<MailConversation>,
}

/// A message with its bodies.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailMessage {
  #[serde(flatten)]
  pub listed: MailListMessage,
  pub cc: Vec<MailAddress>,
  pub bcc: Vec<MailAddress>,
  pub reply_to: Vec<MailAddress>,
  pub in_reply_to: Option<String>,
  pub references: Vec<String>,
  /// Where a read receipt is asked to go, when the message asks for one.
  pub receipt_to: Option<String>,
  pub text: Option<String>,
  pub html: Option<String>,
  pub attachments: Vec<MailPart>,
  pub inline: Vec<MailPart>,
}

/// A part of a message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MailPart {
  /// The part's number in the message, as a fetch asks for it.
  pub part: String,
  #[serde(rename = "type")]
  pub kind: String,
  pub charset: Option<String>,
  pub encoding: Option<String>,
  pub filename: Option<String>,
  /// The encoded size the server reports; a base64 part decodes to about three quarters of it.
  pub size: u64,
  /// The content id without its angle brackets, for a part an HTML body shows inline.
  pub cid: Option<String>,
}

impl MailPart {
  /// The decoded size of the part, from the encoded size the server reports.
  pub fn decoded_size(&self) -> u64 {
    if self.encoding.as_deref() == Some("base64") { self.size * 3 / 4 } else { self.size }
  }
}

/// A message's parts, sorted: the text body, the HTML body, the attachments and the inline pictures.
#[derive(Clone, Debug, Default)]
pub struct MailParts {
  pub text: Option<MailPart>,
  pub html: Option<MailPart>,
  pub attachments: Vec<MailPart>,
  pub inline: Vec<MailPart>,
}

/// The part a special-use folder plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FolderRole {
  Inbox,
  Sent,
  Drafts,
  Junk,
  Trash,
}

impl FolderRole {
  /// Every role, in the order the mail nav lists them.
  pub const ALL: [FolderRole; 5] = [Self::Inbox, Self::Sent, Self::Drafts, Self::Junk, Self::Trash];

  /// The role's name, as a view and an address name it.
  pub fn name(self) -> &'static str {
    match self {
      Self::Inbox => "inbox",
      Self::Sent => "sent",
      Self::Drafts => "drafts",
      Self::Junk => "junk",
      Self::Trash => "trash",
    }
  }

  /// The role a name names.
  pub fn named(name: &str) -> Option<Self> {
    Self::ALL.into_iter().find(|role| role.name() == name)
  }
}

/// A folder of the mailbox.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MailFolder {
  pub path: String,
  /// The last part of the path, as the screen shows a folder that has no role.
  pub name: String,
  pub role: Option<FolderRole>,
}

/// A folder with its counts, for the mail nav.
#[derive(Clone, Debug, Serialize)]
pub struct MailFolderState {
  #[serde(flatten)]
  pub folder: MailFolder,
  pub unseen: u32,
  pub total: u32,
}

/// A label of an account.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MailLabel {
  pub id: String,
  pub name: String,
  pub color: String,
  pub position: i64,
}

/// An attachment of a message being written: a file uploaded into the staging folder, or a part of a
/// message already on the server — the original of a forward, or the saved draft being reopened.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum AttachmentRef {
  Upload {
    id: String,
    filename: String,
    #[serde(rename = "type")]
    kind: String,
    size: u64,
  },
  Part {
    folder: String,
    uid: u32,
    part: String,
    filename: String,
    #[serde(rename = "type")]
    kind: String,
    size: u64,
  },
}

/// What a draft is written as.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DraftKind {
  New,
  Reply,
  Forward,
}

/// A message being written.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
  /// The account the message goes out from, by its address.
  pub from: String,
  pub to: Vec<MailAddress>,
  pub cc: Vec<MailAddress>,
  pub bcc: Vec<MailAddress>,
  pub subject: String,
  pub html: String,
  pub attachments: Vec<AttachmentRef>,
  /// Whether a read receipt is asked for.
  pub receipt: bool,
  pub kind: DraftKind,
  /// The message a reply answers or a forward carries, by its message ID.
  pub original: Option<String>,
  pub in_reply_to: Option<String>,
  pub references: Vec<String>,
  /// The saved draft this one replaces, when it has been saved before.
  pub saved: Option<MailRef>,
}

/// What a rule's condition reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleField {
  From,
  To,
  Subject,
  Header,
}

/// One condition of a rule.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleCondition {
  pub field: RuleField,
  /// The header's name, for a header condition.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub header: Option<String>,
  /// The text the field must hold, compared without regard to case.
  pub contains: String,
}

/// One action of a rule.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum RuleAction {
  Label { label: String },
  Star,
  Read,
  Move { folder: String },
  Spam,
  Trash,
}

/// Whether a rule needs all of its conditions or any one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleMatch {
  All,
  Any,
}

/// A rule as the screen writes it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleDraft {
  pub enabled: bool,
  #[serde(rename = "match")]
  pub matching: RuleMatch,
  pub conditions: Vec<RuleCondition>,
  pub actions: Vec<RuleAction>,
}

/// A rule as it is kept.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MailRule {
  pub id: i64,
  pub position: i64,
  #[serde(flatten)]
  pub rule: RuleDraft,
}

/// An account's settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailSettings {
  /// The signature is markup, written in the same editor a message's body is.
  pub signature: String,
  pub receipt_default: bool,
  /// Whether a message's pictures from outside are shown as the message opens, or held behind the bar
  /// over the body until the reader asks for them.
  pub show_images: bool,
}

/// A file uploaded into the staging folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct StagedFile {
  pub id: String,
  pub filename: String,
  #[serde(rename = "type")]
  pub kind: String,
  pub size: u64,
}

/// Where a send stands.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum SendState {
  Holding,
  Sent,
  Cancelled,
  Failed { reason: String },
}

/// What of an account may reach the browser.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PublicMailAccount {
  pub address: String,
  pub key: String,
}

/// What a list shows: a folder by its role or its path, the starred view, one label's view, or a search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MailView {
  Role(FolderRole),
  Folder(String),
  Starred,
  Label(String),
  Search(String),
}

/// The column a list is ordered by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MailSortBy {
  Date,
  From,
  Subject,
}

/// How a list stands: which column orders it, which way that column runs, and whether it holds only what
/// has not been read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MailSort {
  pub by: MailSortBy,
  /// Whether the least stands first: the oldest date, the first name, the first subject.
  pub ascending: bool,
  /// Whether the list holds back everything already read.
  pub unread: bool,
}

impl MailSort {
  /// A list asked for with nothing said: the newest first, nothing held back.
  pub const NEWEST: Self = Self {
    by: MailSortBy::Date,
    ascending: false,
    unread: false,
  };
}

/// One message a mail summary names: enough for a notice and for the address that opens it.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailSummaryMessage {
  pub uid: u32,
  /// The conversation it belongs to, as the screen's addresses name it.
  pub key: String,
  pub from: Vec<MailAddress>,
  pub subject: String,
  pub date: i64,
  /// The service moved it into the inbox, so it is not new mail.
  pub moved_in: bool,
}

/// One account's inbox as the tabs watch it: how much is unread, the newest message, and the unread
/// messages at the top of the inbox, newest first. An account whose server did not answer says so rather
/// than reading as empty.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailAccountSummary {
  pub address: String,
  pub account: String,
  pub unread: u32,
  pub newest: Option<MailSummaryMessage>,
  pub recent_unread: Vec<MailSummaryMessage>,
  pub failed: bool,
}
