import type { BrowserContext, Route } from "playwright-core";

import type {
  Draft,
  DraftPatch,
  FolderRole,
  MailAccount,
  MailAccountKind,
  MailAddress,
  MailFolder,
  MailLabel,
  MailMessage,
  MailRule,
  MailServer,
  MailServerPatch,
  MailSettings,
  NewMailAccount,
  NewMailServer,
  NewMailServerTest,
} from "../../shared/mail";

// A stand-in for the mail service behind the screen's relay address (docs/api.md): one account of a kind that
// declares whether its mailbox moves mail between folders, holding the role folders a test names and the folders
// of its own it names, the labels it names, a few messages in the inbox, three
// filter rules, the known addresses it names and the drafts the composer writes; and the mail servers the owner
// reaches — a company's incoming and outgoing server the account stands on and the platform's outgoing one — with
// the servers of the owner's own added, changed and removed, the connection tests asked and the accounts added on
// them. It keeps what the screen changes, so a page loaded again reads what an earlier one saved, and it answers
// only the routes the list, the nav, the settings, the composer and the accounts ask; any other is answered 404
// and counted, so a test sees a route the screen began asking that the stand-in does not hold.

export const ACCOUNT = "box";

// The company holding the servers the account stands on, by its name on the delegation chain.
export const COMPANY = "Example Company";

// A host a connection test does not reach.
export const UNREACHED_HOST = "unreached.example.com";

const ROLE_FOLDERS: Array<Pick<MailFolder, "name" | "role">> = [
  { name: "INBOX", role: "inbox" },
  { name: "Sent", role: "sent" },
  { name: "Drafts", role: "drafts" },
  { name: "Junk", role: "junk" },
  { name: "Trash", role: "trash" },
];

export interface StandIn {
  // Whether the account's kind moves mail between folders, as the kinds the service declares say; null where the
  // service declares no kind at all.
  moves: boolean | null;
  // The role folders the mailbox holds.
  roles: FolderRole[];
  messages: MailMessage[];
  rules: MailRule[];
  labels: MailLabel[];
  // The account's folders of its own, by name, after the role folders.
  folders: string[];
  // The addresses the service knows, found by the words typed.
  known: MailAddress[];
  drafts: Draft[];
  // The owner's accounts, the one above first.
  accounts: MailAccount[];
  // The servers the owner reaches, in the order the service answers them.
  servers: MailServer[];
  // The connection tests asked, as they were sent.
  serverTests: NewMailServerTest[];
  // The accounts added, as they were sent.
  added: NewMailAccount[];
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
    moves: null,
    roles: ROLE_FOLDERS.map((one) => one.role!),
    messages: [message("m1", "First note", now), message("m2", "Second note", now - 60_000), message("m3", "Third note", now - 120_000)],
    rules: [rule(1, "alpha"), rule(2, "beta"), rule(3, "gamma")],
    labels: [{ id: "work", name: "Work", colour: "blue", position: 1 }],
    folders: ["Projects"],
    known: [],
    drafts: [],
    accounts: [{ ...ACCOUNT_BODY }],
    servers: [
      server(1, "imap", "imap.example.com", 993, "tls", COMPANY_HOLDER, 4),
      server(2, "smtp", "smtp.example.com", 587, "starttls", COMPANY_HOLDER, 4),
      server(3, "smtp", "relay.example.net", 465, "tls", { link: "platform", name: null }, 0),
    ],
    serverTests: [],
    added: [],
    unanswered: [],
  };
}

const COMPANY_HOLDER = { link: "unit:7", name: COMPANY };

function server(id: number, protocol: MailServer["protocol"], host: string, port: number, security: MailServer["security"], holder: MailServer["holder"], accountCount: number): MailServer {
  return { id, protocol, direction: protocol === "smtp" ? "outgoing" : "incoming", name: host, host, port, security, tlsName: null, auth: "password", holder, mayChange: holder.link === "own", accountCount };
}

const ACCOUNT_BODY: MailAccount = {
  name: ACCOUNT,
  kind: "imap-smtp",
  address: "box@example.com",
  position: 1,
  settings: {},
  incomingServer: 1,
  outgoingServer: 2,
  unreadCount: 0,
  reachable: true,
};

const SETTINGS: MailSettings = { signature: "", undoSendMs: 0, readReceipts: false, showImages: false, spamSenders: [] };

const page = <T>(items: T[]) => ({ items, more: false });

const PASSWORD_FIELDS: MailAccountKind["fields"] = [
  { name: "login", kind: "text", required: false, default: null },
  { name: "password", kind: "secret", required: true, default: null },
];

// The kinds the service declares, where it declares any: the account's own, moving mail or not as the test says,
// and the POP3 kind beside it.
function kinds(state: StandIn): MailAccountKind[] {
  if (state.moves === null) return [];

  return [
    { name: ACCOUNT_BODY.kind, signIn: "password", moves: state.moves, servers: { incoming: "imap", outgoing: "smtp" }, fields: PASSWORD_FIELDS },
    { name: "pop3-smtp", signIn: "password", moves: false, servers: { incoming: "pop3", outgoing: "smtp" }, fields: PASSWORD_FIELDS },
  ];
}

const USUAL_PORTS: Record<MailServer["protocol"], Partial<Record<MailServer["security"], number>>> = {
  imap: { tls: 993, starttls: 143, none: 143 },
  pop3: { tls: 995, none: 110 },
  smtp: { tls: 465, starttls: 587, none: 25 },
};

// A server of the owner's own added, its defaults filled in as the service fills them.
function addServer(state: StandIn, sent: NewMailServer): MailServer {
  const security = sent.security ?? (sent.protocol === "smtp" ? "starttls" : "tls");
  const kept: MailServer = {
    ...server(Math.max(0, ...state.servers.map((one) => one.id)) + 1, sent.protocol, sent.host, sent.port ?? USUAL_PORTS[sent.protocol][security]!, security, { link: "own", name: null }, 0),
    name: sent.name ?? sent.host,
    tlsName: sent.tlsName ?? null,
    auth: sent.auth ?? "password",
  };

  state.servers.push(kept);

  return kept;
}

// What the server routes answer: the servers, an own one added, changed or removed, and a connection test.
function answerServers(state: StandIn, method: string, parts: string[], query: URLSearchParams, sent: unknown): Answer | null {
  const [what, id] = parts;
  const found = state.servers.find((one) => String(one.id) === id);

  if (what === "server-tests" && method === "POST") {
    const test = sent as NewMailServerTest;

    state.serverTests.push(test);

    return { status: 200, body: { reached: test.host !== UNREACHED_HOST, signedIn: null, failure: test.host === UNREACHED_HOST ? "unreachable" : null } };
  }

  if (what !== "servers") return null;
  if (method === "GET" && id === undefined) {
    const direction = query.get("direction");

    return { status: 200, body: page(state.servers.filter((one) => direction === null || one.direction === direction)) };
  }
  if (method === "POST" && id === undefined) return { status: 201, body: addServer(state, sent as NewMailServer) };
  if (found === undefined || !found.mayChange) return { status: 404, body: { code: "not-found", error: "No such server." } };
  if (method === "PATCH") {
    Object.assign(found, Object.fromEntries(Object.entries(sent as MailServerPatch).filter(([, value]) => value !== null && value !== undefined)));

    return { status: 200, body: found };
  }
  if (method === "DELETE") {
    if (found.accountCount > 0) return { status: 409, body: { code: "server-in-use", error: "Accounts stand on the server." } };

    state.servers = state.servers.filter((one) => one !== found);

    return { status: 204 };
  }

  return null;
}

// An account added on the servers it names, standing on them from then on.
function addAccount(state: StandIn, sent: NewMailAccount): Answer {
  const incoming = state.servers.find((one) => one.id === sent.incomingServer);
  const outgoing = state.servers.find((one) => one.id === sent.outgoingServer);

  if (incoming === undefined || outgoing === undefined) return { status: 422, body: { code: "unknown-server", error: "No such server." } };

  const address = sent.address ?? "";
  const added: MailAccount = {
    ...ACCOUNT_BODY,
    name: sent.name ?? address.split("@")[0]!,
    kind: sent.kind ?? ACCOUNT_BODY.kind,
    address,
    position: state.accounts.length + 1,
    incomingServer: incoming.id,
    outgoingServer: outgoing.id,
  };

  state.added.push(sent);
  state.accounts.push(added);
  incoming.accountCount += 1;
  outgoing.accountCount += 1;

  return { status: 201, body: added };
}

function folders(state: StandIn): MailFolder[] {
  return [...ROLE_FOLDERS.filter((one) => state.roles.includes(one.role!)), ...state.folders.map((name) => ({ name, role: null }))].map((folder, index) => ({
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

  if (method === "GET" && path === "/accounts") return { status: 200, body: page(state.accounts) };
  if (method === "POST" && path === "/accounts") return addAccount(state, sent as NewMailAccount);
  if (method === "GET" && path === "/summary") return { status: 200, body: { accounts: state.accounts, unreadCount: 0 } };
  if (method === "GET" && path === "/account-kinds") return { status: 200, body: page(kinds(state)) };

  const serverAnswer = answerServers(state, method, parts, query, sent);

  if (serverAnswer !== null) return serverAnswer;
  // No sender has a logo, which the service answers as it does a domain it found none for.
  if (method === "GET" && parts[0] === "avatars") return { status: 404, body: { code: "not-found", error: "No logo." } };
  if (parts[0] !== "accounts" || !state.accounts.some((one) => one.name === parts[1])) return null;

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

      if (given.body === undefined) return route.fulfill({ status: given.status });

      return route.fulfill({ status: given.status, contentType: "application/json", body: JSON.stringify(given.body) });
    },
  );
}
