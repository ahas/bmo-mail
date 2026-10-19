//! The screen's search box (`docs/mail.md`, 「Search」): Gmail's operators read into an IMAP search.
//! `label:` and `in:` are not IMAP criteria — they choose which label's keyword and which folders the
//! search runs over — so they come back apart from the criteria for the caller to resolve against the
//! account's own labels and folders.
//!
//! The box's words are this service's own query language and are read here. A search is written as IMAP's
//! `SEARCH` keys, since the IMAP library takes the keys as the command's text: a text that is plain ASCII
//! is written quoted, and any other as a non-synchronising literal under `CHARSET UTF-8`.

use std::sync::LazyLock;

use chrono::NaiveDate;
use regex::Regex;

/// One criterion of an IMAP search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Search {
  All,
  Seen(bool),
  Flagged(bool),
  Keyword(String),
  Text(String),
  From(String),
  To(String),
  Cc(String),
  Bcc(String),
  Subject(String),
  Header(String, String),
  Before(NaiveDate),
  Since(NaiveDate),
  Not(Box<Search>),
  /// At least one of them.
  Or(Vec<Search>),
  /// Every one of them.
  And(Vec<Search>),
}

/// An IMAP string: quoted where it is printable ASCII, a non-synchronising literal where it is not.
fn string(text: &str, out: &mut String, utf8: &mut bool) {
  if text.chars().all(|c| c.is_ascii() && !c.is_ascii_control()) {
    out.push_str(&crate::imap::quoted(text));
  } else {
    *utf8 = true;
    out.push_str(&format!("{{{}+}}\r\n{text}", text.len()));
  }
}

/// A keyword as an IMAP atom: only the letters, digits and marks a keyword of this service is made of, so a
/// keyword asked for by a request can never write more of the command than one atom.
fn atom(keyword: &str) -> String {
  let kept: String = keyword
    .chars()
    .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '$'))
    .collect();

  if kept.is_empty() { "$None".to_owned() } else { kept }
}

fn day(date: &NaiveDate) -> String {
  date.format("%-d-%b-%Y").to_string()
}

impl Search {
  fn write(&self, out: &mut String, utf8: &mut bool) {
    let pair = |out: &mut String, utf8: &mut bool, key: &str, text: &str| {
      out.push_str(key);
      out.push(' ');
      string(text, out, utf8);
    };

    match self {
      Self::All => out.push_str("ALL"),
      Self::Seen(true) => out.push_str("SEEN"),
      Self::Seen(false) => out.push_str("UNSEEN"),
      Self::Flagged(true) => out.push_str("FLAGGED"),
      Self::Flagged(false) => out.push_str("UNFLAGGED"),
      Self::Keyword(keyword) => out.push_str(&format!("KEYWORD {}", atom(keyword))),
      Self::Text(text) => pair(out, utf8, "TEXT", text),
      Self::From(text) => pair(out, utf8, "FROM", text),
      Self::To(text) => pair(out, utf8, "TO", text),
      Self::Cc(text) => pair(out, utf8, "CC", text),
      Self::Bcc(text) => pair(out, utf8, "BCC", text),
      Self::Subject(text) => pair(out, utf8, "SUBJECT", text),
      Self::Header(name, value) => {
        out.push_str("HEADER ");
        string(name, out, utf8);
        out.push(' ');
        string(value, out, utf8);
      }
      Self::Before(date) => out.push_str(&format!("BEFORE {}", day(date))),
      Self::Since(date) => out.push_str(&format!("SINCE {}", day(date))),
      Self::Not(inner) => {
        out.push_str("NOT ");
        inner.write(out, utf8);
      }
      Self::Or(terms) => match terms.as_slice() {
        [] => out.push_str("ALL"),
        [one] => one.write(out, utf8),
        [first, rest @ ..] => {
          out.push_str("OR ");
          first.write(out, utf8);
          out.push(' ');
          Self::Or(rest.to_vec()).write(out, utf8);
        }
      },
      Self::And(terms) if terms.is_empty() => out.push_str("ALL"),
      Self::And(terms) => {
        out.push('(');

        for (index, term) in terms.iter().enumerate() {
          if index > 0 {
            out.push(' ');
          }

          term.write(out, utf8);
        }

        out.push(')');
      }
    }
  }

  /// The search as the keys of a `SEARCH` command.
  pub fn command(&self) -> String {
    let mut keys = String::new();
    let mut utf8 = false;

    self.write(&mut keys, &mut utf8);

    if utf8 { format!("CHARSET UTF-8 {keys}") } else { keys }
  }
}

/// Every term as one search.
pub fn all_of(terms: Vec<Search>) -> Search {
  match terms.len() {
    0 => Search::All,
    1 => terms.into_iter().next().expect("one term"),
    _ => Search::And(terms),
  }
}

/// What the search box asked.
#[derive(Debug, Default)]
pub struct MailQuery {
  /// Every criterion, all of which must hold.
  pub terms: Vec<Search>,
  /// The label names `label:` asked for.
  pub labels: Vec<String>,
  /// What `in:` asked for, as written: a folder role (`inbox`, `sent`, `drafts`, `spam`, `trash`),
  /// `starred`, or a folder's name. None when the search runs over the usual folders.
  pub scope: Option<String>,
}

static WORD: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r#"(?i)-?(?:[a-z_]+:)?(?:"[^"]*"?|\S+)"#).expect("the word pattern reads"));
static OPERATOR: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"(?is)^([a-z_]+):(.*)$").expect("the operator pattern reads"));
static DAY: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^(\d{4})[-/.](\d{1,2})[-/.](\d{1,2})$").expect("the day pattern reads"));

/// A date the way Gmail writes one — `2026/09/18` or `2026-09-18` — or none.
fn date_of(value: &str) -> Option<NaiveDate> {
  let found = DAY.captures(value)?;

  NaiveDate::from_ymd_opt(found[1].parse().ok()?, found[2].parse().ok()?, found[3].parse().ok()?)
}

/// The query cut into words, a quoted run kept whole, an operator kept with its value.
pub fn query_words(query: &str) -> Vec<String> {
  WORD.find_iter(query).map(|found| found.as_str().to_owned()).collect()
}

fn unquote(value: &str) -> &str {
  let value = value.strip_prefix('"').unwrap_or(value);

  value.strip_suffix('"').unwrap_or(value)
}

/// One word's criterion, or none for a word that only scopes the search.
fn criterion(word: &str, query: &mut MailQuery) -> Option<Search> {
  let Some(found) = OPERATOR.captures(word) else {
    return Some(Search::Text(unquote(word).to_owned()));
  };
  let operator = found[1].to_lowercase();
  let value = unquote(&found[2]).to_owned();

  if value.is_empty() {
    return Some(Search::Text(word.to_owned()));
  }

  Some(match operator.as_str() {
    "from" => Search::From(value),
    "to" => Search::To(value),
    "cc" => Search::Cc(value),
    "bcc" => Search::Bcc(value),
    "subject" => Search::Subject(value),
    "label" => {
      query.labels.push(value);

      return None;
    }
    "in" => {
      query.scope = Some(value.to_lowercase());

      return None;
    }
    "is" => match value.to_lowercase().as_str() {
      "unread" => Search::Seen(false),
      "read" => Search::Seen(true),
      "starred" => Search::Flagged(true),
      "unstarred" => Search::Flagged(false),
      _ => Search::Text(word.to_owned()),
    },
    // A message with an attachment is a mixed multipart in every mailer the service meets.
    "has" if value.eq_ignore_ascii_case("attachment") => {
      Search::Header("content-type".to_owned(), "multipart/mixed".to_owned())
    }
    "before" | "after" => match date_of(&value) {
      Some(date) if operator == "before" => Search::Before(date),
      Some(date) => Search::Since(date),
      None => Search::Text(word.to_owned()),
    },
    _ => Search::Text(word.to_owned()),
  })
}

/// What the search box's text asks.
pub fn parse_mail_query(text: &str) -> MailQuery {
  let mut query = MailQuery::default();

  for word in query_words(text) {
    let negated = word.starts_with('-') && word.len() > 1;
    let term = criterion(if negated { &word[1..] } else { &word }, &mut query);

    if let Some(term) = term {
      query.terms.push(if negated { Search::Not(Box::new(term)) } else { term });
    }
  }

  query
}
