import type { Translate } from "bmo-ui/shared/wording";

import type { MailServer, MailServerTest, ServerAuth, ServerDirection, ServerHolder, ServerProtocol, ServerSecurity } from "../../shared/mail";

// The mail servers as the screen reads them (docs/mail.md, 「Mail servers」): what each protocol is — which way
// mail goes over it and the usual port for each security it takes — the words each value is shown by, who holds a
// server, and the servers gathered under their holders in the order the service answers them.

interface ProtocolFacts {
  direction: ServerDirection;
  // The usual port for each security the protocol takes, in the order a form offers them.
  ports: Partial<Record<ServerSecurity, number>>;
  words: string;
}

const PROTOCOLS: Record<ServerProtocol, ProtocolFacts> = {
  imap: { direction: "incoming", ports: { tls: 993, starttls: 143, none: 143 }, words: "mail.server_protocol.imap" },
  pop3: { direction: "incoming", ports: { tls: 995, none: 110 }, words: "mail.server_protocol.pop3" },
  smtp: { direction: "outgoing", ports: { tls: 465, starttls: 587, none: 25 }, words: "mail.server_protocol.smtp" },
};

export const SERVER_PROTOCOLS = Object.keys(PROTOCOLS) as ServerProtocol[];

export const DIRECTIONS: readonly ServerDirection[] = ["incoming", "outgoing"];

export const DIRECTION_WORDS: Record<ServerDirection, string> = {
  incoming: "mail.label.incoming_server",
  outgoing: "mail.label.outgoing_server",
};

export const SECURITY_WORDS: Record<ServerSecurity, string> = {
  tls: "mail.server_security.tls",
  starttls: "mail.server_security.starttls",
  none: "mail.server_security.none",
};

export const AUTH_WORDS: Record<ServerAuth, string> = {
  password: "mail.server_auth.password",
  none: "mail.server_auth.none",
};

export const AUTH_DEFAULT: ServerAuth = "password";

export function directionOf(protocol: ServerProtocol): ServerDirection {
  return PROTOCOLS[protocol].direction;
}

export function protocolWords(t: Translate, protocol: ServerProtocol): string {
  return t(PROTOCOLS[protocol].words);
}

// The securities a protocol takes: POP3 takes no STARTTLS, since its client upgrades no session.
export function securitiesOf(protocol: ServerProtocol): ServerSecurity[] {
  return Object.keys(PROTOCOLS[protocol].ports) as ServerSecurity[];
}

// The security a server is kept with where none is given: TLS for an incoming server, STARTTLS for an outgoing one.
export function defaultSecurity(protocol: ServerProtocol): ServerSecurity {
  return directionOf(protocol) === "incoming" ? "tls" : "starttls";
}

// The port a server is kept on where none is given, for the security it is reached over.
export function usualPort(protocol: ServerProtocol, security: ServerSecurity): number {
  const ports = PROTOCOLS[protocol].ports;

  return ports[security] ?? ports[defaultSecurity(protocol)]!;
}

// Who holds a server, in the screen's words: the company or unit by its name, the platform, or the owner.
export function holderWords(t: Translate, holder: ServerHolder): string {
  if (holder.link === "own") return t("mail.label.holder_own");
  if (holder.link === "platform") return t("mail.label.holder_platform");

  return holder.name ?? holder.link;
}

export interface HolderGroup {
  holder: ServerHolder;
  servers: MailServer[];
}

// The servers gathered under their holders, each holder once, in the order the servers stand.
export function holderGroups(servers: readonly MailServer[]): HolderGroup[] {
  const groups: HolderGroup[] = [];

  for (const server of servers) {
    const group = groups.find((one) => one.holder.link === server.holder.link);

    if (group === undefined) groups.push({ holder: server.holder, servers: [server] });
    else group.servers.push(server);
  }

  return groups;
}

// What a connection test came to, as the words said beside its button: reached, not reached, or reached and
// refusing the sign-in.
export function testWords(t: Translate, test: MailServerTest): string {
  if (!test.reached) return t("mail.status.server_unreached");
  if (test.signedIn === false) return t("mail.status.server_sign_in_failed");

  return t("mail.status.server_reached");
}

// A server's address as one line: its host and port.
export function serverAddress(server: Pick<MailServer, "host" | "port">): string {
  return `${server.host}:${server.port}`;
}
