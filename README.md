**English** | [한국어](README.ko.md) | [日本語](README.ja.md) | [简体中文](README.zh-CN.md)

# bmo-mail

Mail as one service with its own API: each login account's own mailboxes, added from the screen, read, written
and watched through an adapter for each kind of account, IMAP and submission first. It threads and pages lists
itself, runs filter rules as mail arrives, finds sender logos, holds a sent message for the account's undo
span so it can be taken back, and says on its own event stream what came and what changed. It calls no other
service. Its hosted tool, `bmo/mail_sent`, reads what a person sent on a day from the Outlook
on their own PC, else from Microsoft Graph with a token an optional page-token service lends for them, through agency,
the receiver, the account service, the node service and that page-token service. A coding harness takes it through its MCP server, `bmo-mail mcp`, and the `bmo-mail` skill; a console
takes its screen through the bundle in `client/`, which the service serves with its manifest, as `docs/api.md`
states.

## Tests

The testbed's `personal-data` check holds the personal-data rule (`PERSONAL-DATA.md`). The repository
carries the old archive's import in `tests/import.rs`, the accounts in `tests/accounts.rs`:
their routes, one owner key never reaching another's, the sealing of their secrets and the accounts file's
import, the watcher over a dropped connection in `tests/watcher.rs`, a POP3 account against stand-in POP3 and
submission servers in `tests/pop3.rs`: its inbox listed and read with an attachment, new mail told by `UIDL`, and
a reply kept as a draft in the store and sent, the mail servers in `tests/servers.rs`: the accounts kept before servers
were records moved onto one record per set of identical settings, the servers an owner reaches against a stand-in
account service's delegation chain, an account choosing among them, and the account service's system send against a
stand-in submission server, the console face in `tests/console.rs`: the
served manifest read through the vendored bmo-ui release's own reading and how far the console's credential and
the service's token reach, and the screen's words in `tests/words.rs`: every key the screen writes is its own
pack's or one the release's `wording-keys` module lists, and the hosted tool in `tests/sent_mail.rs`, called as
the tool host calls it against stand-ins of agency, the node service, the page-token service and Graph: whose person and which
machine, the two roads in their order, the token each upstream is asked with, the refusals, and the schemas its
folder keeps. `cargo test` runs them all. The console plugin bundle
carries its browser tests in `client/tests/`: the screen stands on its own in a small Nuxt host there, served by
the client's dev server and driven with the mouse in Chromium against a stand-in of the service that keeps what
the screen changes, and `tests/browser/mail-drag.test.ts` carries a list row onto a folder and a label of the mail
nav and a filter rule to another place, reading each result from a page loaded again, and
`tests/browser/mail-kind-actions.test.ts` holds an account whose kind moves no mail to a screen that draws nothing
that would, at a wide window, a narrow one and a phone's width, and `tests/browser/mail-servers.test.ts` keeps a
server of the person's own with its connection test and adds an account on servers chosen from the selects, a new
server among them, at a wide window and a phone's width. The bundle's check runs
them after its typecheck. The other behaviour tests are deferred: they are written and run here once
the minimal services stand on the new repositories, and `docs/inventory.md` lists what the old behaviour was
held to. The build check is `cargo check --all-targets` and the bundle's typecheck.

## Layout

| Folder | Holds |
|---|---|
| `src/` | The service, its client, its OpenAPI document and its MCP server, with its command, one crate split into its own modules |
| `examples/` | `openapi`, which prints the OpenAPI document as `docs/openapi.json` keeps it, and `tool_schemas`, which writes the hosted tool's schemas into its folder |
| `migrations/` | The store, its tables made by these statements in order |
| `wording/` | The service's own words, one file to a language |
| `plugin/` | The harness plugin's skill, `skills/bmo-mail/` |
| `tools/` | The hosted tool, `sent-mail/`: its manifest, its schemas and its manual |
| `client/` | The console plugin bundle: the mail screen, a project of its own |
| `deploy/` | The service's user unit |
| `docs/` | The canonical statements of what the service does, and the OpenAPI document |
| `tests/` | The behaviour tests that stand |

At the root stand the Cargo manifest, `VERSION`, `LICENSE`, `testbed.toml`, `install.toml`, `rustfmt.toml`, `CLAUDE.md`,
`PERSONAL-DATA.md`, `korean-ledger.json`, `settings.toml` and `settings.local.toml.example`, the example of the
local settings a deployment gives.

## The console plugin bundle

`client/` holds the mail screen as a console bundle on bmo-ui. It keeps bmo-ui's console plugin contract
(bmo-ui's `docs/console-bundles.md`) through bmo-chat's console face (`docs/api.md`, 「The console plugin
bundle」): `bun run bundle` builds the module, the stylesheet, the words and the layer it was built on with
bmo-ui's bundle build, from `client/console/`, and the service serves them beside the manifest its declaration
makes. The bmo-ui release bmo-chat pins and a pack of bmo-text's TypeScript package stand under `client/vendor/`
for its own install, typecheck and build, so a clone needs no other checkout. Its bodies are typed by
`client/shared/openapi.ts`, which `bun run openapi` generates from `docs/openapi.json`. The bundle's check is
`bun run check` inside `client/`: its frozen install, its typecheck and its browser tests.

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
