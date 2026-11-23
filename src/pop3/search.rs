//! A search over what the store keeps of a POP3 account's messages (`docs/mail.md`, 「A POP3 account」): the
//! search box's criteria judged against each message's head and the flags the service keeps for it. Words match
//! the subject and the addresses, never the text, since reading the text would fetch every message whole.

use chrono::NaiveDate;

use crate::imap::messages::Heads;
use crate::imap::search::Search;
use crate::mail::MailAddress;
use crate::rules::MessageFacts;

/// What one message is judged by: its head read, its headers by name, its flags and when it arrived.
pub struct Judged<'a> {
  pub heads: &'a Heads,
  pub facts: &'a MessageFacts,
  pub flags: &'a [String],
  pub at: i64,
}

fn holds_text(text: &str, wanted: &str) -> bool {
  text.to_lowercase().contains(&wanted.to_lowercase())
}

fn holds_people(people: &[MailAddress], wanted: &str) -> bool {
  people
    .iter()
    .any(|one| holds_text(&one.address, wanted) || holds_text(&one.name, wanted))
}

fn day(at: i64) -> NaiveDate {
  chrono::DateTime::from_timestamp_millis(at).unwrap_or_default().date_naive()
}

impl Judged<'_> {
  /// Whether the message meets the search.
  pub fn meets(&self, search: &Search) -> bool {
    let heads = self.heads;
    let has = |flag: &str| self.flags.iter().any(|one| one == flag);

    match search {
      Search::All => true,
      Search::Seen(seen) => has("\\Seen") == *seen,
      Search::Flagged(flagged) => has("\\Flagged") == *flagged,
      Search::Keyword(keyword) => has(keyword),
      Search::Text(words) => {
        holds_text(&heads.subject, words)
          || [&heads.from, &heads.to, &heads.cc, &heads.bcc]
            .into_iter()
            .any(|people| holds_people(people, words))
      }
      Search::From(words) => holds_people(&heads.from, words),
      Search::To(words) => holds_people(&heads.to, words),
      Search::Cc(words) => holds_people(&heads.cc, words),
      Search::Bcc(words) => holds_people(&heads.bcc, words),
      Search::Subject(words) => holds_text(&heads.subject, words),
      Search::Header(name, value) => {
        self.facts.headers.get(&name.to_lowercase()).is_some_and(|held| holds_text(held, value))
      }
      Search::Before(date) => day(self.at) < *date,
      Search::Since(date) => day(self.at) >= *date,
      Search::Not(inner) => !self.meets(inner),
      Search::Or(any) => any.iter().any(|one| self.meets(one)),
      Search::And(all) => all.iter().all(|one| self.meets(one)),
    }
  }
}
