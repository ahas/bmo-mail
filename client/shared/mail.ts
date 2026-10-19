// The mail screen's shapes as they travel between the mail service and the browser (docs/api.md,
// 「What a message is」). The service answers them and the screen reads them from here, so a list, a
// message or a draft means the same thing on either side.

export interface MailRef {
  folder: string;
  uid: number;
}

export interface MailAddress {
  name: string;
  address: string;
}

export interface MailListMessage extends MailRef {
  key: string;
  messageId: string | null;
  subject: string;
  from: MailAddress[];
  to: MailAddress[];
  date: number;
  seen: boolean;
  flagged: boolean;
  answered: boolean;
  draft: boolean;
  // Whether a read receipt this message asked for has been answered, sent or declined.
  receiptAnswered: boolean;
  labels: string[];
  hasAttachments: boolean;
  snippet: string;
}

export interface MailConversation {
  key: string;
  subject: string;
  date: number;
  // The senders of the conversation's messages on this page, newest first, each once.
  from: MailAddress[];
  count: number;
  unread: boolean;
  flagged: boolean;
  labels: string[];
  hasAttachments: boolean;
  snippet: string;
  messages: MailRef[];
}

export interface MailPage {
  // How many messages the view holds, and where this page starts among them.
  total: number;
  offset: number;
  conversations: MailConversation[];
}

export interface MailMessage extends MailListMessage {
  cc: MailAddress[];
  bcc: MailAddress[];
  replyTo: MailAddress[];
  inReplyTo: string | null;
  references: string[];
  // Where a read receipt is asked to go, when the message asks for one.
  receiptTo: string | null;
  text: string | null;
  html: string | null;
  attachments: MailPart[];
  inline: MailPart[];
}

export interface MailFolderState extends MailFolder {
  unseen: number;
  total: number;
}

// The decoded size of a part the server reports by its encoded size.
export function partSize(part: { size: number; encoding: string | null }): number {
  return part.encoding === "base64" ? Math.floor((part.size * 3) / 4) : part.size;
}

export interface MailPart {
  // The part's number in the message, as a fetch asks for it.
  part: string;
  type: string;
  charset: string | null;
  encoding: string | null;
  filename: string | null;
  // The encoded size the server reports; a base64 part decodes to about three quarters of it.
  size: number;
  // The content id without its angle brackets, for a part an HTML body shows inline.
  cid: string | null;
}

export interface MailParts {
  text: MailPart | null;
  html: MailPart | null;
  attachments: MailPart[];
  inline: MailPart[];
}

export type FolderRole = "inbox" | "sent" | "drafts" | "junk" | "trash";

export interface MailFolder {
  path: string;
  // The last part of the path, as the screen shows a folder that has no role.
  name: string;
  role: FolderRole | null;
}

export interface MailLabel {
  id: string;
  name: string;
  color: string;
  position: number;
}

// An attachment of a message being written: a file uploaded into the staging folder, or a part of a
// message already on the server — the original of a forward, or the saved draft being reopened.
export type AttachmentRef =
  | { kind: "upload"; id: string; filename: string; type: string; size: number }
  | { kind: "part"; folder: string; uid: number; part: string; filename: string; type: string; size: number };

export type DraftKind = "new" | "reply" | "forward";

export interface Draft {
  // The account the message goes out from, by its address.
  from: string;
  to: MailAddress[];
  cc: MailAddress[];
  bcc: MailAddress[];
  subject: string;
  html: string;
  attachments: AttachmentRef[];
  // Whether a read receipt is asked for.
  receipt: boolean;
  kind: DraftKind;
  // The message a reply answers or a forward carries, by its message ID.
  original: string | null;
  inReplyTo: string | null;
  references: string[];
  // The saved draft this one replaces, when it has been saved before.
  saved: MailRef | null;
}

export type RuleField = "from" | "to" | "subject" | "header";

export interface RuleCondition {
  field: RuleField;
  // The header's name, for a header condition.
  header?: string;
  // The text the field must hold, compared without regard to case.
  contains: string;
}

export type RuleAction =
  | { kind: "label"; label: string }
  | { kind: "star" }
  | { kind: "read" }
  | { kind: "move"; folder: string }
  | { kind: "spam" }
  | { kind: "trash" };

export interface MailRule {
  id: number;
  position: number;
  enabled: boolean;
  match: "all" | "any";
  conditions: RuleCondition[];
  actions: RuleAction[];
}

export type RuleDraft = Omit<MailRule, "id" | "position">;

export interface MailSettings {
  // The signature is markup, written in the same editor a message's body is.
  signature: string;
  receiptDefault: boolean;
  // Whether a message's pictures from outside the console are shown as the message opens, or held
  // behind the bar over the body until the reader asks for them.
  showImages: boolean;
}

export interface StagedFile {
  id: string;
  filename: string;
  type: string;
  size: number;
}

export type SendState = { state: "holding" } | { state: "sent" } | { state: "cancelled" } | { state: "failed"; reason: string };

// What of an account may reach the browser.
export interface PublicMailAccount {
  address: string;
  key: string;
}

// What a list on the mail screen shows (docs/mail.md, 「The screen」): a folder by its role or its
// path, the starred view, one label's view, or a search.
export type MailView =
  | { kind: "role"; role: FolderRole }
  | { kind: "folder"; path: string }
  | { kind: "starred" }
  | { kind: "label"; id: string }
  | { kind: "search"; query: string };

// What every mail route answers where this console has no mail accounts file.
export interface NoMailAccounts {
  configured: false;
}

// A mail route's answer: its own shape, or that there are no mail accounts at all.
export type MailAnswer<T> = ({ configured: true } & T) | NoMailAccounts;

// How a list stands (docs/mail.md, 「Reading」): which column orders it, which way that column
// runs, and whether it holds only what has not been read. The columns a list can be ordered by are
// the ones its rows show — who wrote, what about, and when.
export type MailSortBy = "date" | "from" | "subject";

export interface MailSort {
  by: MailSortBy;
  // Whether the least stands first: the oldest date, the first name, the first subject.
  ascending: boolean;
  // Whether the list holds back everything already read.
  unread: boolean;
}

// A list asked for with nothing said: the newest first, nothing held back.
export const MAIL_SORT_NEWEST: MailSort = { by: "date", ascending: false, unread: false };

// Which way a column runs when it is chosen: a date opens at its newest, a name and a subject at
// their first.
export function sortFirstAscending(by: MailSortBy): boolean {
  return by !== "date";
}

// One message a mail summary names: enough for a notice and for the address that opens it.
export interface MailSummaryMessage {
  uid: number;
  // The conversation it belongs to, as the screen's addresses name it.
  key: string;
  from: { name: string; address: string }[];
  subject: string;
  date: number;
  // The service moved it into the inbox — not spam, a move to the inbox — so it is not new mail.
  movedIn: boolean;
}

// One account's inbox as the tabs watch it (docs/mail.md, 「Reaching the open tabs」): how much is
// unread, the newest message, and the unread messages at the top of the inbox, newest first. A tab
// compares one summary with the last it read to know what arrived; an account whose server did not
// answer says so rather than reading as empty.
export interface MailAccountSummary {
  address: string;
  account: string;
  unread: number;
  newest: MailSummaryMessage | null;
  recentUnread: MailSummaryMessage[];
  failed: boolean;
}

export interface MailSummary {
  // Whether the service has mail accounts at all.
  configured: boolean;
  accounts: MailAccountSummary[];
}
