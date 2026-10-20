-- The event stream's window (docs/api.md, 「The event stream」): each event under its number, which only grows,
-- with the account it belongs to, kept for a day and at most 10,000.
CREATE TABLE events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  kind TEXT NOT NULL,
  account TEXT,
  at INTEGER NOT NULL,
  data TEXT NOT NULL
);

CREATE INDEX events_at ON events (at);

-- How long an account's send waits before it goes, while it can still be taken back.
ALTER TABLE settings ADD COLUMN undo_send_ms INTEGER NOT NULL DEFAULT 10000;
