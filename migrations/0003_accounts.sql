-- The accounts (docs/mail.md, 「Accounts and credentials」): each one an owner's, named uniquely among that
-- owner's accounts, of the kind whose adapter reaches its mailbox. The settings are the kind's fields that are
-- not secret, as JSON; the secret is the kind's secret fields as JSON, sealed with the key file's key and the
-- account's id, so it reads nowhere but here.
CREATE TABLE accounts (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  owner TEXT NOT NULL,
  name TEXT NOT NULL,
  kind TEXT NOT NULL,
  address TEXT NOT NULL,
  position INTEGER NOT NULL,
  settings TEXT NOT NULL,
  secret BLOB NOT NULL,
  created_at INTEGER NOT NULL,
  UNIQUE (owner, name)
);

-- The record that a source of accounts was imported, once, and for which owner key: the accounts file's
-- import is recorded here and never runs again.
CREATE TABLE imports (
  source TEXT PRIMARY KEY,
  owner TEXT NOT NULL,
  count INTEGER NOT NULL,
  at INTEGER NOT NULL
);

-- An event is its owner's: a reader reads the events of the owner key it carries alone.
ALTER TABLE events ADD COLUMN owner TEXT;

CREATE INDEX events_owner ON events (owner, id);
