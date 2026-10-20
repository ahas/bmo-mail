//! The service's client (`docs/api.md`, 「The client」): one method for each route, sending and reading the
//! bodies `docs/openapi.json` states, and a refusal read back as its status, code and sentence. It carries the
//! contract and no caller's policy.

use std::time::Duration;

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use reqwest::{Method, RequestBuilder};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::wire::{
  Account, Address, AddressQuery, Draft, DraftPatch, Event, EventQuery, Folder, FolderPatch, Health, Label,
  LabelPatch, ListQuery, Message, MessagePatch, MessageQuery, NewAttachment, NewDraft, NewLabel,
  NewReceipt, NewRule, NewRuleRun, NewSend, Page, Receipt, Rule, RulePatch, RuleRun, Send, Settings,
  SettingsPatch, SpamSender, Summary, Thread, ThreadQuery, WireError,
};

/// How long one call waits for its answer.
const PATIENCE: Duration = Duration::from_secs(120);

/// What a path segment keeps as it is: the characters no URI gives a meaning of their own.
const SEGMENT: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'.').remove(b'_').remove(b'~');

/// Why a call came to no answer of the route's shape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientFailure {
  /// The service answered with a status that is no success: its error body where it was one, and the body as
  /// it came.
  Answered {
    status: u16,
    answer: Option<WireError>,
    body: String,
  },
  /// Nothing answered at the address.
  Unreached(String),
  /// A success came back in no shape the route answers.
  Unreadable(String),
}

impl std::fmt::Display for ClientFailure {
  fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Answered {
        status,
        answer: Some(answer),
        ..
      } => write!(out, "{status} {}: {}", answer.code, answer.error),
      Self::Answered { status, body, .. } => write!(out, "{status}: {body}"),
      Self::Unreached(why) => write!(out, "the service could not be reached: {why}"),
      Self::Unreadable(why) => write!(out, "the service's answer does not read: {why}"),
    }
  }
}

impl std::error::Error for ClientFailure {}

type Called<T> = Result<T, ClientFailure>;

/// A path segment holding a key, every character with a meaning in a path encoded.
pub fn segment(key: &str) -> String {
  utf8_percent_encode(key, SEGMENT).to_string()
}

/// The client of one service: its address and its bearer token.
#[derive(Clone)]
pub struct MailClient {
  address: String,
  token: String,
  http: reqwest::Client,
}

impl MailClient {
  pub fn new(address: impl Into<String>, token: impl Into<String>) -> Self {
    Self {
      address: address.into().trim_end_matches('/').to_owned(),
      token: token.into(),
      http: reqwest::Client::builder().timeout(PATIENCE).build().unwrap_or_default(),
    }
  }

  fn request(&self, method: Method, path: &str) -> RequestBuilder {
    self
      .http
      .request(method, format!("{}{path}", self.address))
      .bearer_auth(&self.token)
      .header(reqwest::header::ACCEPT, "application/json")
  }

  /// Sends a call and answers the body of a success.
  async fn send(&self, request: RequestBuilder) -> Called<String> {
    let answer = request.send().await.map_err(|why| ClientFailure::Unreached(why.to_string()))?;
    let status = answer.status().as_u16();
    let body = answer.text().await.map_err(|why| ClientFailure::Unreadable(why.to_string()))?;

    if !(200..300).contains(&status) {
      return Err(ClientFailure::Answered {
        status,
        answer: serde_json::from_str(&body).ok(),
        body,
      });
    }

    Ok(body)
  }

  async fn read<T: DeserializeOwned>(&self, request: RequestBuilder) -> Called<T> {
    let body = self.send(request).await?;

    serde_json::from_str(&body).map_err(|why| ClientFailure::Unreadable(why.to_string()))
  }

  async fn get<T: DeserializeOwned>(&self, path: &str, query: &impl Serialize) -> Called<T> {
    self.read(self.request(Method::GET, path).query(query)).await
  }

  async fn with_body<T: DeserializeOwned>(
    &self,
    method: Method,
    path: &str,
    body: &impl Serialize,
  ) -> Called<T> {
    self.read(self.request(method, path).json(body)).await
  }

  async fn delete(&self, path: &str) -> Called<()> {
    self.send(self.request(Method::DELETE, path)).await.map(drop)
  }

  /// A byte answer's bytes.
  async fn bytes(&self, path: &str) -> Called<Vec<u8>> {
    let answer = self
      .request(Method::GET, path)
      .send()
      .await
      .map_err(|why| ClientFailure::Unreached(why.to_string()))?;
    let status = answer.status().as_u16();

    if !(200..300).contains(&status) {
      let body = answer.text().await.unwrap_or_default();

      return Err(ClientFailure::Answered {
        status,
        answer: serde_json::from_str(&body).ok(),
        body,
      });
    }

    answer
      .bytes()
      .await
      .map(|bytes| bytes.to_vec())
      .map_err(|why| ClientFailure::Unreadable(why.to_string()))
  }

  fn account(name: &str) -> String {
    format!("/accounts/{}", segment(name))
  }

  /// `GET /health`.
  pub async fn health(&self) -> Called<Health> {
    self.get("/health", &()).await
  }

  /// `GET /events`, as a page.
  pub async fn events(&self, query: &EventQuery) -> Called<Page<Event>> {
    self.get("/events", query).await
  }

  /// `GET /accounts`.
  pub async fn accounts(&self, query: &ListQuery) -> Called<Page<Account>> {
    self.get("/accounts", query).await
  }

  /// `GET /summary`.
  pub async fn summary(&self) -> Called<Summary> {
    self.get("/summary", &()).await
  }

  /// `GET /accounts/{accountName}/folders`.
  pub async fn folders(&self, account: &str, query: &ListQuery) -> Called<Page<Folder>> {
    self.get(&format!("{}/folders", Self::account(account)), query).await
  }

  /// `PATCH /accounts/{accountName}/folders/{folderName}`.
  pub async fn change_folder(&self, account: &str, folder: &str, patch: &FolderPatch) -> Called<Folder> {
    let path = format!("{}/folders/{}", Self::account(account), segment(folder));

    self.with_body(Method::PATCH, &path, patch).await
  }

  /// `GET /accounts/{accountName}/messages`.
  pub async fn messages(&self, account: &str, query: &MessageQuery) -> Called<Page<Message>> {
    self.get(&format!("{}/messages", Self::account(account)), query).await
  }

  fn message_path(account: &str, message: &str) -> String {
    format!("{}/messages/{}", Self::account(account), segment(message))
  }

  /// `GET /accounts/{accountName}/messages/{messageId}`.
  pub async fn message(&self, account: &str, message: &str) -> Called<Message> {
    self.get(&Self::message_path(account, message), &()).await
  }

  /// `PATCH /accounts/{accountName}/messages/{messageId}`; none where the server did not say where a moved
  /// message landed.
  pub async fn change_message(
    &self,
    account: &str,
    message: &str,
    patch: &MessagePatch,
  ) -> Called<Option<Message>> {
    let request = self.request(Method::PATCH, &Self::message_path(account, message)).json(patch);
    let body = self.send(request).await?;

    if body.trim().is_empty() {
      return Ok(None);
    }

    serde_json::from_str(&body)
      .map(Some)
      .map_err(|why| ClientFailure::Unreadable(why.to_string()))
  }

  /// `DELETE /accounts/{accountName}/messages/{messageId}`.
  pub async fn delete_message(&self, account: &str, message: &str) -> Called<()> {
    self.delete(&Self::message_path(account, message)).await
  }

  /// `GET /accounts/{accountName}/messages/{messageId}/parts/{partId}/content`: the part's bytes.
  pub async fn part_content(&self, account: &str, message: &str, part: &str) -> Called<Vec<u8>> {
    self
      .bytes(&format!("{}/parts/{}/content", Self::message_path(account, message), segment(part)))
      .await
  }

  /// `GET /accounts/{accountName}/threads/{threadId}`.
  pub async fn thread(&self, account: &str, thread: &str, query: &ThreadQuery) -> Called<Thread> {
    self
      .get(&format!("{}/threads/{}", Self::account(account), segment(thread)), query)
      .await
  }

  /// `GET /accounts/{accountName}/labels`.
  pub async fn labels(&self, account: &str, query: &ListQuery) -> Called<Page<Label>> {
    self.get(&format!("{}/labels", Self::account(account)), query).await
  }

  /// `POST /accounts/{accountName}/labels`.
  pub async fn create_label(&self, account: &str, label: &NewLabel) -> Called<Label> {
    self.with_body(Method::POST, &format!("{}/labels", Self::account(account)), label).await
  }

  /// `PATCH /accounts/{accountName}/labels/{labelId}`.
  pub async fn change_label(&self, account: &str, label: &str, patch: &LabelPatch) -> Called<Label> {
    let path = format!("{}/labels/{}", Self::account(account), segment(label));

    self.with_body(Method::PATCH, &path, patch).await
  }

  /// `DELETE /accounts/{accountName}/labels/{labelId}`.
  pub async fn delete_label(&self, account: &str, label: &str) -> Called<()> {
    self.delete(&format!("{}/labels/{}", Self::account(account), segment(label))).await
  }

  /// `GET /accounts/{accountName}/addresses`.
  pub async fn addresses(&self, account: &str, query: &AddressQuery) -> Called<Page<Address>> {
    self.get(&format!("{}/addresses", Self::account(account)), query).await
  }

  /// `POST /accounts/{accountName}/drafts`.
  pub async fn create_draft(&self, account: &str, draft: &NewDraft) -> Called<Draft> {
    self.with_body(Method::POST, &format!("{}/drafts", Self::account(account)), draft).await
  }

  fn draft_path(account: &str, draft: &str) -> String {
    format!("{}/drafts/{}", Self::account(account), segment(draft))
  }

  /// `GET /accounts/{accountName}/drafts/{draftId}`.
  pub async fn draft(&self, account: &str, draft: &str) -> Called<Draft> {
    self.get(&Self::draft_path(account, draft), &()).await
  }

  /// `PATCH /accounts/{accountName}/drafts/{draftId}`.
  pub async fn change_draft(&self, account: &str, draft: &str, patch: &DraftPatch) -> Called<Draft> {
    self.with_body(Method::PATCH, &Self::draft_path(account, draft), patch).await
  }

  /// `DELETE /accounts/{accountName}/drafts/{draftId}`.
  pub async fn delete_draft(&self, account: &str, draft: &str) -> Called<()> {
    self.delete(&Self::draft_path(account, draft)).await
  }

  /// `POST /accounts/{accountName}/drafts/{draftId}/attachments`.
  pub async fn attach(&self, account: &str, draft: &str, file: &NewAttachment) -> Called<Draft> {
    self
      .with_body(Method::POST, &format!("{}/attachments", Self::draft_path(account, draft)), file)
      .await
  }

  /// `POST /accounts/{accountName}/sends`.
  pub async fn create_send(&self, account: &str, send: &NewSend) -> Called<Send> {
    self.with_body(Method::POST, &format!("{}/sends", Self::account(account)), send).await
  }

  /// `GET /accounts/{accountName}/sends/{sendId}`.
  pub async fn send_of(&self, account: &str, send: &str) -> Called<Send> {
    self.get(&format!("{}/sends/{}", Self::account(account), segment(send)), &()).await
  }

  /// `DELETE /accounts/{accountName}/sends/{sendId}`.
  pub async fn cancel_send(&self, account: &str, send: &str) -> Called<()> {
    self.delete(&format!("{}/sends/{}", Self::account(account), segment(send))).await
  }

  /// `POST /accounts/{accountName}/receipts`.
  pub async fn create_receipt(&self, account: &str, receipt: &NewReceipt) -> Called<Receipt> {
    self
      .with_body(Method::POST, &format!("{}/receipts", Self::account(account)), receipt)
      .await
  }

  /// `GET /accounts/{accountName}/rules`.
  pub async fn rules(&self, account: &str, query: &ListQuery) -> Called<Page<Rule>> {
    self.get(&format!("{}/rules", Self::account(account)), query).await
  }

  /// `POST /accounts/{accountName}/rules`.
  pub async fn create_rule(&self, account: &str, rule: &NewRule) -> Called<Rule> {
    self.with_body(Method::POST, &format!("{}/rules", Self::account(account)), rule).await
  }

  /// `PATCH /accounts/{accountName}/rules/{ruleId}`.
  pub async fn change_rule(&self, account: &str, rule: i64, patch: &RulePatch) -> Called<Rule> {
    self
      .with_body(Method::PATCH, &format!("{}/rules/{rule}", Self::account(account)), patch)
      .await
  }

  /// `DELETE /accounts/{accountName}/rules/{ruleId}`.
  pub async fn delete_rule(&self, account: &str, rule: i64) -> Called<()> {
    self.delete(&format!("{}/rules/{rule}", Self::account(account))).await
  }

  /// `POST /accounts/{accountName}/rule-runs`.
  pub async fn create_rule_run(&self, account: &str, run: &NewRuleRun) -> Called<RuleRun> {
    self
      .with_body(Method::POST, &format!("{}/rule-runs", Self::account(account)), run)
      .await
  }

  /// `PUT /accounts/{accountName}/spam-senders/{address}`.
  pub async fn put_spam_sender(&self, account: &str, address: &str) -> Called<SpamSender> {
    self
      .read(
        self.request(Method::PUT, &format!("{}/spam-senders/{}", Self::account(account), segment(address))),
      )
      .await
  }

  /// `DELETE /accounts/{accountName}/spam-senders/{address}`.
  pub async fn delete_spam_sender(&self, account: &str, address: &str) -> Called<()> {
    self
      .delete(&format!("{}/spam-senders/{}", Self::account(account), segment(address)))
      .await
  }

  /// `GET /accounts/{accountName}/settings`.
  pub async fn settings(&self, account: &str) -> Called<Settings> {
    self.get(&format!("{}/settings", Self::account(account)), &()).await
  }

  /// `PATCH /accounts/{accountName}/settings`.
  pub async fn change_settings(&self, account: &str, patch: &SettingsPatch) -> Called<Settings> {
    self
      .with_body(Method::PATCH, &format!("{}/settings", Self::account(account)), patch)
      .await
  }

  /// `GET /avatars/{address}`: the logo's bytes.
  pub async fn avatar(&self, address: &str) -> Called<Vec<u8>> {
    self.bytes(&format!("/avatars/{}", segment(address))).await
  }
}
