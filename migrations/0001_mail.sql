-- The mail service's own state (docs/mail.md, 「Where each piece of state lives」), each row an account's
-- by its address but for a sender logo, which is a domain's. Messages themselves stay on the mail server;
-- only what the server has no place for is kept here.

CREATE TABLE labels (
  account TEXT NOT NULL,
  id TEXT NOT NULL,
  name TEXT NOT NULL,
  color TEXT NOT NULL,
  position INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  PRIMARY KEY (account, id)
);

-- A rule's conditions and actions are JSON the rule engine reads; the match says whether all of the
-- conditions must hold or any one.
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
);

CREATE TABLE spam_senders (
  account TEXT NOT NULL,
  address TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  PRIMARY KEY (account, address)
);

-- The watcher's high-water mark: the highest UID it has handled in a folder, valid only against the UID
-- validity it was taken under.
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

-- A sender domain's own logo as the service found it, so a list of rows is not a lookup per row and a
-- domain that has none is not asked again for a while. The bytes are null where nothing was found, and the
-- row is the record that the looking happened.
CREATE TABLE avatars (
  domain TEXT PRIMARY KEY,
  type TEXT NOT NULL,
  bytes BLOB,
  looked_up_at INTEGER NOT NULL
);
