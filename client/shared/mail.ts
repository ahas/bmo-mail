// The mail service's members as the screen reads them (docs/api.md), each the type generated from the
// service's own OpenAPI document into `openapi.ts`, and the screen's own reading beside them: a conversation
// made of a page's messages, and what a message's flags and parts say.

import type { components } from "./openapi";

type Schemas = components["schemas"];

export type MailAccount = Schemas["Account"];
export type MailAccountKind = Schemas["AccountKind"];
export type MailKindField = Schemas["KindField"];
export type NewMailAccount = Schemas["NewAccount"];
export type MailAccountPatch = Schemas["AccountPatch"];
export type MailAuthorization = Schemas["Authorization"];
export type MailSummary = Schemas["Summary"];
export type MailFolder = Schemas["Folder"];
export type FolderRole = Schemas["FolderRole"];
export type MailMailbox = Schemas["Mailbox"];
export type MailMessage = Schemas["Message"];
export type MailMessagePage = Schemas["MessagePage"];
export type MailPart = Schemas["Part"];
export type MailThread = Schemas["Thread"];
export type MailLabel = Schemas["Label"];
export type MailAddress = Schemas["Address"];
export type Draft = Schemas["Draft"];
export type DraftPatch = Schemas["DraftPatch"];
export type DraftAttachment = Schemas["DraftAttachment"];
export type DraftKind = Schemas["DraftKind"];
export type MailSend = Schemas["Send"];
export type MailRule = Schemas["Rule"];
export type NewRule = Schemas["NewRule"];
export type RulePatch = Schemas["RulePatch"];
export type RuleMatching = Schemas["RuleMatching"];
export type RuleCondition = Schemas["RuleCondition"];
export type RuleAction = Schemas["RuleAction"];
export type RuleField = Schemas["RuleField"];
export type MailSettings = Schemas["Settings"];
export type MailSettingsPatch = Schemas["SettingsPatch"];
export type MailEvent = Schemas["Event"];

// The flags a message's state is read from and written as.
export const SEEN = "\\Seen";
export const FLAGGED = "\\Flagged";
export const JUNK = "$Junk";
export const NOT_JUNK = "$NotJunk";
export const RECEIPT_ANSWERED = "$MDNSent";

export function isRead(message: Pick<MailMessage, "flags">): boolean {
  return message.flags.includes(SEEN);
}

export function isStarred(message: Pick<MailMessage, "flags">): boolean {
  return message.flags.includes(FLAGGED);
}

// Whether a read receipt the message asked for has been answered, sent or declined.
export function receiptAnswered(message: Pick<MailMessage, "flags">): boolean {
  return message.flags.includes(RECEIPT_ANSWERED);
}

// The files a message carries, and the pictures its HTML body shows by content id.
export function attachmentsOf(message: Pick<MailMessage, "parts">): MailPart[] {
  return message.parts.filter((part) => part.kind === "attachment");
}

// The decoded size of a part the server reports by its encoded size.
export function partSize(part: { size: number; encoding?: string | null }): number {
  return part.encoding === "base64" ? Math.floor((part.size * 3) / 4) : part.size;
}

// A conversation as a row of the list shows it: the messages of one page that share a thread, the newest
// first, and what the row says of them, read from the newest.
export interface MailConversation {
  // The thread's id, which the screen's address names the conversation by.
  key: string;
  subject: string;
  at: number;
  // The senders of the conversation's messages on this page, newest first, each once.
  from: MailMailbox[];
  count: number;
  unread: boolean;
  flagged: boolean;
  labelIds: string[];
  hasAttachments: boolean;
  snippet: string;
  messages: MailMessage[];
}

// A page's messages gathered into the conversations they make up, in the page's order: a conversation takes
// the place of its newest message on the page.
export function conversationsOf(messages: readonly MailMessage[]): MailConversation[] {
  const groups = new Map<string, MailMessage[]>();

  for (const message of messages) {
    const group = groups.get(message.threadId);

    if (group === undefined) groups.set(message.threadId, [message]);
    else group.push(message);
  }

  return [...groups.entries()].map(([key, group]) => {
    const byNewest = [...group].sort((a, b) => b.at - a.at);
    const newest = byNewest[0]!;
    const oldest = byNewest[byNewest.length - 1]!;
    const from: MailMailbox[] = [];

    for (const message of byNewest) {
      if (message.from != null && !from.some((one) => one.address === message.from!.address)) from.push(message.from);
    }

    return {
      key,
      subject: oldest.subject === "" ? newest.subject : oldest.subject,
      at: newest.at,
      from,
      count: group.length,
      unread: group.some((message) => !isRead(message)),
      flagged: group.some(isStarred),
      labelIds: [...new Set(group.flatMap((message) => message.labelIds))],
      hasAttachments: group.some((message) => attachmentsOf(message).length > 0),
      snippet: newest.snippet,
      messages: byNewest,
    };
  });
}

// The ready rules the settings offer: the verdicts a spam filter on the mail server writes into a header,
// so a filter installed there later reaches the screen with no change here.
export const SPAM_HEADER_PRESETS: NewRule[] = [
  ["X-Spam-Flag", "YES"],
  ["X-Bogosity", "Spam"],
].map(([header, contains]) => ({
  match: { kind: "any", conditions: [{ field: "header", header: header!, contains: contains! }] },
  actions: [{ kind: "spam" }],
  on: true,
}));
