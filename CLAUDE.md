# Working in bmo-mail

## The three standing principles

These hold every bmo repository, and every change here:

(a) each service runs on its own, with no context bleeding in from other repositories: it builds, tests and
runs from its own repository, data and settings, and never reads another repository's code, files, data
folders or tables; (b) services talk to each other only through documented APIs (HTTP routes, events, a
documented command), never by importing another service's code; (c) references run one way: lower code uses
the base, and the base never knows what is below it (no per-provider branch such as `if telegram`, no list of
the units below, no dependency on any service); where two repositories must know the same policy value, the
lowest service in the dependency direction owns it and the ones above read it through that service's API;
and each repository holds only what it uses itself, since the same thing kept in several places is the wrong
structure (archive document #301, 「원칙」).

## The documents come first

- `docs/` is the canonical statement of what the service does; the code is one realisation of it. Before
  changing code, read the document covering that area: `docs/mail.md` for what mail does and where its state
  lives, `docs/api.md` for the routes, the event stream, the service's own words, the store, the command, the
  settings and the console plugin bundle, `docs/inventory.md` for what was taken from the old repository.
- A change that would make a document wrong or incomplete repairs the document first, stating what is true
  now, and the code is written to match.
- A document states what is true now: no change history, no rationale told through an earlier design, no
  pasted source, signature or config fragment, and no file-and-line pins.

## Boundaries

- **The service calls no other unit.** The console in front of the service checks the sign-in, hands the
  screen's requests on, relays the event stream and rings the push; the service checks its bearer token and
  listens on a loopback address (`docs/api.md`, 「Who checks what」). The hosted tool alone calls agency, the
  receiver, the account service, the node service and the Teams page service (`docs/api.md`, 「The hosted tool」). Every other unit is reached through its
  API, never through its code, its tables or its files.
- **A client of another service is one module named for that service** under `src/clients/`, holding only
  the calls this repository makes, so it can be swapped for that service's own client.
- **Dependencies:** crates from the registry pinned in `Cargo.toml`, and among other repositories only the
  base libraries `bmo-chat`, `bmo-chat-client`, bmo-chat's `bmo-console-face`, `bmo-ui` and `bmo-text`.
- **The plumbing knows no route.** A route registers itself beside its own answer; adding one changes no
  file but its own.
- **The accounts file is a secret.** Nothing read from it but an account's address reaches a log line or an
  answer (`docs/mail.md`, 「Accounts and credentials」).
- **No lookup table standing in for polymorphism.** A value that differs per kind of view, action or event
  belongs to that kind.
- **No hand-written parser for a general format.** TOML, JSON, MIME, IMAP responses, HTML, DNS records,
  HTTP headers and command lines are read by a maintained library. The accounts file's line syntax is this
  service's own and is read here.

## Language

- Everything in this repository is English: code, comments, strings, tests, documents, commit messages and
  branch names. The exceptions are the service's own words under `wording/` and the screen's language pack
  under `client/i18n/locales/`, a file to a language, each in its own.
- **Hangul the code inherently handles** is written plainly, never as an escape or a character code, and
  each code file's count of lines holding it stands in `korean-ledger.json`; the wording files and the language
  pack are no code files and stand outside it. No other Korean stands in code or tests.
- What the service answers is an English sentence for a log and a snake_case code a screen shows in its own
  language's words (`docs/api.md`, 「Refusals」). The screen's words stand in the bundle's own language pack,
  one key for every code (`docs/api.md`, 「The console plugin bundle」).
- No personal data: no user names, home directories, machine names, internal addresses, mail addresses or
  names of any agent (`PERSONAL-DATA.md`). What the service needs at runtime comes from its settings and the
  accounts file, never from the code.

## Layout

- `Cargo.toml` is the only manifest at the root, one crate split into its own modules, and no directory is
  named after its parent.
- The console plugin bundle stands in `client/`, a project of its own.
- Tests live in `tests/`, one test target, `tests/main.rs`, each file of tests a module of it (bmo-chat's
  `docs/conventions.md`, 「Rust builds」); no test module stands in `src/`. The repository carries the
  behaviour tests standing so far there (README.md); the testbed's `personal-data` check holds the
  personal-data rule.

## The stages of a change

1. **Writing:** the only check is `cargo check --all-targets`, and for the bundle its typecheck.
2. **Review:** read the whole diff against the document governing it, then `cargo fmt --all --check` and
   `cargo clippy --all-targets -- -D warnings`.
3. **Verification:** the tests touched, then the whole suite once, output written to a file and the file
   read. A module's tests run by its name filter, `cargo test --test main <module>::`; the whole suite is
   `cargo test` with the bundle's check.
