# bmo-mail

Mail as one service with its own API: the mailboxes an accounts file names, read, written and watched over
IMAP and submission. It threads and orders lists itself, runs filter rules as mail arrives, finds sender
logos, holds a sent message for a short while so it can be taken back, and says on its own event stream when
an inbox changed and what arrived. It calls no other unit; the console in front of it hands the mail screen's
requests on, relays the event stream and rings the push, as `docs/api.md` states.

## Tests are deferred

No test is carried, written or run in this repository yet. The tests are written and run here once the
minimal services stand on the new repositories; until then the only check is the build check, `cargo check
--all-targets` and the bundle's typecheck. `docs/inventory.md` lists the tests the old behaviour was held to.

## Layout

| Folder | Holds |
|---|---|
| `src/` | The service and its command, one crate split into its own modules |
| `migrations/` | The store, its tables made by these statements in order |
| `wording/` | The service's own words, one file to a language |
| `client/` | The console plugin bundle: the mail screen, a project of its own |
| `deploy/` | The service's user unit |
| `docs/` | The canonical statements of what the service does |

At the root stand the Cargo manifest, `VERSION`, `testbed.toml`, `rustfmt.toml`, `CLAUDE.md`,
`korean-ledger.json`, `settings.toml` and `settings.local.toml.example`, the example of the local settings a
deployment gives.

## Documents

| Document | States |
|---|---|
| `docs/mail.md` | What mail does: the accounts, the watcher, reading, writing, filters, avatars and the screen |
| `docs/api.md` | The routes, the event stream, the service's own words, the store, the command, the settings, the console plugin bundle and what the console does |
| `docs/inventory.md` | What this repository takes from the old repository, with each set's count and source |

## Version

`VERSION` holds the repository's version in semver, the crate's own. `docs/api.md`, 「Versions」, says which
change moves which part.
