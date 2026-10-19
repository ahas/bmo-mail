//! Labels (`docs/mail.md`, 「Where each piece of state lives」, 「The mail server」): kept in the store, and
//! carried on a message as an IMAP keyword of the service's own form.

/// The keyword names Maildir keeps per folder in its files; a keyword past them lives only in the server's
/// index and is lost when the index is rebuilt.
const KEYWORDS_PER_FOLDER: usize = 26;

/// The keywords the service sets itself, which count against the folder's keywords as much as a label's.
pub const OWN_KEYWORDS: [&str; 4] = ["$Forwarded", "$Junk", "$NotJunk", "$MDNSent"];

/// The most labels an account may hold.
pub const MAX_LABELS: usize = KEYWORDS_PER_FOLDER - OWN_KEYWORDS.len();
