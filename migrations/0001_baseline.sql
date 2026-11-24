-- The mail service's store at 1.0.0, whole (docs/api.md, 「The store」).

CREATE TABLE labels (
  account TEXT NOT NULL,
  id TEXT NOT NULL,
  name TEXT NOT NULL,
  color TEXT NOT NULL,
  position INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  PRIMARY KEY (account, id)
);

CREATE TABLE rules (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  account TEXT NOT NULL,
  position INTEGER NOT NULL,
  enabled INTEGER NOT NULL,
  match TEXT NOT NULL,
  conditions TEXT NOT NULL,
  actions TEXT NOT NULL,
  created_at INTEGER NOT NULL
);

CREATE INDEX rules_account ON rules (account, position);

CREATE TABLE settings (
  account TEXT PRIMARY KEY,
  signature TEXT NOT NULL,
  receipt_default INTEGER NOT NULL,
  show_images INTEGER NOT NULL DEFAULT 0
, undo_send_ms INTEGER NOT NULL DEFAULT 10000);

CREATE TABLE spam_senders (
  account TEXT NOT NULL,
  address TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  PRIMARY KEY (account, address)
);

CREATE TABLE marks (
  account TEXT NOT NULL,
  folder TEXT NOT NULL,
  uid_validity TEXT NOT NULL,
  last_uid INTEGER NOT NULL,
  PRIMARY KEY (account, folder)
);

CREATE TABLE addresses (
  account TEXT NOT NULL,
  address TEXT NOT NULL,
  name TEXT NOT NULL,
  last_used INTEGER NOT NULL,
  uses INTEGER NOT NULL,
  PRIMARY KEY (account, address)
);

CREATE TABLE avatars (
  domain TEXT PRIMARY KEY,
  type TEXT NOT NULL,
  bytes BLOB,
  looked_up_at INTEGER NOT NULL
);

CREATE TABLE events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  kind TEXT NOT NULL,
  account TEXT,
  at INTEGER NOT NULL,
  data TEXT NOT NULL
, owner TEXT);

CREATE INDEX events_at ON events (at);

CREATE TABLE accounts (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  owner TEXT NOT NULL,
  name TEXT NOT NULL,
  kind TEXT NOT NULL,
  address TEXT NOT NULL,
  position INTEGER NOT NULL,
  settings TEXT NOT NULL,
  secret BLOB NOT NULL,
  created_at INTEGER NOT NULL, incoming_server INTEGER, outgoing_server INTEGER,
  UNIQUE (owner, name)
);

CREATE TABLE imports (
  source TEXT PRIMARY KEY,
  owner TEXT NOT NULL,
  count INTEGER NOT NULL,
  at INTEGER NOT NULL
);

CREATE INDEX events_owner ON events (owner, id);

CREATE TABLE arrivals (
  account TEXT NOT NULL,
  folder TEXT NOT NULL,
  uid_validity TEXT NOT NULL,
  uid INTEGER NOT NULL,
  PRIMARY KEY (account, folder, uid_validity, uid)
);

CREATE TABLE pop3_messages (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  account TEXT NOT NULL,
  uidl TEXT NOT NULL,
  at INTEGER NOT NULL,
  size INTEGER NOT NULL,
  head BLOB NOT NULL,
  flags TEXT NOT NULL,
  parts TEXT,
  snippet TEXT,
  created_at INTEGER NOT NULL,
  UNIQUE (account, uidl)
);

CREATE TABLE drafts (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  account TEXT NOT NULL,
  message_id TEXT NOT NULL,
  message BLOB NOT NULL,
  flags TEXT NOT NULL,
  changed_at INTEGER NOT NULL,
  UNIQUE (account, message_id)
);

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

