import { relayAddress } from "bmo-ui/app/domain/relay";

import { FLAGGED, JUNK, NOT_JUNK, SEEN } from "../../shared/mail";
import { appUrl } from "./mail-urls";

import type {
  Draft,
  DraftPatch,
  FolderRole,
  MailAccount,
  MailAddress,
  MailFolder,
  MailLabel,
  MailMessage,
  MailMessagePage,
  MailRule,
  MailSend,
  MailSettings,
  MailSettingsPatch,
  MailSummary,
  MailThread,
  NewRule,
  RulePatch,
} from "../../shared/mail";
import type { MailViewKey } from "./mail";

// The mail service's routes as the screen calls them (docs/api.md), each at the address the console's relay
// answers for the service. Bodies are the members `shared/mail` names; what the screen does with a choice —
// a star, a move to spam, a page of a view — is read here into the route that does it.

const SERVICE = "mail";

// The address of one of the service's routes, as `$fetch` and the browser's own requests take it.
export function mailAddress(path: string): string {
  return relayAddress(SERVICE, path);
}

function segment(key: string): string {
  return encodeURIComponent(key);
}

// A page of a list's items.
interface Page<T> {
  items: T[];
  more: boolean;
}

export async function mailAccounts(): Promise<MailAccount[]> {
  return (await $fetch<Page<MailAccount>>(mailAddress("/accounts"))).items;
}

export async function mailSummary(): Promise<MailSummary> {
  return $fetch<MailSummary>(mailAddress("/summary"));
}

// What a change of messages asks for: flags set and taken off, labels added and taken away, a folder to
// move into.
export interface MailChange {
  add?: string[];
  remove?: string[];
  addLabels?: string[];
  removeLabels?: string[];
  folderName?: string;
}

// The whole of the flags and labels a message should carry once the change is made, as the route takes them.
function changed(message: MailMessage, change: MailChange): { flags: string[]; labelIds: string[]; folderName?: string } {
  const flags = new Set(message.flags);
  const labels = new Set(message.labelIds);

  for (const flag of change.add ?? []) flags.add(flag);
  for (const flag of change.remove ?? []) flags.delete(flag);
  for (const id of change.addLabels ?? []) labels.add(id);
  for (const id of change.removeLabels ?? []) labels.delete(id);

  return { flags: [...flags], labelIds: [...labels], ...(change.folderName === undefined ? {} : { folderName: change.folderName }) };
}

// The folder of a role, by its name, where the mailbox has one.
export function roleFolder(folders: readonly MailFolder[], role: FolderRole): string | null {
  return folders.find((folder) => folder.role === role)?.name ?? null;
}

// What a page of a view asks the service: a folder by its name, a label, or words, with the unread filter
// as the search word that holds a list to unread mail. A role the mailbox holds in two folders is read as
// the search that stands over both.
export function pageQuery(view: MailViewKey, words: string, unreadOnly: boolean, folders: readonly MailFolder[]): { folderName?: string; q?: string; labelId?: string } {
  const unread = unreadOnly ? "is:unread" : "";
  const join = (...parts: string[]) => parts.filter((part) => part !== "").join(" ");
  const ask: { folderName?: string; q?: string; labelId?: string } = {};

  if (view === "search") {
    ask.q = join(words, unread);
  } else if (view === "starred") {
    ask.q = join("is:starred", unread);
  } else if (view.startsWith("label:")) {
    ask.labelId = view.slice(6);
  } else if (view.startsWith("folder:")) {
    ask.folderName = view.slice(7);
  } else {
    const holding = folders.filter((folder) => folder.role === view);

    if (holding.length === 1) ask.folderName = holding[0]!.name;
    else if (view !== "inbox") ask.q = `in:${view}`;
  }

  if (unread !== "" && ask.q === undefined) ask.q = unread;
  else if (unread !== "" && ask.q !== undefined && !ask.q.includes(unread)) ask.q = join(ask.q, unread);

  return ask;
}

function readAsBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();

    reader.onload = () => {
      const url = String(reader.result);

      resolve(url.slice(url.indexOf(",") + 1));
    };
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(file);
  });
}

export function mailApi(account: string) {
  const at = (path: string) => mailAddress(`/accounts/${segment(account)}${path}`);
  const items = async <T>(path: string, query?: Record<string, string | number | undefined>) => (await $fetch<Page<T>>(at(path), { query })).items;
  const body = <T>(method: "POST" | "PUT" | "PATCH", path: string, sent?: unknown) => $fetch<T>(at(path), { method, body: sent as Record<string, unknown> });
  const remove = (path: string) => $fetch(at(path), { method: "DELETE" });

  // One message changed; the answer is the message as it now stands, under its new id where it moved, or
  // null where the server did not say where it landed.
  async function change(message: MailMessage, asked: MailChange): Promise<MailMessage | null> {
    const answer = await $fetch<MailMessage | undefined>(at(`/messages/${segment(message.id)}`), { method: "PATCH", body: changed(message, asked) });

    return answer ?? null;
  }

  return {
    folders: () => items<MailFolder>("/folders"),
    labels: () => items<MailLabel>("/labels"),
    page: (ask: { folderName?: string; q?: string; labelId?: string }, before: number | null) =>
      $fetch<MailMessagePage>(at("/messages"), { query: { ...ask, ...(before === null ? {} : { before }) } }),
    thread: (threadId: string, folderName: string | null) =>
      $fetch<MailThread>(at(`/threads/${segment(threadId)}`), { query: folderName === null ? {} : { folderName } }),
    change,
    // The same change made to each message, one at a time so a folder is never asked twice at once.
    changeAll: async (messages: readonly MailMessage[], asked: MailChange) => {
      for (const message of messages) await change(message, asked);
    },
    // Each message marked read or unread, those already so passed over.
    markRead: async (messages: readonly MailMessage[], read: boolean) => {
      for (const message of messages) {
        if (message.flags.includes(SEEN) !== read) await change(message, read ? { add: [SEEN] } : { remove: [SEEN] });
      }
    },
    star: async (messages: readonly MailMessage[], on: boolean) => {
      for (const message of messages) await change(message, on ? { add: [FLAGGED] } : { remove: [FLAGGED] });
    },
    // Spam reported: into the junk folder, the learning keyword set; not spam the other way, into the inbox.
    reportSpam: async (messages: readonly MailMessage[], junk: string) => {
      for (const message of messages) await change(message, { add: [JUNK], remove: [NOT_JUNK], folderName: junk });
    },
    reportNotSpam: async (messages: readonly MailMessage[], inbox: string) => {
      for (const message of messages) await change(message, { add: [NOT_JUNK], remove: [JUNK], folderName: inbox });
    },
    deleteForever: async (messages: readonly MailMessage[]) => {
      for (const message of messages) await remove(`/messages/${segment(message.id)}`);
    },
    readFolder: (folderName: string) => body<MailFolder>("PATCH", `/folders/${segment(folderName)}`, { read: true }),
    createLabel: (name: string, colour: string) => body<MailLabel>("POST", "/labels", { name, colour }),
    updateLabel: (id: string, patch: { name?: string; colour?: string; position?: number }) => body<MailLabel>("PATCH", `/labels/${segment(id)}`, patch),
    deleteLabel: (id: string) => remove(`/labels/${segment(id)}`),
    rules: () => items<MailRule>("/rules"),
    createRule: (rule: NewRule) => body<MailRule>("POST", "/rules", rule),
    updateRule: (id: number, patch: RulePatch) => body<MailRule>("PATCH", `/rules/${id}`, patch),
    deleteRule: (id: number) => remove(`/rules/${id}`),
    runRules: (folderName: string) => body<{ folderName: string; movedCount: number; labelledCount: number }>("POST", "/rule-runs", { folderName }),
    settings: () => $fetch<MailSettings>(at("/settings")),
    saveSettings: (patch: MailSettingsPatch) => body<MailSettings>("PATCH", "/settings", patch),
    addSpamSenders: async (addresses: readonly string[]) => {
      for (const address of addresses) await body("PUT", `/spam-senders/${segment(address)}`);
    },
    removeSpamSender: (address: string) => remove(`/spam-senders/${segment(address)}`),
    addresses: (q: string) => items<MailAddress>("/addresses", { q }),
    // A draft started: new, a reply to the message, to all of its recipients, or a forward of it.
    startDraft: (kind: "new" | "reply" | "reply-all" | "forward", message?: MailMessage) =>
      body<Draft>("POST", "/drafts", kind === "new" || message === undefined ? {} : kind === "forward" ? { forward: message.id } : { replyTo: message.id, replyAll: kind === "reply-all" }),
    openDraft: (id: string) => $fetch<Draft>(at(`/drafts/${segment(id)}`)),
    saveDraft: (id: string, patch: DraftPatch) => body<Draft>("PATCH", `/drafts/${segment(id)}`, patch),
    attach: async (id: string, file: File) =>
      body<Draft>("POST", `/drafts/${segment(id)}/attachments`, { name: file.name, mediaType: file.type === "" ? "application/octet-stream" : file.type, content: await readAsBase64(file) }),
    discardDraft: (id: string) => remove(`/drafts/${segment(id)}`),
    send: (draftId: string) => body<MailSend>("POST", "/sends", { draftId }),
    sendState: (id: string) => $fetch<MailSend>(at(`/sends/${segment(id)}`)),
    cancelSend: (id: string) => remove(`/sends/${segment(id)}`),
    sendReceipt: (message: MailMessage) => body<{ messageId: string; sent: boolean }>("POST", "/receipts", { messageId: message.id }),
  };
}

export type MailApi = ReturnType<typeof mailApi>;

// Where a part of a message downloads from, by its number, and where a picture its HTML body carries by
// `cid:` is fetched, by its content id.
export function partUrl(account: string, message: Pick<MailMessage, "id">, part: string): string {
  return appUrl(mailAddress(`/accounts/${segment(account)}/messages/${segment(message.id)}/parts/${segment(part)}/content`));
}

export function cidUrl(account: string, message: Pick<MailMessage, "id">, cid: string): string {
  return partUrl(account, message, cid);
}

// Where a sender's logo is fetched from. The service looks the domain up and fetches it, so the browser
// asks this address and never the sender's own (docs/mail.md, 「Sender avatars」).
export function avatarUrl(address: string): string {
  return appUrl(mailAddress(`/avatars/${segment(address)}`));
}
