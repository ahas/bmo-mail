import type { BrowserContext, Route } from "playwright-core";

import type { Draft, DraftPatch, MailAccount, MailAddress, MailFolder, MailLabel, MailMessage, MailRule, MailSettings } from "../../shared/mail";

// A stand-in for the mail service behind the screen's relay address (docs/api.md): one account holding the
// role folders and the folders of its own a test names, the labels it names, a few messages in the inbox, three
// filter rules, the known addresses it names and the drafts the composer writes. It keeps what the screen
// changes, so a page loaded again reads what an earlier one saved, and it answers only the routes the list, the
// nav, the settings and the composer ask; any other is answered 404 and counted, so a test sees a route the
// screen began asking that the stand-in does not hold.

export const ACCOUNT = "box";

const ROLE_FOLDERS: Array<Pick<MailFolder, "name" | "role">> = [
  { name: "INBOX", role: "inbox" },
  { name: "Sent", role: "sent" },
  { name: "Drafts", role: "drafts" },
  { name: "Junk", role: "junk" },
  { name: "Trash", role: "trash" },
];

export interface StandIn {
  messages: MailMessage[];
  rules: MailRule[];
  labels: MailLabel[];
  // The account's folders of its own, by name, after the role folders.
  folders: string[];
  // The addresses the service knows, found by the words typed.
  known: MailAddress[];
  drafts: Draft[];
  // The routes asked that the stand-in does not answer, as `METHOD path`.
  unanswered: string[];
}

function message(id: string, subject: string, at: number): MailMessage {
  return {
    id,
    accountName: ACCOUNT,
    threadId: `thread-${id}`,
    folderName: "INBOX",
    references: [],
    subject,
    from: { name: `Sender ${id}`, address: `sender-${id}@example.com` },
    to: [{ name: null, address: "box@example.com" }],
    cc: [],
    bcc: [],
    replyTo: [],
    at,
    flags: ["\\Seen"],
    labelIds: [],
    snippet: `About ${subject.toLowerCase()}`,
    parts: [],
  };
}

function rule(id: number, contains: string): MailRule {
  return { id, position: id, match: { kind: "all", conditions: [{ field: "subject", header: null, contains }] }, actions: [{ kind: "star" }], on: true };
}

export function standIn(): StandIn {
  const now = Date.UTC(2026, 10, 1, 9);

  return {
    messages: [message("m1", "First note", now), message("m2", "Second note", now - 60_000), message("m3", "Third note", now - 120_000)],
    rules: [rule(1, "alpha"), rule(2, "beta"), rule(3, "gamma")],
    labels: [{ id: "work", name: "Work", colour: "blue", position: 1 }],
    folders: ["Projects"],
    known: [],
    drafts: [],
    unanswered: [],
  };
}

const ACCOUNT_BODY: MailAccount = {
  name: ACCOUNT,
  kind: "imap",
  address: "box@example.com",
  position: 1,
  settings: {},
  unreadCount: 0,
  reachable: true,
};

const SETTINGS: MailSettings = { signature: "", undoSendMs: 0, readReceipts: false, showImages: false, spamSenders: [] };

const page = <T>(items: T[]) => ({ items, more: false });

function folders(state: StandIn): MailFolder[] {
  return [...ROLE_FOLDERS, ...state.folders.map((name) => ({ name, role: null }))].map((folder, index) => ({
    accountName: ACCOUNT,
    name: folder.name,
    shortName: folder.name,
    role: folder.role,
    position: index + 1,
    unreadCount: 0,
    totalCount: state.messages.filter((one) => one.folderName === folder.name).length,
  }));
}

// The rules numbered again from 1 in their order, as the service keeps their positions.
function renumber(rules: MailRule[]): MailRule[] {
  return rules.map((one, index) => ({ ...one, position: index + 1 }));
}

type Answer = { status: number; body?: unknown };

// What the stand-in answers one call, by its method, its path past the relay's `/mail` and its query.
function answer(state: StandIn, method: string, path: string, query: URLSearchParams, sent: unknown): Answer | null {
  const parts = path.split("/").filter((part) => part !== "").map(decodeURIComponent);

  if (method === "GET" && path === "/accounts") return { status: 200, body: page([ACCOUNT_BODY]) };
  if (method === "GET" && path === "/summary") return { status: 200, body: { accounts: [ACCOUNT_BODY], unreadCount: 0 } };
  if (method === "GET" && path === "/account-kinds") return { status: 200, body: page([]) };
  // No sender has a logo, which the service answers as it does a domain it found none for.
  if (method === "GET" && parts[0] === "avatars") return { status: 404, body: { code: "not-found", error: "No logo." } };
  if (parts[0] !== "accounts" || parts[1] !== ACCOUNT) return null;

  const [, , what, id] = parts;

  if (method === "GET" && what === "folders") return { status: 200, body: page(folders(state)) };
  if (method === "GET" && what === "labels") return { status: 200, body: page(state.labels) };
  if (method === "GET" && what === "rules") return { status: 200, body: page(state.rules) };
  if (method === "GET" && what === "settings") return { status: 200, body: SETTINGS };
  if (method === "GET" && what === "addresses") {
    const words = (query.get("q") ?? "").toLowerCase();

    return { status: 200, body: page(state.known.filter((one) => `${one.name ?? ""} ${one.address}`.toLowerCase().includes(words))) };
  }

  if (method === "POST" && what === "drafts" && id === undefined) {
    const draft: Draft = {
      id: `d${state.drafts.length + 1}`,
      accountName: ACCOUNT,
      kind: "new",
      from: { name: null, address: ACCOUNT_BODY.address },
      to: [],
      cc: [],
      bcc: [],
      subject: "",
      body: "",
      attachments: [],
      receiptRequested: false,
      references: [],
      changedAt: Date.UTC(2026, 10, 1, 9),
    };

    state.drafts.push(draft);

    return { status: 200, body: draft };
  }

  if ((method === "PATCH" || method === "GET") && what === "drafts" && id !== undefined) {
    const found = state.drafts.find((one) => one.id === id);

    if (found === undefined) return { status: 404, body: { code: "not-found", error: "No such draft." } };

    Object.assign(found, method === "PATCH" ? (sent as DraftPatch) : {});

    return { status: 200, body: found };
  }

  if (method === "GET" && what === "messages" && id === undefined) {
    const folderName = query.get("folderName");
    const labelId = query.get("labelId");
    const items = state.messages.filter((one) => (labelId !== null ? one.labelIds.includes(labelId) : one.folderName === (folderName ?? "INBOX")));

    return { status: 200, body: page(items) };
  }

  if (method === "PATCH" && what === "messages" && id !== undefined) {
    const found = state.messages.find((one) => one.id === id);

    if (found === undefined) return { status: 404, body: { code: "not-found", error: "No such message." } };

    Object.assign(found, sent as Partial<MailMessage>);

    return { status: 200, body: found };
  }

  if (method === "PATCH" && what === "rules" && id !== undefined) {
    const at = state.rules.findIndex((one) => one.id === Number(id));
    const patch = sent as { position?: number };

    if (at === -1) return { status: 404, body: { code: "not-found", error: "No such rule." } };

    if (patch.position !== undefined) {
      const [moved] = state.rules.splice(at, 1);

      state.rules.splice(Math.min(Math.max(patch.position - 1, 0), state.rules.length), 0, moved!);
    }

    state.rules = renumber(state.rules);

    return { status: 200, body: state.rules.find((one) => one.id === Number(id)) };
  }

  return null;
}

// The stand-in put behind the screen's calls in a browser context: every fetch the page makes under the
// relay's `/mail`, the event stream held open with nothing said. A page load under `/mail` is the screen's
// own and goes to the server.
export async function standBehind(context: BrowserContext, state: StandIn): Promise<void> {
  await context.route(
    (url) => url.pathname.startsWith("/mail/"),
    async (route: Route) => {
      const request = route.request();

      if (!["fetch", "xhr", "eventsource", "image"].includes(request.resourceType())) return route.continue();

      const url = new URL(request.url());
      const path = url.pathname.slice("/mail".length);

      if (path === "/events") return route.fulfill({ status: 200, contentType: "text/event-stream", body: ": open\n\n" });

      const body = request.postData();
      const given = answer(state, request.method(), path, url.searchParams, body === null ? undefined : JSON.parse(body));

      if (given === null) {
        state.unanswered.push(`${request.method()} ${path}`);

        return route.fulfill({ status: 404, contentType: "application/json", body: JSON.stringify({ code: "not-found", error: "Not in the stand-in." }) });
      }

      return route.fulfill({ status: given.status, contentType: "application/json", body: JSON.stringify(given.body ?? null) });
    },
  );
}
