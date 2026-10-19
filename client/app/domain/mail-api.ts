import type {
  Draft,
  MailAddress,
  MailAnswer,
  MailFolderState,
  MailLabel,
  MailMessage,
  MailPage,
  MailRef,
  MailRule,
  MailSettings,
  MailSort,
  PublicMailAccount,
  RuleDraft,
  SendState,
  StagedFile,
} from "../../shared/mail";

import { appUrl } from "./mail-urls";

// The mail routes as the screen calls them (docs/mail.md). Every answer may be that there are no
// mail accounts at all; the screen asks `configured` once, through the accounts, and every other
// call reads its answer as the shape it asked for.

type Answer<T> = MailAnswer<T>;

function base(account: string): string {
  return `/api/mail/${encodeURIComponent(account)}`;
}

// The answer of a route called on an account the screen already knows exists.
function sure<T>(answer: Answer<T>): T {
  if (!answer.configured) {
    throw new Error("no mail account is configured");
  }

  return answer;
}

export async function mailAccounts(): Promise<Answer<{ accounts: PublicMailAccount[]; maxLabels: number }>> {
  return $fetch("/api/mail/accounts");
}

export function mailApi(account: string) {
  const at = base(account);
  const post = <T>(path: string, body: unknown, method: "POST" | "PUT" | "PATCH" | "DELETE" = "POST") => $fetch<Answer<T>>(`${at}${path}`, { method, body: body as Record<string, unknown> }).then(sure);

  return {
    folders: () => $fetch<Answer<{ folders: MailFolderState[]; labels: MailLabel[] }>>(`${at}/folders`).then(sure),
    list: (view: string, offset: number, sort: MailSort, q?: string) =>
      $fetch<Answer<MailPage>>(`${at}/list`, { query: { view, offset, q, sort: sort.by, dir: sort.ascending ? "asc" : "desc", unread: sort.unread ? "1" : undefined } }).then(sure),
    thread: (key: string, folder: string) => $fetch<Answer<{ messages: MailMessage[] }>>(`${at}/thread`, { query: { key, folder } }).then(sure),
    flags: (refs: MailRef[], add: string[], remove: string[]) => post<object>("/flags", { refs, add, remove }),
    readAll: (view: string, q?: string) => post<{ marked: number }>("/read-all", { view, q }),
    move: (refs: MailRef[], to: "trash" | "spam" | "not-spam" | "folder", folder?: string) => post<object>("/move", { refs, to, folder }),
    deleteForever: (refs: MailRef[]) => post<object>("/delete", { refs }),
    createLabel: (name: string, color: string) => post<{ label: MailLabel }>("/labels", { name, color }),
    updateLabel: (id: string, change: { name?: string; color?: string }) => post<object>(`/labels/${encodeURIComponent(id)}`, change, "PATCH"),
    deleteLabel: (id: string) => post<object>(`/labels/${encodeURIComponent(id)}`, {}, "DELETE"),
    rules: () => $fetch<Answer<{ rules: MailRule[]; presets: RuleDraft[] }>>(`${at}/rules`).then(sure),
    createRule: (rule: RuleDraft) => post<{ rule: MailRule }>("/rules", rule),
    updateRule: (id: number, rule: RuleDraft) => post<object>(`/rules/${id}`, rule, "PUT"),
    deleteRule: (id: number) => post<object>(`/rules/${id}`, {}, "DELETE"),
    orderRules: (ids: number[]) => post<object>("/rules/order", { ids }),
    runRules: (folder: string) => post<{ kept: number; moved: number }>("/rules/run", { folder }),
    settings: () => $fetch<Answer<MailSettings & { spamSenders: string[] }>>(`${at}/settings`).then(sure),
    saveSettings: (settings: Partial<MailSettings>) => post<MailSettings>("/settings", settings),
    addSpamSenders: (add: string[]) => post<{ spamSenders: string[] }>("/spam-senders", { add }),
    removeSpamSender: (address: string) => post<{ spamSenders: string[] }>("/spam-senders", { address }, "DELETE"),
    addresses: (q: string) => $fetch<Answer<{ addresses: MailAddress[] }>>(`${at}/addresses`, { query: { q } }).then(sure),
    upload: async (files: File[], attached: number[]) => {
      const form = new FormData();

      for (const file of files) {
        form.append("file", file, file.name);
      }

      form.append("attached", JSON.stringify(attached));

      return sure(await $fetch<Answer<{ files: StagedFile[] }>>(`${at}/upload`, { method: "POST", body: form }));
    },
    startDraft: (kind: "new" | "reply" | "reply-all" | "forward", ref?: MailRef) => post<{ draft: Draft }>("/draft/start", { kind, ...ref }),
    openDraft: (ref: MailRef) => $fetch<Answer<{ draft: Draft }>>(`${at}/draft`, { query: { folder: ref.folder, uid: ref.uid } }).then(sure),
    saveDraft: (draft: Draft) => post<{ saved: MailRef }>("/draft", draft),
    discardDraft: (ref: MailRef) => post<object>("/draft", ref, "DELETE"),
    send: (draft: Draft) => post<{ id: string; draft: MailRef; holdMs: number }>("/send", draft),
    sendState: (id: string) => $fetch<Answer<{ state: SendState | null }>>(`${at}/send/${id}`).then(sure),
    cancelSend: (id: string) => post<{ draft: MailRef | null }>(`/send/${id}`, {}, "DELETE"),
    answerReceipt: (ref: MailRef, send: boolean) => post<object>("/receipt", { ...ref, send }),
  };
}

export type MailApi = ReturnType<typeof mailApi>;

// Where an attachment downloads from, and where an image a body carries by `cid:` is fetched.
export function partUrl(account: string, ref: MailRef, part: string): string {
  return appUrl(`${base(account)}/part?folder=${encodeURIComponent(ref.folder)}&uid=${ref.uid}&part=${encodeURIComponent(part)}`);
}

// Where a sender domain's logo is fetched from. The console looks the domain up and fetches it,
// so the browser asks this address and never the sender's own (docs/mail.md, 「Sender avatars」).
export function avatarUrl(domain: string): string {
  return appUrl(`/api/mail/avatar?domain=${encodeURIComponent(domain)}`);
}

export function cidUrl(account: string, ref: MailRef, cid: string): string {
  return appUrl(`${base(account)}/cid?folder=${encodeURIComponent(ref.folder)}&uid=${ref.uid}&cid=${encodeURIComponent(cid)}`);
}
