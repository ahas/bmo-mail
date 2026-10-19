# What this repository takes from the old bmo

Every file, part of a file, table, setting, test and service unit this repository takes from the old
single repository, with where each came from, how many, and what becomes of it here. Every set below was
listed from the old repository at its commit `3e4c032`; the command that lists a set stands beside it.
Nothing here was copied with its history.

The tests are deferred: none is carried, written or run in this phase. They are written and run here once
the minimal services stand on the new repositories. The deferred tests are listed below so that work knows
what the old behaviour was held to.

## How the set was listed

- **By path:** `git ls-files` over `client/server/utils/mail/`, `client/server/api/mail/`,
  `client/app/components/Mail*`, `client/app/domain/mail*`, `client/app/pages/mail/`, and the single files
  `client/server/plugins/mail.ts`, `client/shared/mail.ts`, `client/scripts/mail-bench.ts`,
  `client/scripts/mail-bench.test.ts`, `client/browser/mail-body.test.ts` and `docs/mail.md`. That gives
  114 files.
- **By what they read:** every import of those files that leaves the set, and every language-pack key
  they read (`grep` over the 89 files that are not tests). That adds the parts of other files below.
- **So the set is 114 files.** 90 are carried or ported, 24 are not carried, each for the reason its table
  gives. Seventeen parts of other files are carried or listed beside them.
- **Earlier lists that disagree:** the old code graph's mail node names 121 files, counting the console's
  shared files the screen reads; those are listed here as parts of other files, not as files of this unit.

## How the old code is carried

- **The server is ported to Rust,** one crate split into modules, keeping what each old module did. The
  old server ran inside the console's Nuxt server; this service stands on its own, behind its own token
  (`docs/api.md`).
- **The screen is copied** as the console plugin bundle in `client/`, a Nuxt layer on bmo-ui, its imports
  and its routes re-pointed (`docs/api.md`, 「The console plugin bundle」).
- **`docs/mail.md` is carried** and stated for this repository: its settings, store and event stream in
  place of the console's environment, the archive's tables and the console's plugin.

## The server modules: 26 files, 3,852 lines, ported to Rust

`git ls-files client/server/utils/mail`, less the 13 tests.

| Old file | Lines | Here |
|---|---|---|
| `accounts.ts` | 156 | `src/accounts.rs` |
| `requests.ts` | 225 | `src/routes/requests.rs` |
| `session.ts` | 133 | `src/imap/session.rs` |
| `folders.ts` | 89 | `src/imap/folders.rs` |
| `structure.ts` | 96 | `src/imap/structure.rs` |
| `charset.ts` | 111 | `src/imap/charset.rs` |
| `messages.ts` | 593 | `src/imap/messages.rs` |
| `conversation.ts` | 37 | `src/imap/conversation.rs` |
| `search.ts` | 138 | `src/imap/search.rs` |
| `views.ts` | 126 | `src/imap/views.rs` |
| `moved-in.ts` | 70 | `src/imap/moved_in.rs` |
| `downloads.ts` | 23 | `src/routes/downloads.rs` |
| `labels.ts` | 110 | `src/labels.rs` |
| `compose.ts` | 303 | `src/writing/compose.rs` |
| `drafts.ts` | 134 | `src/writing/drafts.rs` |
| `staging.ts` | 107 | `src/writing/staging.rs` |
| `send.ts` | 191 | `src/writing/send.rs` |
| `receipts.ts` | 77 | `src/writing/receipts.rs` |
| `addresses.ts` | 35 | `src/writing/addresses.rs` |
| `rules.ts` | 322 | `src/rules.rs` |
| `settings.ts` | 21 | `src/account_settings.rs` |
| `avatars.ts` | 375 | `src/avatars/mod.rs` |
| `domains.ts` | 34 | `src/avatars/domains.rs` |
| `watcher.ts` | 271 | `src/watcher/mod.rs` |
| `arrivals.ts` | 41 | `src/watcher/arrivals.rs` |
| `notice.ts` | 34 | `src/watcher/notice.rs` |

## The routes: 36 files, 706 lines, ported to Rust

`git ls-files client/server/api/mail`, less `routes.test.ts`. Each answers at the same path behind the
bearer token in place of the console's sign-in (`docs/api.md`).

| Old files | Here |
|---|---|
| `accounts.get.ts`, `summary.get.ts` | `src/routes/accounts.rs` |
| `[account]/folders.get.ts`, `list.get.ts`, `thread.get.ts`, `message.get.ts`, `part.get.ts`, `cid.get.ts`, `read-all.post.ts` | `src/routes/lists.rs` |
| `[account]/flags.post.ts`, `move.post.ts`, `delete.post.ts` | `src/routes/changes.rs` |
| `[account]/labels.post.ts`, `labels/[id].patch.ts`, `labels/[id].delete.ts` | `src/routes/labels.rs` |
| `[account]/addresses.get.ts`, `draft.get.ts`, `draft.post.ts`, `draft.delete.ts`, `draft/start.post.ts`, `upload.post.ts`, `send.post.ts`, `send/[id].get.ts`, `send/[id].delete.ts`, `receipt.post.ts` | `src/routes/writing.rs` |
| `[account]/rules.get.ts`, `rules.post.ts`, `rules/[id].put.ts`, `rules/[id].delete.ts`, `rules/order.post.ts`, `rules/run.post.ts`, `spam-senders.post.ts`, `spam-senders.delete.ts`, `settings.get.ts`, `settings.post.ts` | `src/routes/filters.rs` |
| `avatar.get.ts` | `src/routes/avatar.rs` |

That is 2 + 7 + 3 + 3 + 10 + 10 + 1 = 36.

## What the server ran beside them: 2 files, 330 lines

| Old file | Lines | Here |
|---|---|---|
| `client/server/plugins/mail.ts` | 97 | `src/watcher/mod.rs`: the watchers started with the service, the accounts file read again every minute, the staging sweep every hour, everything closed on stop; the console's event word and push ring become the event stream (`docs/api.md`, 「The event stream」) |
| `client/shared/mail.ts` | 233 | `src/wire.rs`, the shapes the routes answer; and copied as `client/shared/mail.ts` for the screen |

## The console screen: 25 files, 4,835 lines, carried

Copied into `client/` at the same paths below `app/`.

| Folder | Files | Lines |
|---|---|---|
| `client/app/components/` | `MailAddressField.vue`, `MailAvatar.vue`, `MailBody.vue`, `MailComposeForm.vue`, `MailComposer.vue`, `MailEditor.vue`, `MailEditorSkeleton.vue`, `MailLabelChoice.vue`, `MailList.vue`, `MailMessageCard.vue`, `MailNav.vue`, `MailNavEntry.vue`, `MailRow.vue`, `MailSettingsBody.vue`, `MailThread.vue` (15) | 2,671 |
| `client/app/domain/` | `mail-api.ts`, `mail-body.ts`, `mail-editor-load.ts`, `mail-editor.ts`, `mail-fit.ts`, `mail-frame.ts`, `mail-notice.ts`, `mail-summary.ts`, `mail.ts` (9) | 1,227 |
| `client/app/pages/mail/` | `[...path].vue` (1) | 937 |

**What the screen reads from bmo-ui** (its `docs/boundary.md`), depended on rather than carried: the domain
modules `actionbar`, `dragselect`, `errors`, `form-save`, `island`, `layers`, `loading`, `numbers`, `screen`
and `side-parts`, and its components.

## The documents: 1 file

| Old file | Lines | Here |
|---|---|---|
| `docs/mail.md` | 528 | `docs/mail.md`, stated for this repository: no account, host, machine or address named; the screen's words named in English; the stand-in mail server deferred |

## Parts of other files: 17

| Old file | Part | Here |
|---|---|---|
| `client/server/utils/db.ts` | the seven `web_mail_*` tables, their index and `migrateMailImages` | `migrations/`, the store (「Stored state」) |
| `client/server/utils/push.ts` | `clipBody`, the notification body's clip | `src/watcher/notice.rs` |
| `client/shared/types.ts` | `MailSummary`, `MailAccountSummary`, `MailSummaryMessage` | `src/wire.rs`, and `client/shared/mail.ts` for the screen |
| `client/shared/types.ts` | `PushPayload` | the `push` of an arrival (`docs/api.md`, 「The event stream」) |
| `client/shared/config.ts` | `MAIL_PUSH` | its id, `mail`, is the arrival's `kind`; the kind's declaration and words are bmo-web's (`docs/api.md`, 「Missing in siblings」) |
| `client/shared/paths.ts` | `mailThreadPath` | the arrival's `path`, under the console's base path |
| `client/shared/links.ts` | `linkParts`, which the screen's frame reads | the bundle, on bmo-markup's `web-address` |
| `client/app/domain/settings.ts` | `useMailColumns`, the list's column widths kept in the browser | the bundle's own domain module, `mail-columns` |
| `client/app/domain/avatars.ts` | `identityMark` and the tones and initial it draws with, the letter an address wears without a logo | the bundle's own domain module, `mail-avatars` |
| `client/app/domain/urls.ts` | `appUrl`, a path under the console's base path, which a part's, an inline picture's and a logo's addresses are written with | the bundle's own domain module, `mail-urls` |
| `client/app/domain/events.ts` | `useMailEvents` | owed by the host (`docs/api.md`, 「The console plugin bundle」) |
| `client/app/domain/screens.ts` | the `/mail` navigation entry and its unread mark | the host's; it reads the mark from the bundle's `useMailNotices` |
| `client/app/layouts/default.vue` | the call of `useMailNotices` | the host's |
| `client/i18n/locales/ko.yaml` | the `mail` subtree, 292 keys: title 10, label 92, action 58, placeholder 9, aria 15, status 15, message 15, error 78 | `client/i18n/locales/ko.yaml`, the bundle's pack |
| `client/i18n/locales/ko.yaml` | `screens.label.mail`, `screens.aria.mail_unread` | the bundle's pack |
| `client/i18n/locales/ko.yaml` | 17 `common` keys the screen reads: `common.action.{add,cancel,close,collapse,expand,more,move_up,refresh,save,send}`, `common.error.{not_sent,save_failed}`, `common.label.{code,link,none,count,default}` | the bundle's pack |
| `client/i18n/locales/ko.yaml` | the 14 words the server wrote: `mail.title.arrival_one`, `arrival_many`, `receipt_subject`; `mail.message.arrival`, `receipt_body`; `mail.label.no_sender`, `no_subject`, `reply_attribution`, `forward_header`, `forward_from`, `forward_date`, `forward_subject`, `forward_to`, `forward_cc` | `wording/ko.toml`, the service's own words, under the keys `docs/api.md` names (「The service's own words」) |

The pack's words are 311 keys in all (292 + 2 + 17). The server's refusals, which were the pack's sentences,
are English sentences and codes here; the screen shows each by `mail.error.<code>`, one of the 78.

## Not carried: 24 files

### Tests deferred: 23 files, 2,779 lines

Each is written again here once tests are, against the Rust module or the screen file it held.

| Folder | Files | Lines |
|---|---|---|
| `client/server/utils/mail/` | `accounts.test.ts`, `avatars.test.ts`, `charset.test.ts`, `compose.test.ts`, `conversation.test.ts`, `domains.test.ts`, `notice.test.ts`, `rules.test.ts`, `search.test.ts`, `send.test.ts`, `staging.test.ts`, `store.test.ts`, `watcher.test.ts` (13) | 1,321 |
| `client/server/api/mail/` | `routes.test.ts` (1) | 462 |
| `client/app/components/` | `MailNavEntry.test.ts` (1) | 55 |
| `client/app/domain/` | `mail-editor-load.test.ts`, `mail-editor.test.ts`, `mail-frame.test.ts`, `mail-notice.test.ts`, `mail-summary.test.ts`, `mail.test.ts` (6) | 586 |
| `client/browser/` | `mail-body.test.ts` (1) | 259 |
| `client/scripts/` | `mail-bench.test.ts` (1) | 96 |

The Korean these tests hold — charset input, reply subjects, bodies — is input the tested code inherently
handles; where it is written again, it is written plainly and counted in `korean-ledger.json`.

### The stand-in mail server: 1 file, 429 lines

`client/scripts/mail-bench.ts` brought up the stand-in Dovecot container and the SMTP stand-in the old
development console and its tests used (`docs/mail.md`, 「A stand-in mail server」). It is deferred with
the tests, which are what it served.

### What the old console did for this unit

The console's sign-in on every route, its refusal helper, its build verdict and its data-root reading
stay the console's; this service checks its token and reads its settings (`docs/api.md`). The ordered
runner `@bmo/util/in-order` the server used is the service's own one-at-a-time handling here. The
verdict of which console rings (`ringsFromThisServer`, `BMO_PUSH_IN_DEV`) is the console's: the service
hands every arrival on its stream. `docs/web.md`'s 「Mail screen」 section says how the screen sits in the
console and stays bmo-web's; the stand-in lines of `docs/worker-bench.md` stay with the old bench.

## Stored state: 7 tables

| Old table | Rows | Here |
|---|---|---|
| `web_mail_labels` | 0 | `labels` |
| `web_mail_rules` | 0 | `rules` |
| `web_mail_settings` | 0 | `settings` |
| `web_mail_spam_senders` | 0 | `spam_senders` |
| `web_mail_marks` | 0 | `marks` |
| `web_mail_addresses` | 5 | `addresses` |
| `web_mail_avatars` | 44 | `avatars` |

- **Columns** are the ones the old server made and read (`docs/api.md`, 「The store」). The `user_id`
  column the archive's user migration added to six of them is not carried: no mail code reads or writes it.
- **`mail_forms`** is not carried: no migration defines it, no code reads it, and it holds 0 rows.
- **The rows themselves** move with the switch-over's data move, not here.

## Secrets and settings: 4

| What | Old place | Here |
|---|---|---|
| the accounts file | `BMO_MAIL_ACCOUNTS_FILE`, else the data root's `env/mail-accounts` for a built console | the setting `accounts.file`, which a deployment names; the account addresses, hosts and logins stand only in that file |
| the staging folder | `mail-staging` beside the archive file | the setting `staging.dir` |
| the language of the words the server wrote and of a list's order | Korean, fixed in the code | the setting `wording.language` |
| the time zone dates were written in | fixed in the code | the setting `wording.time_zone` |

## Service units: 1

| Unit | Old place | Here |
|---|---|---|
| `bmo-mail.service` | none: the console server's plugin held the watchers and answered the routes | `deploy/bmo-mail.service`, the service itself |

## Crates and packages

- **No crate or package of the old workspace is depended on,** and no `bmo-*` package of another unit. The
  screen depends on bmo-ui and bmo-markup among the base libraries.
- **What the old server's libraries become:** `imapflow` an IMAP crate, `nodemailer` and its composer and
  connection a submission crate and a message builder, `libmime` and `postal-mime` a MIME parser,
  `htmlparser2` and `domhandler` an HTML parser, and `node:dns` a DNS resolver; each crate is named in the
  manifest. The list of registry tails `domains.ts` kept is the public suffix list, read through a crate.
- **What the screen carries:** `dompurify` and the TipTap editor packages `@tiptap/vue-3`,
  `@tiptap/extension-text-style`, `@tiptap/extension-text-align` and `@tiptap/extension-table`, the
  sanitiser at the version the old console pinned and the editor packages at the one the bundle's Nuxt UI
  builds its own editor on, so the two read one editor type.
- **Policy files:** none. Nothing in the set reads `policy/`.
