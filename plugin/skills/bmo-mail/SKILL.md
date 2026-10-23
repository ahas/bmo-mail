---
name: bmo-mail
description: Read, write and send mail through the bmo-mail MCP server - list an account's messages, read a message or a whole conversation with its bodies, change flags, labels and folders, write a draft and send it. Use when asked to look through a mailbox, find or read a mail, answer or forward one, tidy messages into folders or labels, or send a message from one of the service's accounts.
---

# bmo-mail

The mail service holds each owner's own mailboxes and serves them over one API: accounts, folders,
messages and conversations, drafts and sends, labels, rules and settings. The `bmo-mail` MCP server gives a
harness one tool for each route a reader and a writer need, each a thin call through the service's client.

## Starting the server

Register the server with the harness as a stdio MCP server:

```
bmo-mail mcp
```

It reaches the service at the address `--address` or `BMO_MAIL_ADDRESS` names, else at `http://` and the
service's own `server.listen` setting, with the bearer token `--token` or `BMO_MAIL_TOKEN` gives, else the
service's `server.token` setting. A wrong token answers every tool with a 401 `unauthorized` error. It acts for
the owner key `--owner` or `BMO_MAIL_OWNER` gives, else the service's `accounts.administrator` setting, and
sees that owner's accounts alone. It holds the role `--role` or `BMO_MAIL_ROLE` names, `administrator` or
`member`, else `administrator`; either reaches every tool.

## Reading

1. `mail_accounts`: the accounts, each with its `name` (every other tool takes it as `accountName`), its
   `address` and its inbox's `unreadCount`.
2. `mail_messages` with `accountName`: a page of messages newest first, without their bodies, each with its
   `id`, `threadId`, `folderName`, `from`, `subject`, `snippet`, `flags`, `labelIds` and `at`.
   - With nothing else it reads the inbox; `folderName` reads a folder, `labelId` a label's messages.
   - `q` takes search words: `from:`, `to:`, `subject:`, `label:`, `in:` (a folder or `spam`, `trash`,
     `sent`, `drafts`, `starred`), `is:unread`, `is:read`, `is:starred`, `has:attachment`, `before:` and
     `after:` with a date such as `2026/10/01`, and plain words of the text.
   - `more` true means older messages stand: ask again with `before` set to the last message's `at`.
3. `mail_message` with a message's `id` for its plain-text and HTML bodies (`text`, `html`) and its `parts`;
   `mail_thread` with its `threadId` for the whole conversation, oldest first, each message with its bodies.

Reading marks nothing read.

## Changing a message

`mail_update` takes the whole of what the message should carry: `flags` among `\Seen`, `\Flagged`,
`\Answered`, `$Forwarded`, `$Junk`, `$NotJunk` and `$MDNSent`, and `labelIds`. Start from the message's own
`flags` and `labelIds` and add or take away; a flag left out is taken off. `folderName` moves it, and the
answer is the message under its new `id` — an id names a message in one folder only.

- Mark read: add `\Seen`. Star: add `\Flagged`.
- Spam: `folderName` the junk folder, `$Junk` added and `$NotJunk` taken off; not spam the other way round
  into `INBOX`.
- Trash: `folderName` the trash folder.

## Writing and sending

1. `mail_draft` with `accountName` starts a draft and saves it in the drafts folder at once: a new message,
   or with `replyTo` (a message's `id`, and `replyAll` true to answer every recipient) a reply quoting it,
   or with `forward` a forward carrying it and its attachments. The answer's `id` names the draft across
   every save.
2. `mail_draft_update` with the draft's `id` changes it: `to`, `cc` and `bcc` as lists of `{ "name",
   "address" }` (the name may be left out), `subject`, `body` as HTML (a plain paragraph is
   `<p>text</p>`), `attachments` to keep by `partId`, `receiptRequested`. Each field given replaces the
   draft's own; a reply's body already holds the quoted original under the signature, so read it first and
   write above it.
3. `mail_send` with the draft's `id` sends it. It waits in the service for the account's undo span (ten
   seconds unless the account says otherwise), or until `sendAt` within that span, and then goes; the answer
   is a send in state `waiting`. The draft is deleted once the message has gone and its copy is in the sent
   folder.

Send only what the person asked to be sent, to the recipients they named.

## Failures

A refusal is a tool error whose text is JSON: `status`, `code` and `error` as the service answered them.
`not-found` means the key names nothing (an id from an older answer, after a move); `flag-not-settable`,
`unknown-label`, `unknown-folder`, `no-recipients` and `send-too-late` name the rule the request broke;
`upstream-failed` means the mail server did not answer, and asking again later is the remedy.
