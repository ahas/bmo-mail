//! A POP3 account's mailbox (`docs/mail.md`, 「A POP3 account」): the maildrop read one session at a time, what
//! POP3 does not hold kept in the store under the account, its drafts in a drafts folder of the service's own,
//! and a watcher that polls.

pub mod drafts;
pub mod maildrop;
pub mod mime;
pub mod search;
pub mod session;
pub mod watcher;

/// The inbox, the maildrop itself.
pub const INBOX: &str = "INBOX";
/// The drafts folder of the service's own.
pub const DRAFTS: &str = "Drafts";
/// The one UID validity every POP3 account's mark and arrivals stand under, since no number is given twice.
pub const VALIDITY: &str = "pop3";
