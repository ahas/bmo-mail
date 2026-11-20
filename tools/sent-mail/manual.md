# bmo/mail_sent

Reads the mail your own person sent on one day, on their own PC, and answers each message's subject, its
recipients and when it was sent. It never reads a body or an attachment, and it reads nobody else's mail: the
person is the one your account belongs to, and the machine is the node agency binds to that account alone.

## Arguments

- `day`: the date to read, `YYYY-MM-DD`, in the service's time zone. Leave it out for today.

## How it reads

1. The classic Outlook already running on the person's PC, attached to and never started.
2. Where Outlook is not running or cannot be attached to: Microsoft Graph, with the token the person's own
   Teams page on that PC holds.

The answer's `road` says which one answered.

## Answer

`day`, `road` (`outlook` or `graph`), `node`, and `messages` in the order they were sent, each `subject`,
`sentAt` (epoch milliseconds) and `recipients`, each `name`, `address` and `kind` (`to`, `cc` or `bcc`).

## Refusals

- Your session acts for no account, or the account has no machine of its own (none bound, or one shared with
  other accounts).
- Neither road stood: Outlook is not running and the Teams page is not signed in or not standing. The refusal
  says why for each. The person signs in to Outlook or to the Teams page on their own PC.

## Example

    bmo use bmo/mail_sent --arg day=2026-11-20
