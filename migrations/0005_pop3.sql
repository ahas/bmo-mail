-- Each message a POP3 account's maildrop holds (docs/mail.md, 「A POP3 account」): known by its UIDL, given a
-- number of its own that no other message of any account is ever given again, which is its UID; when it
-- arrived, its size, its head, the flags and label keywords the service keeps for it, and its parts and snippet
-- once it was read whole.
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

-- The drafts of an account whose server keeps none, a POP3 account's: each the whole message as it was last
-- saved, under the Message-ID it keeps across every save, with the flags the service keeps for it.
CREATE TABLE drafts (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  account TEXT NOT NULL,
  message_id TEXT NOT NULL,
  message BLOB NOT NULL,
  flags TEXT NOT NULL,
  changed_at INTEGER NOT NULL,
  UNIQUE (account, message_id)
);
