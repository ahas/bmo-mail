//! The service's own words (`docs/api.md`, 「The service's own words」): what it writes into a message, in the
//! language the settings name. Each language is one TOML file, `wording/<language>.toml`
//! under the settings folder; the repository's own files for the shipped languages are built in, so a service
//! started where none stands still writes.

use chrono::{Locale, TimeZone, Utc};
use chrono_tz::Tz;
use serde::Deserialize;

use crate::settings::Settings;

/// The shipped languages' words, each as the repository's own file states it.
const SHIPPED: [(&str, &str); 4] = [
  ("ko", include_str!("../wording/ko.toml")),
  ("en", include_str!("../wording/en.toml")),
  ("ja", include_str!("../wording/ja.toml")),
  ("zh-CN", include_str!("../wording/zh-CN.toml")),
];

/// The language whose shipped words stand where the settings name a language none is shipped in.
const DEFAULT_LANGUAGE: &str = "ko";

/// The line over a quoted reply.
#[derive(Clone, Debug, Deserialize)]
pub struct Reply {
  pub attribution: String,
}

/// The head of a forwarded original.
#[derive(Clone, Debug, Deserialize)]
pub struct Forward {
  pub header: String,
  pub from: String,
  pub date: String,
  pub subject: String,
  pub to: String,
  pub cc: String,
}

/// A read receipt sent.
#[derive(Clone, Debug, Deserialize)]
pub struct Receipt {
  pub subject: String,
  pub body: String,
}

/// The words, as one language's file holds them.
#[derive(Clone, Debug, Deserialize)]
struct Words {
  locale: String,
  date: String,
  reply: Reply,
  forward: Forward,
  receipt: Receipt,
}

/// The service's own words, with the time zone its dates are written in.
#[derive(Clone, Debug)]
pub struct Wording {
  pub reply: Reply,
  pub forward: Forward,
  pub receipt: Receipt,
  locale: Locale,
  date: String,
  time_zone: Tz,
}

/// Why the words could not be read.
#[derive(Debug, thiserror::Error)]
pub enum WordingError {
  #[error("the words in {0} could not be read: {1}")]
  Read(String, String),
  #[error("the words name a locale no date can be written in: {0}")]
  Locale(String),
  #[error("the settings name a time zone that is not one: {0}")]
  TimeZone(String),
}

impl Wording {
  /// The words of the settings' language: its file under the settings folder, else the words shipped in it,
  /// else the Korean words shipped.
  pub fn load(settings: &Settings) -> Result<Self, WordingError> {
    let language = &settings.wording.language;
    let file = settings.folder.join("wording").join(format!("{language}.toml"));
    let (text, source) = match std::fs::read_to_string(&file) {
      Ok(text) => (text, file.display().to_string()),
      Err(_) => (shipped(language).to_owned(), "the shipped words".to_owned()),
    };

    Self::read(&text, &source, &settings.wording.time_zone)
  }

  fn read(text: &str, source: &str, time_zone: &str) -> Result<Self, WordingError> {
    let words: Words =
      toml::from_str(text).map_err(|why| WordingError::Read(source.to_owned(), why.to_string()))?;
    let locale =
      Locale::try_from(words.locale.as_str()).map_err(|_| WordingError::Locale(words.locale.clone()))?;
    let time_zone = time_zone.parse::<Tz>().map_err(|_| WordingError::TimeZone(time_zone.to_owned()))?;

    Ok(Self {
      reply: words.reply,
      forward: words.forward,
      receipt: words.receipt,
      locale,
      date: words.date,
      time_zone,
    })
  }

  /// A moment, Unix milliseconds, as the words write a date.
  pub fn date(&self, millis: i64) -> String {
    let utc = Utc.timestamp_millis_opt(millis).single().unwrap_or_default();

    utc.with_timezone(&self.time_zone).format_localized(&self.date, self.locale).to_string()
  }
}

/// The words shipped in a language, else the default language's.
fn shipped(language: &str) -> &'static str {
  let words = |wanted: &str| SHIPPED.iter().find(|(shipped, _)| *shipped == wanted).map(|(_, text)| *text);

  words(language)
    .or_else(|| words(DEFAULT_LANGUAGE))
    .expect("the default language's words are shipped")
}

/// A value with each `{name}` it holds replaced by the value given for it.
pub fn fill(template: &str, values: &[(&str, &str)]) -> String {
  values
    .iter()
    .fold(template.to_owned(), |text, (name, value)| text.replace(&format!("{{{name}}}"), value))
}
