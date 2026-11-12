# bmo-mail

Mail as one service with its own API: each login account's own mailboxes, added from the screen, read, written
and watched through an adapter for each kind of account, IMAP and submission first. It threads and pages lists
itself, runs filter rules as mail arrives, finds sender logos, holds a sent message for the account's undo
span so it can be taken back, and says on its own event stream what came and what changed. It calls no other
service. A coding harness takes it through its MCP server, `bmo-mail mcp`, and the `bmo-mail` skill; a console
takes its screen through the bundle in `client/`, which the service serves with its manifest, as `docs/api.md`
states.

## Tests

The repository carries the repository-wide personal-data check in `tests/personal_data.rs`
(`PERSONAL-DATA.md`), the old archive's import in `tests/import.rs`, the accounts in `tests/accounts.rs`:
their routes, one owner key never reaching another's, the sealing of their secrets and the accounts file's
import, and the console plugin bundle in `tests/console.rs`: the manifest read through bmo-ui's own reading,
the guard of the screen's routes and the module's name following its bytes. `cargo test` runs them all. The other behaviour tests are deferred: they are written and run here once
the minimal services stand on the new repositories, and `docs/inventory.md` lists what the old behaviour was
held to. The build check is `cargo check --all-targets` and the bundle's typecheck.

## Layout

| Folder | Holds |
|---|---|
| `src/` | The service, its client, its OpenAPI document and its MCP server, with its command, one crate split into its own modules |
| `examples/` | `openapi`, which prints the OpenAPI document as `docs/openapi.json` keeps it |
| `migrations/` | The store, its tables made by these statements in order |
| `wording/` | The service's own words, one file to a language |
| `plugin/` | The harness plugin's skill, `skills/bmo-mail/` |
| `client/` | The console plugin bundle: the mail screen, a project of its own |
| `deploy/` | The service's user unit |
| `docs/` | The canonical statements of what the service does, and the OpenAPI document |
| `tests/` | The repository-wide personal-data check, `personal_data.rs`, and the behaviour tests that stand |
| `.graph/` | The code graph: its settings and its nodes, one feature to a node |

At the root stand the Cargo manifest, `VERSION`, `LICENSE`, `testbed.toml`, `rustfmt.toml`, `CLAUDE.md`,
`PERSONAL-DATA.md`, `korean-ledger.json`, `settings.toml` and `settings.local.toml.example`, the example of the
local settings a deployment gives.

## The console plugin bundle

`client/` holds the mail screen as a console bundle on bmo-ui. It keeps bmo-ui's console plugin contract
(bmo-ui's `docs/console-bundles.md`) through bmo-chat's console face (`docs/api.md`, 「The console plugin
bundle」): `bun run bundle` builds the module, the stylesheet, the words and the layer it was built on with
bmo-ui's bundle build, from `client/console/`, and the service serves them beside the manifest its declaration
makes. The bmo-ui release bmo-chat pins stands under `client/vendor/` for its own install, typecheck and build,
and bmo-text's TypeScript package is read from the base library. Its bodies are typed by
`client/shared/openapi.ts`, which `bun run openapi` generates from `docs/openapi.json`. The bundle's check is
`bun install --frozen-lockfile && bun run typecheck` inside `client/`.

## Documents

| Document | States |
|---|---|
| `docs/mail.md` | What mail does: the accounts, the watcher, reading, writing, filters, avatars and the screen |
| `docs/api.md` | The routes, the event stream, the client, the harness plugin, the service's own words, the store, the command, the settings and the console plugin bundle |
| `docs/openapi.json` | The OpenAPI 3.1 contract, generated from the service's own types |
| `docs/inventory.md` | What this repository takes from the old repository, with each set's count and source |
| `PERSONAL-DATA.md` | What may never stand in this repository: a person's name, address, path, device or network details, a room member's, a company's, or a line of an actual conversation, and the check that holds it |

## Version

`VERSION` holds the repository's version, the crate's own, which the health, the OpenAPI document, the MCP
server and the bundle answer. It moves only at a release the owner declares, as bmo-chat's
`docs/conventions.md`, 「Versions」, states: a commit, a deploy or a contract change moves none, and a
deployment is told apart by its commit.
