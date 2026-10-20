//! The service's mail read into the members the routes answer (`docs/api.md`, 「What a message is」): a
//! message, a folder, a rule and the settings, under the account they belong to.

use crate::keys::{message_id, thread_id};
use crate::mail::{MailAddress, MailFolderState, MailMessage, MailPart, MailRef, MailRule, MailSettings};
use crate::wire::{Folder, Mailbox, Message, Part, PartKind, Rule, RuleMatching, Settings};

/// A message ID as a member carries it: without its angle brackets.
pub fn bare_id(id: &str) -> String {
  id.trim().trim_start_matches('<').trim_end_matches('>').to_owned()
}

/// A name and an address, the name null where none is given.
pub fn mailbox(address: &MailAddress) -> Mailbox {
  Mailbox {
    name: Some(address.name.trim().to_owned()).filter(|name| !name.is_empty()),
    address: address.address.clone(),
  }
}

fn mailboxes(addresses: &[MailAddress]) -> Vec<Mailbox> {
  addresses.iter().map(mailbox).collect()
}

/// A mailbox as the service writes it into a message.
pub fn address_of(mailbox: &Mailbox) -> MailAddress {
  MailAddress {
    name: mailbox.name.clone().unwrap_or_default().trim().to_owned(),
    address: mailbox.address.trim().to_lowercase(),
  }
}

fn part(part: &MailPart, kind: PartKind) -> Part {
  Part {
    part_id: part.part.clone(),
    kind,
    media_type: part.kind.clone(),
    charset: part.charset.clone(),
    encoding: part.encoding.clone(),
    file_name: part.filename.clone(),
    size: part.size,
    content_id: part.cid.clone(),
  }
}

/// A message as a member, under the account it is in.
pub fn message(account: &str, mail: MailMessage) -> Message {
  let listed = mail.listed;
  let at = MailRef {
    folder: listed.folder.clone(),
    uid: listed.uid,
  };
  let parts = mail
    .text_part
    .iter()
    .map(|one| part(one, PartKind::Text))
    .chain(mail.html_part.iter().map(|one| part(one, PartKind::Html)))
    .chain(mail.attachments.iter().map(|one| part(one, PartKind::Attachment)))
    .chain(mail.inline.iter().map(|one| part(one, PartKind::Inline)))
    .collect();

  Message {
    id: message_id(&at),
    account_name: account.to_owned(),
    thread_id: thread_id(&listed.key),
    folder_name: listed.folder,
    internet_message_id: listed.message_id.as_deref().map(bare_id),
    in_reply_to: mail.in_reply_to.as_deref().map(bare_id),
    references: mail.references.iter().map(|id| bare_id(id)).collect(),
    subject: listed.subject,
    from: listed.from.first().map(mailbox),
    to: mailboxes(&listed.to),
    cc: mailboxes(&mail.cc),
    bcc: mailboxes(&mail.bcc),
    reply_to: mailboxes(&mail.reply_to),
    at: listed.date,
    flags: listed.flags,
    label_ids: listed.labels,
    receipt_to: mail.receipt_to,
    snippet: listed.snippet,
    text: mail.text,
    html: mail.html,
    parts,
  }
}

/// A folder with its counts as a member, at its place among the folders.
pub fn folder(account: &str, index: usize, state: MailFolderState) -> Folder {
  Folder {
    account_name: account.to_owned(),
    name: state.folder.path,
    short_name: state.folder.name,
    role: state.folder.role,
    position: index as i64 + 1,
    unread_count: state.unseen,
    total_count: state.total,
  }
}

/// A rule as a member.
pub fn rule(rule: MailRule) -> Rule {
  Rule {
    id: rule.id,
    position: rule.position,
    matching: RuleMatching {
      kind: rule.rule.matching,
      conditions: rule.rule.conditions,
    },
    actions: rule.rule.actions,
    on: rule.rule.enabled,
  }
}

/// An account's settings as a member, with its always-spam senders.
pub fn settings(settings: MailSettings, spam_senders: Vec<String>) -> Settings {
  Settings {
    signature: settings.signature,
    undo_send_ms: settings.undo_send_ms,
    read_receipts: settings.receipt_default,
    show_images: settings.show_images,
    spam_senders,
  }
}
