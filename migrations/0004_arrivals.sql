-- The messages the watcher took that are still owed their message.created (docs/mail.md, 「Connections and the
-- watcher」): each by its account, folder, the UID validity it was taken under and its UID, let go once said.
CREATE TABLE arrivals (
  account TEXT NOT NULL,
  folder TEXT NOT NULL,
  uid_validity TEXT NOT NULL,
  uid INTEGER NOT NULL,
  PRIMARY KEY (account, folder, uid_validity, uid)
);
