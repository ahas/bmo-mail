import { countText } from "bmo-ui/app/domain/numbers";

import { identityMark } from "./mail-avatars";

import type { FolderRole, MailAccount, MailAccountKind, MailConversation, MailFolder, MailLabel, MailMailbox, MailMessage } from "../../shared/mail";
import type { Translate } from "bmo-ui/shared/wording";
import type { AvatarProps } from "@nuxt/ui";

// The mail screen's own reading of what it shows (docs/mail.md, 「The screen」): which account,
// view and conversation an address names, what each is called, how a row's date and senders read,
// and what a choice in the toolbar or a key on the keyboard does. Nothing here asks the server.

export const ROLE_LABELS: Record<FolderRole, string> = {
  inbox: "mail.label.role_inbox",
  sent: "mail.label.role_sent",
  drafts: "mail.label.role_drafts",
  junk: "mail.label.role_junk",
  trash: "mail.label.role_trash",
};

export const ROLE_ICONS: Record<FolderRole, string> = {
  inbox: "i-material-symbols-inbox-outline",
  sent: "i-material-symbols-send-outline",
  drafts: "i-material-symbols-draft-outline",
  junk: "i-material-symbols-report-outline",
  trash: "i-material-symbols-delete-outline",
};

// The order the folder roles stand in the mail nav, with the starred view after the inbox.
export const NAV_ROLES: readonly FolderRole[] = ["inbox", "sent", "drafts", "junk", "trash"];

// Whether an account's mailbox moves mail between folders, as the kind it names declares
// (docs/mail.md, 「The screen」); never read off a kind's name. While the kinds are unread, or where
// none of them is the account's, it is taken to, so nothing is held back on a guess.
export function accountMoves(account: Pick<MailAccount, "kind"> | undefined, kinds: readonly Pick<MailAccountKind, "name" | "moves">[] | null): boolean {
  return kinds?.find((kind) => kind.name === account?.kind)?.moves ?? true;
}

// One place chosen mail can go: the inbox, the trash, or another folder, each by its folder's name.
export interface MoveTarget {
  key: string;
  label: string;
  icon: string;
  folderName: string;
}

// What a new choice of labels changes on the chosen mail, against the labels all of it has: each label
// chosen that not all of it has goes on, each one all of it had that is no longer chosen comes off.
export function labelChanges(all: ReadonlySet<string>, chosen: readonly string[]): { id: string; on: boolean }[] {
  const now = new Set(chosen);

  return [...chosen.filter((id) => !all.has(id)).map((id) => ({ id, on: true })), ...[...all].filter((id) => !now.has(id)).map((id) => ({ id, on: false }))];
}

// The drag type chosen conversations travel under from a wide list to the mail nav's folders and labels.
export const MAIL_DRAG = "application/x-mail-conversations";

// What a list shows, as its address spells it: a folder role, `starred`, `label:<id>`,
// `folder:<path>`, or `search`.
export type MailViewKey = string;

export interface MailPlace {
  account: string;
  view: MailViewKey;
  // The open conversation, by its key; null when only the list is open.
  thread: string | null;
}

export const MAIL_BASE = "/mail";

// The label column of the composer's rows. A label and its field stand side by side, so the labels
// share one column wide enough for the longest of them on one line: a column narrower than that
// folds a label in two and leaves every row a different height.
export const MAIL_FIELD_LABEL = "w-24 shrink-0 text-sm whitespace-nowrap text-muted";

// The address of a place on the mail screen. A view and a conversation key hold characters an
// address cannot carry bare, so each is escaped as one segment.
export function mailPath(place: MailPlace, query?: string): string {
  const parts = [MAIL_BASE, encodeURIComponent(place.account), encodeURIComponent(place.view)];

  if (place.thread !== null) {
    parts.push(encodeURIComponent(place.thread));
  }

  const path = parts.join("/");

  return place.view === "search" && query !== undefined && query !== "" ? `${path}?q=${encodeURIComponent(query)}` : path;
}

// The place an address's segments after `/mail` name. Anything missing reads as the first account's
// inbox, so a bare `/mail` opens where Gmail opens.
export function parseMailPath(segments: readonly string[], firstAccount: string | null): MailPlace | null {
  const decode = (value: string | undefined) => {
    if (value === undefined || value === "") {
      return undefined;
    }

    try {
      return decodeURIComponent(value);
    } catch {
      return undefined;
    }
  };
  const account = decode(segments[0]) ?? firstAccount;

  if (account === null) {
    return null;
  }

  return { account, view: decode(segments[1]) ?? "inbox", thread: decode(segments[2]) ?? null };
}

export function isRole(view: MailViewKey): view is FolderRole {
  return (NAV_ROLES as readonly string[]).includes(view);
}

// What the screen calls a view: a role's Korean name, the starred view, a label's name, a folder's name, the search.
export function viewTitle(t: Translate, view: MailViewKey, labels: readonly MailLabel[], folders: readonly MailFolder[]): string {
  if (isRole(view)) return t(ROLE_LABELS[view]);
  if (view === "starred") return t("mail.label.view_starred");
  if (view === "search") return t("mail.label.view_search");

  if (view.startsWith("label:")) {
    return labels.find((label) => label.id === view.slice(6))?.name ?? t("mail.label.view_label");
  }

  if (view.startsWith("folder:")) {
    return folders.find((folder) => folder.name === view.slice(7))?.shortName ?? view.slice(7);
  }

  return view;
}

// The folder a list's action works from: the role folder the view names, or the folder itself.
// A view across folders — the starred view, a label, a search — acts on each message where it lies.
export function viewRole(view: MailViewKey, folders: readonly MailFolder[]): FolderRole | null {
  if (isRole(view)) return view;

  if (view.startsWith("folder:")) {
    return folders.find((folder) => folder.name === view.slice(7))?.role ?? null;
  }

  return null;
}

// The one folder a view reads, where it reads one: a folder by its name, or a role the mailbox holds in
// one folder. A view across folders — starred, a label, a search — reads none.
export function viewFolder(view: MailViewKey, folders: readonly MailFolder[]): MailFolder | null {
  if (view.startsWith("folder:")) {
    return folders.find((folder) => folder.name === view.slice(7)) ?? null;
  }

  if (isRole(view)) {
    const holding = folders.filter((folder) => folder.role === view);

    return holding.length === 1 ? holding[0]! : null;
  }

  return null;
}

// A date on a row, in the screen's locale: the time for today, the month and day for this year, the whole
// date before.
export function mailDate(locale: string, at: number, now: number = Date.now()): string {
  const date = new Date(at);
  const today = new Date(now);

  if (date.toDateString() === today.toDateString()) {
    return new Intl.DateTimeFormat(locale, { hour: "numeric", minute: "2-digit" }).format(date);
  }

  if (date.getFullYear() === today.getFullYear()) {
    return new Intl.DateTimeFormat(locale, { month: "long", day: "numeric" }).format(date);
  }

  return new Intl.DateTimeFormat(locale, { year: "numeric", month: "numeric", day: "numeric" }).format(date);
}

// The full date in the screen's locale, for a message's header and a date's tooltip.
export function mailDateLong(locale: string, at: number): string {
  return new Intl.DateTimeFormat(locale, { dateStyle: "long", timeStyle: "short" }).format(new Date(at));
}

// A person as a row names them: their name, or the part of the address before `@`; the account
// itself is "me".
export function personName(t: Translate, person: MailMailbox, self: string): string {
  if (person.address.toLowerCase() === self.toLowerCase()) {
    return t("mail.label.self");
  }

  return person.name != null && person.name !== "" ? person.name : person.address.split("@")[0]!;
}

// A conversation's senders as its row names them, and its count after them when it holds more
// than one message.
export function sendersLine(t: Translate, conversation: Pick<MailConversation, "from" | "count">, self: string): string {
  const names = conversation.from.map((person) => personName(t, person, self));
  const line = names.length === 0 ? t("mail.label.no_sender_mark") : names.slice(0, 3).join(", ");

  return conversation.count > 1 ? `${line} (${countText(conversation.count)})` : line;
}

// A subject as a row shows it, with a mark where the message has none.
export function subjectText(t: Translate, subject: string): string {
  return subject.trim() === "" ? t("mail.label.no_subject") : subject;
}

// The range the toolbar shows: which messages of the view this page holds, of how many where the view is
// one folder and its count is known.
export function rangeText(offset: number, shown: number, total: number | null): string {
  const of = total === null ? "" : ` / ${countText(total)}`;

  if (shown === 0) {
    return `0${of}`;
  }

  return `${countText(offset + 1)}–${countText(offset + shown)}${of}`;
}

// How many messages a page asks for: the service pages by fifty messages.
export const PAGE_MESSAGES = 50;

// The list's leading cell, which holds the checkbox, the star, the unread mark and the avatar.
// The header stands over the same cell, so the width is written once and read by both.
export const MAIL_ROW_LEAD_PX = 112;

// What a one-line row spends besides the sender, the subject and the date: its padding either side
// (px-3), the leading cell, the attachment mark (size-4) and the four gaps (gap-2) between its five
// cells.
const ROW_FIXED_PX = 2 * 12 + MAIL_ROW_LEAD_PX + 16 + 4 * 8;

// The least subject a one-line row is worth standing for: about eleven Hangul characters at the
// row's text size, which still says which thread a row is. Below it the row stacks instead.
export const MAIL_SUBJECT_LEAST_PX = 160;

// Whether a list this wide stands its rows on one line, and at what sender width. The sender gives
// way first, down to its floor, so the subject is the last cell cut (docs/mail.md, 「The screen」).
export function rowFit(room: number, from: number, date: number): { oneLine: boolean; from: number } {
  const left = room - ROW_FIXED_PX - date - MAIL_SUBJECT_LEAST_PX;
  const least = MAIL_COLUMN_BOUNDS.from.least;

  return left < least ? { oneLine: false, from } : { oneLine: true, from: Math.min(from, left) };
}


// What a column's width may be dragged to. A column narrower than its floor shows nothing at all;
// one wider than its ceiling leaves the subject no room, which is the one column that must have it.
export const MAIL_COLUMN_BOUNDS: Record<"from" | "date", { least: number; most: number; fallback: number }> = {
  from: { least: 80, most: 360, fallback: 208 },
  date: { least: 56, most: 200, fallback: 96 },
};

export function clampColumn(column: "from" | "date", width: number): number {
  const bounds = MAIL_COLUMN_BOUNDS[column];

  return Math.min(bounds.most, Math.max(bounds.least, Math.round(width)));
}

// The domain a sender's logo is looked up under: what stands after `@`. An address that is not
// one names no domain, and the screen draws the initial without asking for anything.
export function senderDomain(address: string): string | null {
  const domain = address.slice(address.lastIndexOf("@") + 1).trim().toLowerCase();

  return address.includes("@") && /^[a-z0-9][a-z0-9.-]*\.[a-z]{2,}$/.test(domain) ? domain : null;
}

// The mark an address stands as where no logo was found for it, and the mark an account of one's
// own stands as in the nav: the console's own identity mark, keyed on the address so that the same
// address is always the same letter in the same colour, and recognised by it down a list.
export function addressMark(address: string, name: string): AvatarProps {
  return identityMark(address.trim().toLowerCase(), name.trim() === "" ? address : name);
}

export type SelectKind = "all" | "none" | "read" | "unread" | "starred" | "unstarred";

export const SELECT_KINDS: { kind: SelectKind; label: string }[] = [
  { kind: "all", label: "mail.label.select_all" },
  { kind: "none", label: "mail.label.select_none" },
  { kind: "read", label: "mail.label.select_read" },
  { kind: "unread", label: "mail.label.select_unread" },
  { kind: "starred", label: "mail.label.select_starred" },
  { kind: "unstarred", label: "mail.label.select_unstarred" },
];

// The conversations a choice of the select-all menu selects, by key.
export function selectBy(kind: SelectKind, conversations: readonly MailConversation[]): Set<string> {
  const keep = (conversation: MailConversation): boolean => {
    switch (kind) {
      case "all":
        return true;
      case "none":
        return false;
      case "read":
        return !conversation.unread;
      case "unread":
        return conversation.unread;
      case "starred":
        return conversation.flagged;
      case "unstarred":
        return !conversation.flagged;
    }
  };

  return new Set(conversations.filter(keep).map((conversation) => conversation.key));
}

// Every message of the chosen conversations, which is what a bulk action acts on.
export function messagesOf(keys: ReadonlySet<string>, conversations: readonly MailConversation[]): MailMessage[] {
  return conversations.filter((conversation) => keys.has(conversation.key)).flatMap((conversation) => conversation.messages);
}

// The state group's two buttons follow the selection: mark-read while anything chosen is unread,
// mark-unread once all of it is read; star while anything chosen lacks one.
export function selectionState(keys: ReadonlySet<string>, conversations: readonly MailConversation[]): { anyUnread: boolean; allStarred: boolean } {
  const chosen = conversations.filter((conversation) => keys.has(conversation.key));

  return {
    anyUnread: chosen.some((conversation) => conversation.unread),
    allStarred: chosen.length > 0 && chosen.every((conversation) => conversation.flagged),
  };
}

// The labels a set of conversations carries: those every one of them has, and those only some do.
export function labelsState(keys: ReadonlySet<string>, conversations: readonly MailConversation[]): { all: Set<string>; some: Set<string> } {
  const chosen = conversations.filter((conversation) => keys.has(conversation.key));
  const some = new Set(chosen.flatMap((conversation) => conversation.labelIds));
  const all = new Set([...some].filter((label) => chosen.every((conversation) => conversation.labelIds.includes(label))));

  return { all, some };
}

// The key that presses the screen's one create button, which the button itself answers.
export const CREATE_KEY = "c";

// A key the mail screen answers on a desktop, and what it does. A key typed into a field is the
// field's, never a shortcut; so is one held with a modifier.
export type MailShortcut = "search" | "next" | "previous" | "select" | "star" | "trash" | "reply" | "reply-all" | "forward";

// Each key with what it does, and whether that moves mail between folders: such a key does nothing on an
// account whose kind moves none.
const SHORTCUTS: Record<string, { shortcut: MailShortcut; moves: boolean }> = {
  "/": { shortcut: "search", moves: false },
  j: { shortcut: "next", moves: false },
  k: { shortcut: "previous", moves: false },
  x: { shortcut: "select", moves: false },
  s: { shortcut: "star", moves: false },
  "#": { shortcut: "trash", moves: true },
  r: { shortcut: "reply", moves: false },
  a: { shortcut: "reply-all", moves: false },
  f: { shortcut: "forward", moves: false },
};

export function isTypingTarget(target: EventTarget | null): boolean {
  if (target === null || typeof (target as HTMLElement).closest !== "function") {
    return false;
  }

  const element = target as HTMLElement;

  return element.isContentEditable || element.closest("input, textarea, select, [contenteditable=''], [contenteditable='true'], [role='textbox']") !== null;
}

// The shortcut a key presses, or none where it moves mail and the open account's kind moves none.
export function shortcutOf(event: Pick<KeyboardEvent, "key" | "ctrlKey" | "metaKey" | "altKey" | "target">, moves: boolean): MailShortcut | null {
  if (event.ctrlKey || event.metaKey || event.altKey || isTypingTarget(event.target)) {
    return null;
  }

  const entry = SHORTCUTS[event.key];

  return entry === undefined || (entry.moves && !moves) ? null : entry.shortcut;
}

// The colours a label may take, as the settings offer them.
export const LABEL_COLORS: readonly string[] = ["#ef4444", "#f97316", "#eab308", "#22c55e", "#14b8a6", "#3b82f6", "#8b5cf6", "#ec4899", "#64748b"];

// An address as a field shows it and a completion offers it.
export function addressText(person: { name?: string | null; address: string }): string {
  return person.name == null || person.name === "" ? person.address : `${person.name} <${person.address}>`;
}

// The characters that end one address typed into an address field, as in every mail client.
export const ADDRESS_SEPARATORS: string[] = [",", ";"];

// What was typed into an address field, read as addresses: `Name <a@b>` or a bare address, split at
// commas and semicolons. A piece that is not an address is left out.
export function readAddresses(typed: string): MailMailbox[] {
  return typed
    .split(/[,;\n]/)
    .map((piece) => piece.trim())
    .filter((piece) => piece !== "")
    .map((piece) => {
      const named = /^(.*?)\s*<([^<>\s]+@[^<>\s]+)>$/.exec(piece);

      if (named !== null) {
        return { name: named[1]!.replace(/^"|"$/g, "").trim(), address: named[2]!.toLowerCase() };
      }

      return /^[^@\s<>]+@[^@\s<>]+$/.test(piece) ? { name: null, address: piece.toLowerCase() } : null;
    })
    .filter((person): person is MailMailbox => person !== null);
}

// A size as an attachment chip shows it.
export function sizeText(bytes: number): string {
  if (bytes < 1024) return `${countText(bytes)} B`;
  if (bytes < 1024 * 1024) return `${countText(Math.round(bytes / 1024))} KB`;

  return `${countText(Math.round((bytes / 1024 / 1024) * 10) / 10)} MB`;
}
