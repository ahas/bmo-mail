//! The service's own reading of mail: a message as the server holds it, a draft being written, a rule as the
//! store keeps it. The routes answer the members `wire` states, read from these (`members`).

use schemars::JsonSchema;
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
  /// Every flag it carries but the labels' keywords.
  pub flags: Vec<String>,
  pub labels: Vec<String>,
  pub has_attachments: bool,
  pub snippet: String,
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
  /// The decoded bodies, where they were read.
  pub text: Option<String>,
  pub html: Option<String>,
  pub attachments: Vec<MailPart>,
  pub inline: Vec<MailPart>,
  /// The parts the bodies are read from.
  #[serde(skip)]
  pub text_part: Option<MailPart>,
  #[serde(skip)]
  pub html_part: Option<MailPart>,
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
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
#[derive(Clone, Debug)]
pub struct MailFolderState {
  pub folder: MailFolder,
  pub unseen: u32,
  pub total: u32,
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
  /// The `Message-ID` the draft is saved under, in angle brackets, which it keeps across every save; a
  /// draft read back from a client that wrote none is given one at its next save.
  pub message_id: Option<String>,
  /// The saved draft this one replaces, when it has been saved before.
  pub saved: Option<MailRef>,
  /// When it was last saved, or started where it never was.
  pub changed_at: i64,
}

/// What a rule's condition reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum RuleField {
  From,
  To,
  Subject,
  Header,
}

/// One condition of a rule.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RuleCondition {
  pub field: RuleField,
  /// The header's name, for a header condition; null for any other.
  #[serde(default)]
  pub header: Option<String>,
  /// The text the field must hold, compared without regard to case.
  pub contains: String,
}

/// One action of a rule. The store holds it in this shape; a rule kept before its fields took their
/// names of today is read under the names it was kept with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum RuleAction {
  Label {
    #[serde(rename = "labelId", alias = "label")]
    label_id: String,
  },
  Star,
  Read,
  Move {
    #[serde(rename = "folderName", alias = "folder")]
    folder_name: String,
  },
  Spam,
  Trash,
}

/// Whether a rule needs all of its conditions or any one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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

/// An account's settings as the store keeps them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MailSettings {
  /// The signature is markup, written in the same editor a message's body is.
  pub signature: String,
  pub receipt_default: bool,
  /// Whether a message's pictures from outside are shown as the message opens, or held behind the bar
  /// over the body until the reader asks for them.
  pub show_images: bool,
  /// How long a send waits before it goes, while it can still be taken back.
  pub undo_send_ms: i64,
}

/// A file posted into the staging folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct StagedFile {
  pub id: String,
  pub filename: String,
  #[serde(rename = "type")]
  pub kind: String,
  pub size: u64,
}

/// What a list shows: a folder by its role or its path, one label's view, or a search, each held to one
/// folder where it names one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MailView {
  Role(FolderRole),
  Folder(String),
  Label {
    id: String,
    folder: Option<String>,
  },
  Search {
    query: String,
    folder: Option<String>,
  },
}
