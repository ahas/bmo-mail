-- The mail servers (docs/mail.md, 「Mail servers」): each one kept once, under its holder — `owner:` and the owner
-- key for a person's own, else the delegation chain's link, `platform` or `unit:<id>` — and chosen by any number
-- of accounts. A server holds no secret: the login and the password stay sealed on each account.
CREATE TABLE servers (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  holder TEXT NOT NULL,
  protocol TEXT NOT NULL,
  name TEXT NOT NULL,
  host TEXT NOT NULL,
  port INTEGER NOT NULL,
  security TEXT NOT NULL,
  tls_name TEXT,
  auth TEXT NOT NULL,
  created_at INTEGER NOT NULL
);

CREATE INDEX servers_holder ON servers (holder);

-- An account reached through mail servers names one incoming and one outgoing server.
ALTER TABLE accounts ADD COLUMN incoming_server INTEGER;
ALTER TABLE accounts ADD COLUMN outgoing_server INTEGER;

-- Every account kept before servers were records names its servers in its settings: read once here as the servers
-- they are. The security is read as each connection was made: a certificate name of `-` is a plain connection;
-- else IMAP on 993 and submission on 465 speak TLS from the first byte, POP3 always does, and every other port is
-- upgraded with STARTTLS. A certificate name that is empty, `-` or the host itself is the host's.
CREATE TEMP TABLE named AS
SELECT
  id AS account,
  'owner:' || owner AS holder,
  incoming_protocol,
  incoming_host,
  incoming_port,
  CASE
    WHEN incoming_tls = '-' THEN 'none'
    WHEN incoming_protocol = 'pop3' OR incoming_port = 993 THEN 'tls'
    ELSE 'starttls'
  END AS incoming_security,
  CASE
    WHEN incoming_tls IS NULL OR incoming_tls IN ('', '-') OR incoming_tls = incoming_host THEN NULL
    ELSE incoming_tls
  END AS incoming_tls_name,
  outgoing_host,
  outgoing_port,
  CASE
    WHEN outgoing_tls = '-' THEN 'none'
    WHEN outgoing_port = 465 THEN 'tls'
    ELSE 'starttls'
  END AS outgoing_security,
  CASE
    WHEN outgoing_tls IS NULL OR outgoing_tls IN ('', '-') OR outgoing_tls = outgoing_host THEN NULL
    ELSE outgoing_tls
  END AS outgoing_tls_name
FROM (
  SELECT
    id,
    owner,
    CASE kind WHEN 'imap-smtp' THEN 'imap' ELSE 'pop3' END AS incoming_protocol,
    CASE kind WHEN 'imap-smtp' THEN json_extract(settings, '$.imapHost') ELSE json_extract(settings, '$.pop3Host') END
      AS incoming_host,
    CAST(CASE kind WHEN 'imap-smtp' THEN json_extract(settings, '$.imapPort') ELSE json_extract(settings, '$.pop3Port') END
      AS INTEGER) AS incoming_port,
    CASE kind WHEN 'imap-smtp' THEN json_extract(settings, '$.imapTlsName') ELSE json_extract(settings, '$.pop3TlsName') END
      AS incoming_tls,
    json_extract(settings, '$.smtpHost') AS outgoing_host,
    CAST(json_extract(settings, '$.smtpPort') AS INTEGER) AS outgoing_port,
    json_extract(settings, '$.smtpTlsName') AS outgoing_tls
  FROM accounts
  WHERE kind IN ('imap-smtp', 'pop3-smtp')
)
WHERE incoming_host IS NOT NULL AND outgoing_host IS NOT NULL;

-- One record for every set of identical settings an owner held.
INSERT INTO servers (holder, protocol, name, host, port, security, tls_name, auth, created_at)
SELECT DISTINCT holder, incoming_protocol, incoming_host, incoming_host, incoming_port, incoming_security,
  incoming_tls_name, 'password', CAST(strftime('%s', 'now') AS INTEGER) * 1000
FROM named;

INSERT INTO servers (holder, protocol, name, host, port, security, tls_name, auth, created_at)
SELECT DISTINCT holder, 'smtp', outgoing_host, outgoing_host, outgoing_port, outgoing_security, outgoing_tls_name,
  'password', CAST(strftime('%s', 'now') AS INTEGER) * 1000
FROM named;

UPDATE accounts
SET
  incoming_server = (
    SELECT MIN(servers.id) FROM servers JOIN named ON named.account = accounts.id
    WHERE servers.holder = named.holder AND servers.protocol = named.incoming_protocol
      AND servers.host = named.incoming_host AND servers.port = named.incoming_port
      AND servers.security = named.incoming_security AND servers.tls_name IS named.incoming_tls_name
  ),
  outgoing_server = (
    SELECT MIN(servers.id) FROM servers JOIN named ON named.account = accounts.id
    WHERE servers.holder = named.holder AND servers.protocol = 'smtp'
      AND servers.host = named.outgoing_host AND servers.port = named.outgoing_port
      AND servers.security = named.outgoing_security AND servers.tls_name IS named.outgoing_tls_name
  ),
  settings = json_remove(settings, '$.imapHost', '$.imapPort', '$.imapTlsName', '$.pop3Host', '$.pop3Port',
    '$.pop3TlsName', '$.smtpHost', '$.smtpPort', '$.smtpTlsName')
WHERE id IN (SELECT account FROM named);

DROP TABLE named;
