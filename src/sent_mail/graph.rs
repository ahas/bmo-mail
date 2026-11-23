//! The Graph road (`docs/mail.md`, 「A person's sent mail, read on their own machine」): a Graph access token read
//! for the person from the page-token service, then the messages of their sent items folder sent within
//! the day, followed page by page.

use chrono::{DateTime, SecondsFormat};
use reqwest::Url;
use serde::Deserialize;

use super::{Day, Recipient, RecipientKind, SentMessage};
use crate::clients::page_tokens::PageTokens;
use crate::settings::Upstream;

/// The Graph scopes a token that reads a person's sent mail holds one of: the one that reads what this road
/// reads, and the ones that also read more or write.
pub const SCOPES: [&str; 3] = ["Mail.ReadBasic", "Mail.Read", "Mail.ReadWrite"];

/// The most messages Graph is asked for on one page.
const PAGE_SIZE: &str = "100";

#[derive(Deserialize)]
struct EmailAddress {
  #[serde(default)]
  name: Option<String>,
  #[serde(default)]
  address: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphRecipient {
  email_address: EmailAddress,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphMessage {
  #[serde(default)]
  subject: Option<String>,
  sent_date_time: String,
  #[serde(default)]
  to_recipients: Vec<GraphRecipient>,
  #[serde(default)]
  cc_recipients: Vec<GraphRecipient>,
  #[serde(default)]
  bcc_recipients: Vec<GraphRecipient>,
}

#[derive(Deserialize)]
struct GraphPage {
  value: Vec<GraphMessage>,
  #[serde(rename = "@odata.nextLink", default)]
  next_link: Option<String>,
}

#[derive(Deserialize)]
struct GraphError {
  error: GraphErrorBody,
}

#[derive(Deserialize)]
struct GraphErrorBody {
  code: String,
  message: String,
}

impl GraphMessage {
  fn sent(self) -> Result<SentMessage, String> {
    let sent_at = DateTime::parse_from_rfc3339(&self.sent_date_time)
      .map_err(|_| format!("Graph gave a sending time that does not read: {}", self.sent_date_time))?
      .timestamp_millis();
    let recipients = [
      (RecipientKind::To, self.to_recipients),
      (RecipientKind::Cc, self.cc_recipients),
      (RecipientKind::Bcc, self.bcc_recipients),
    ]
    .into_iter()
    .flat_map(|(kind, recipients)| {
      recipients.into_iter().map(move |recipient| Recipient {
        name: recipient.email_address.name.unwrap_or_default(),
        address: recipient.email_address.address,
        kind,
      })
    })
    .collect();

    Ok(SentMessage {
      subject: self.subject.unwrap_or_default(),
      sent_at,
      recipients,
    })
  }
}

/// The first page's address: the sent items sent within the day, oldest first, with the fields this road reads.
fn first_page(graph: &str, day: &Day) -> Result<Url, String> {
  let moment = |at: DateTime<chrono::Utc>| at.to_rfc3339_opts(SecondsFormat::Secs, true);
  let filter = format!("sentDateTime ge {} and sentDateTime lt {}", moment(day.from), moment(day.to));

  Url::parse_with_params(
    &format!("{}/me/mailFolders/sentitems/messages", graph.trim_end_matches('/')),
    [
      ("$filter", filter.as_str()),
      ("$select", "subject,sentDateTime,toRecipients,ccRecipients,bccRecipients"),
      ("$orderby", "sentDateTime"),
      ("$top", PAGE_SIZE),
    ],
  )
  .map_err(|why| format!("the Graph address {graph} does not read: {why}"))
}

/// The next page's address Graph gave, taken only under Graph's own address, since the person's token goes
/// with it.
fn next_page(graph: &str, link: &str) -> Result<Url, String> {
  if !link.starts_with(&format!("{}/", graph.trim_end_matches('/'))) {
    return Err("Graph's next page lies outside Graph's address".to_owned());
  }

  Url::parse(link).map_err(|why| format!("Graph's next page does not read: {why}"))
}

/// The day's sent mail of `account`, read through Graph with the token the page-token service lends for them; why the road
/// does not stand otherwise.
pub async fn read(
  upstream: &Upstream,
  graph: &str,
  http: &reqwest::Client,
  account: &str,
  day: &Day,
) -> Result<Vec<SentMessage>, String> {
  let tokens = PageTokens::new(upstream, http.clone()).map_err(|why| why.to_string())?;
  let token = tokens.page_token(account, &SCOPES).await.map_err(|why| why.to_string())?.token;
  let mut next = Some(first_page(graph, day)?);
  let mut messages = Vec::new();

  while let Some(page) = next.take() {
    let answer = http
      .get(page)
      .bearer_auth(&token)
      .send()
      .await
      .map_err(|why| format!("Graph could not be reached: {why}"))?;
    let status = answer.status();
    let body = answer.bytes().await.map_err(|why| format!("Graph's answer broke off: {why}"))?;

    if !status.is_success() {
      let why = serde_json::from_slice::<GraphError>(&body)
        .map(|refusal| format!("{}: {}", refusal.error.code, refusal.error.message))
        .unwrap_or_default();

      return Err(format!("Graph answered {status} {why}").trim_end().to_owned());
    }

    let page: GraphPage =
      serde_json::from_slice(&body).map_err(|why| format!("Graph's answer does not read: {why}"))?;

    for message in page.value {
      messages.push(message.sent()?);
    }
    next = page.next_link.map(|link| next_page(graph, &link)).transpose()?;
  }

  Ok(messages)
}
