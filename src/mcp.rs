//! `bmo-mail mcp`: the harness plugin's MCP server on standard input and output (`docs/api.md`, 「The harness
//! plugin」). Each tool is a thin call of one route through the service's client; the server lists them and
//! the OpenAPI document states them under `x-tools`, both from the one table here.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use rmcp::model::{
  CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation, InitializeResult,
  JsonObject, ListToolsResult, PaginatedRequestParams, ServerCapabilities, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler};
use schemars::generate::{SchemaGenerator, SchemaSettings};
use schemars::{JsonSchema, Schema};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::client::{ClientFailure, MailClient};
use crate::wire::{
  DraftAttachment, DraftPatch, ListQuery, Mailbox, Message, MessagePatch, MessageQuery, NewDraft, NewSend,
  ThreadQuery,
};

/// The plugin a harness takes mail in, and the command that starts its MCP server.
pub const PLUGIN: &str = "bmo-mail";
pub const MCP: &str = "bmo-mail mcp";

/// What a harness does first with it, which the server tells a harness when it starts.
pub const ENTRY: &str = "A harness reads a mailbox with mail_messages and mail_thread, writes with mail_draft \
  and mail_draft_update, and sends with mail_send.";

/// What a tool's call came to short of the route's answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolFailure {
  /// The arguments are not the shape the tool takes.
  Arguments(String),
  /// The call reached no answer of the route's shape.
  Client(ClientFailure),
  /// The route answered, but with nothing of the tool's output.
  Unanswered(String),
}

impl ToolFailure {
  /// The failure as the tool error's content: the error shape the service answered with and its status, or the
  /// sentence saying why nothing answered.
  pub fn content(&self) -> Value {
    match self {
      Self::Arguments(why) => json!({ "code": "bad-request", "error": why }),
      Self::Client(ClientFailure::Answered {
        status,
        answer: Some(answer),
        ..
      }) => json!({ "status": status, "code": answer.code, "error": answer.error }),
      Self::Client(failure @ ClientFailure::Answered { status, .. }) => {
        json!({ "status": status, "error": failure.to_string() })
      }
      Self::Client(failure) => json!({ "error": failure.to_string() }),
      Self::Unanswered(why) => json!({ "error": why }),
    }
  }
}

type Calling = Pin<Box<dyn Future<Output = Result<Value, ToolFailure>> + Send>>;
type Call = Box<dyn Fn(MailClient, Value) -> Calling + Send + Sync>;

/// One tool: its name, what it does, the route it calls, its input and output, and the call itself.
pub struct MailTool {
  pub name: &'static str,
  pub description: &'static str,
  /// The route the tool calls, as its method and path.
  pub route: &'static str,
  input: fn(&mut SchemaGenerator) -> Schema,
  output: fn(&mut SchemaGenerator) -> Schema,
  call: Call,
}

impl MailTool {
  /// The tool's input, its nested types referred to where `generator` keeps its definitions.
  pub fn input(&self, generator: &mut SchemaGenerator) -> Schema {
    (self.input)(generator)
  }

  /// The tool's output, referred to where `generator` keeps its definitions.
  pub fn output(&self, generator: &mut SchemaGenerator) -> Schema {
    (self.output)(generator)
  }

  /// Calls the tool's route through `client` with the tool's arguments.
  pub async fn call(&self, client: MailClient, arguments: Value) -> Result<Value, ToolFailure> {
    (self.call)(client, arguments).await
  }
}

/// A tool whose input is `I` and whose answer is `O`, calling `call` with the arguments read as `I`.
fn tool<I, O, F, Done>(
  name: &'static str,
  description: &'static str,
  route: &'static str,
  call: F,
) -> MailTool
where
  I: DeserializeOwned + JsonSchema + Send + 'static,
  O: Serialize + JsonSchema,
  F: Fn(MailClient, I) -> Done + Send + Sync + 'static,
  Done: Future<Output = Result<O, ToolFailure>> + Send + 'static,
{
  let call = Arc::new(call);

  MailTool {
    name,
    description,
    route,
    input: I::json_schema,
    output: SchemaGenerator::subschema_for::<O>,
    call: Box::new(move |client, arguments| {
      let call = Arc::clone(&call);

      Box::pin(async move {
        let input = serde_json::from_value::<I>(arguments).map_err(|error| {
          ToolFailure::Arguments(format!("The arguments are not the shape this tool takes: {error}."))
        })?;
        let answer = call(client, input).await?;

        serde_json::to_value(answer).map_err(|why| ToolFailure::Arguments(why.to_string()))
      })
    }),
  }
}

/// A page of an account's folders or labels.
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AccountListInput {
  account_name: String,
  /// The position of the last item read; the items after it.
  #[serde(default)]
  after: Option<i64>,
  /// The most items the page holds; the whole list where none is given.
  #[serde(default)]
  limit: Option<u32>,
}

/// A page of an account's messages.
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MessagesInput {
  account_name: String,
  /// The folder to read; the inbox where neither it, `q` nor `labelId` is given.
  #[serde(default)]
  folder_name: Option<String>,
  /// Search words: `from:`, `to:`, `subject:`, `label:`, `in:`, `is:unread`, `is:starred`, `has:attachment`,
  /// `before:` and `after:` with a date, and words of the text.
  #[serde(default)]
  q: Option<String>,
  #[serde(default)]
  label_id: Option<String>,
  /// A message's `at`: the messages older than it.
  #[serde(default)]
  before: Option<i64>,
  /// A message's `at`: the messages newer than it.
  #[serde(default)]
  after: Option<i64>,
  /// 1 to 100; 50 where absent.
  #[serde(default)]
  limit: Option<u32>,
}

/// One message of an account.
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MessageInput {
  account_name: String,
  message_id: String,
}

/// A message's change: the whole of its flags and labels, and the folder to move it to.
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateInput {
  account_name: String,
  message_id: String,
  /// Every flag it should hold among `\Seen`, `\Flagged`, `\Answered`, `$Forwarded`, `$Junk`, `$NotJunk` and
  /// `$MDNSent`; one left out is taken off.
  flags: Vec<String>,
  /// Every label it should carry.
  label_ids: Vec<String>,
  #[serde(default)]
  folder_name: Option<String>,
}

/// One thread of an account.
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ThreadInput {
  account_name: String,
  thread_id: String,
}

/// A draft to start.
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DraftInput {
  account_name: String,
  /// The message it answers, by its id.
  #[serde(default)]
  reply_to: Option<String>,
  /// Whether it answers every recipient of `replyTo`.
  #[serde(default)]
  reply_all: bool,
  /// The message it carries, by its id.
  #[serde(default)]
  forward: Option<String>,
}

/// A draft's change: each field given replaces the draft's own.
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DraftUpdateInput {
  account_name: String,
  draft_id: String,
  #[serde(default)]
  to: Option<Vec<Mailbox>>,
  #[serde(default)]
  cc: Option<Vec<Mailbox>>,
  #[serde(default)]
  bcc: Option<Vec<Mailbox>>,
  #[serde(default)]
  subject: Option<String>,
  /// The body, as HTML.
  #[serde(default)]
  body: Option<String>,
  /// The attachments to keep, by `partId`.
  #[serde(default)]
  attachments: Option<Vec<DraftAttachment>>,
  #[serde(default)]
  receipt_requested: Option<bool>,
}

/// A draft to send.
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SendInput {
  account_name: String,
  draft_id: String,
  /// When it goes, in epoch milliseconds, no later than the undo span from now; after the undo span where
  /// absent.
  #[serde(default)]
  send_at: Option<i64>,
}

fn client_failed(failure: ClientFailure) -> ToolFailure {
  ToolFailure::Client(failure)
}

/// The eight tools.
pub fn tools() -> Vec<MailTool> {
  vec![
    tool(
      "mail_accounts",
      "The mail accounts: each one's name, which every other tool takes as accountName, its address and its \
       inbox's unread count.",
      "GET /accounts",
      |client, query: ListQuery| async move { client.accounts(&query).await.map_err(client_failed) },
    ),
    tool(
      "mail_folders",
      "An account's folders: each one's name, which mail_messages and mail_update take as folderName, and its \
       counts.",
      "GET /accounts/{accountName}/folders",
      |client, asked: AccountListInput| async move {
        let query = ListQuery {
          after: asked.after,
          limit: asked.limit,
        };

        client.folders(&asked.account_name, &query).await.map_err(client_failed)
      },
    ),
    tool(
      "mail_labels",
      "An account's labels in their order: each one's id, which mail_messages takes as labelId and mail_update \
       among labelIds, its name and its colour.",
      "GET /accounts/{accountName}/labels",
      |client, asked: AccountListInput| async move {
        let query = ListQuery {
          after: asked.after,
          limit: asked.limit,
        };

        client.labels(&asked.account_name, &query).await.map_err(client_failed)
      },
    ),
    tool(
      "mail_messages",
      "Messages of a folder, a label or matching search words, newest first, without their bodies. The next \
       page is asked with before set to the last message's at.",
      "GET /accounts/{accountName}/messages",
      |client, asked: MessagesInput| async move {
        let query = MessageQuery {
          folder_name: asked.folder_name,
          q: asked.q,
          label_id: asked.label_id,
          before: asked.before,
          after: asked.after,
          limit: asked.limit,
        };

        client.messages(&asked.account_name, &query).await.map_err(client_failed)
      },
    ),
    tool(
      "mail_message",
      "One message with its plain-text and HTML bodies and its parts.",
      "GET /accounts/{accountName}/messages/{messageId}",
      |client, asked: MessageInput| async move {
        client.message(&asked.account_name, &asked.message_id).await.map_err(client_failed)
      },
    ),
    tool(
      "mail_update",
      "Change a message's flags or labels, or move it to a folder: flags and labelIds are the whole of what it \
       should carry. A moved message answers under its new id.",
      "PATCH /accounts/{accountName}/messages/{messageId}",
      |client, asked: UpdateInput| async move {
        let patch = MessagePatch {
          flags: asked.flags,
          label_ids: asked.label_ids,
          folder_name: asked.folder_name,
        };
        let changed = client.change_message(&asked.account_name, &asked.message_id, &patch).await;

        match changed.map_err(client_failed)? {
          Some(message) => Ok::<Message, ToolFailure>(message),
          None => Err(ToolFailure::Unanswered(
            "The message moved, but the server did not say where.".to_owned(),
          )),
        }
      },
    ),
    tool(
      "mail_thread",
      "One conversation and its messages, oldest first, each with its bodies.",
      "GET /accounts/{accountName}/threads/{threadId}",
      |client, asked: ThreadInput| async move {
        let query = ThreadQuery::default();

        client
          .thread(&asked.account_name, &asked.thread_id, &query)
          .await
          .map_err(client_failed)
      },
    ),
    tool(
      "mail_draft",
      "Start a draft, saved in the drafts folder at once: new, a reply (replyTo, with replyAll for every \
       recipient) or a forward. Its id names it across every save.",
      "POST /accounts/{accountName}/drafts",
      |client, asked: DraftInput| async move {
        let draft = NewDraft {
          reply_to: asked.reply_to,
          reply_all: asked.reply_all,
          forward: asked.forward,
        };

        client.create_draft(&asked.account_name, &draft).await.map_err(client_failed)
      },
    ),
    tool(
      "mail_draft_update",
      "Change a draft: each field given replaces the draft's own, recipients as name and address, the body as \
       HTML. The draft is saved again under the same id.",
      "PATCH /accounts/{accountName}/drafts/{draftId}",
      |client, asked: DraftUpdateInput| async move {
        let patch = DraftPatch {
          to: asked.to,
          cc: asked.cc,
          bcc: asked.bcc,
          subject: asked.subject,
          body: asked.body,
          attachments: asked.attachments,
          receipt_requested: asked.receipt_requested,
        };

        client
          .change_draft(&asked.account_name, &asked.draft_id, &patch)
          .await
          .map_err(client_failed)
      },
    ),
    tool(
      "mail_send",
      "Send a draft: it waits for the account's undo span, or until sendAt, and then goes; the answer's state \
       is waiting.",
      "POST /accounts/{accountName}/sends",
      |client, asked: SendInput| async move {
        let send = NewSend {
          draft_id: asked.draft_id,
          send_at: asked.send_at,
        };

        client.create_send(&asked.account_name, &send).await.map_err(client_failed)
      },
    ),
  ]
}

/// The generator every tool schema and the OpenAPI document are written with: JSON Schema 2020-12, read as a
/// caller sends it, definitions kept at `definitions_path`.
pub fn generator(definitions_path: &'static str) -> SchemaGenerator {
  SchemaSettings::draft2020_12()
    .for_deserialize()
    .with(|settings| {
      settings.definitions_path = definitions_path.into();
      settings.meta_schema = None;
    })
    .into_generator()
}

/// The server over the service the client reaches.
pub struct MailServer {
  client: MailClient,
  tools: Arc<Vec<MailTool>>,
}

impl MailServer {
  pub fn new(client: MailClient) -> Self {
    Self {
      client,
      tools: Arc::new(tools()),
    }
  }

  /// The tools as MCP lists them, each input and output a schema of its own with its definitions inside.
  pub fn listed(&self) -> Vec<Tool> {
    self.tools.iter().map(listed).collect()
  }
}

fn listed(tool: &MailTool) -> Tool {
  let mut generator = generator("/$defs");
  let input = tool.input(&mut generator);
  let output = tool.output(&mut generator);
  let definitions = generator.take_definitions(true);
  let mut listed = Tool::new(tool.name, tool.description, standing_alone(input, &definitions));

  listed.output_schema = Some(Arc::new(standing_alone(output, &definitions)));
  listed
}

/// A schema made to stand on its own: an object at its root, with every definition it refers to beside it.
fn standing_alone(schema: Schema, definitions: &Map<String, Value>) -> JsonObject {
  let mut object = match schema.to_value() {
    Value::Object(object) if object.contains_key("$ref") => {
      let mut wrapped = Map::new();

      wrapped.insert("allOf".into(), Value::Array(vec![Value::Object(object)]));
      wrapped.insert("type".into(), "object".into());
      wrapped
    }
    Value::Object(object) => object,
    _ => Map::from_iter([("type".to_owned(), Value::from("object"))]),
  };

  if !definitions.is_empty() {
    object.insert("$defs".into(), Value::Object(definitions.clone()));
  }

  object
}

impl ServerHandler for MailServer {
  fn get_info(&self) -> InitializeResult {
    let mut info = InitializeResult::new(ServerCapabilities::builder().enable_tools().build());
    let mut server = Implementation::default();

    server.name = "bmo-mail".to_owned();
    server.version = crate::VERSION.to_owned();
    info.server_info = server;
    info.instructions = Some(ENTRY.to_owned());
    info
  }

  async fn list_tools(
    &self,
    _: Option<PaginatedRequestParams>,
    _: RequestContext<RoleServer>,
  ) -> Result<ListToolsResult, ErrorData> {
    Ok(ListToolsResult::with_all_items(self.listed()))
  }

  async fn call_tool(
    &self,
    request: CallToolRequestParams,
    _: RequestContext<RoleServer>,
  ) -> Result<CallToolResponse, ErrorData> {
    let Some(tool) = self.tools.iter().find(|tool| tool.name == request.name) else {
      return Err(ErrorData::invalid_params(format!("There is no tool {}.", request.name), None));
    };
    let arguments = Value::Object(request.arguments.unwrap_or_default());
    let result = match tool.call(self.client.clone(), arguments).await {
      Ok(answer) => CallToolResult::structured(answer),
      Err(failure) => CallToolResult::error(vec![ContentBlock::text(failure.content().to_string())]),
    };

    Ok(CallToolResponse::Complete(result))
  }
}

/// Serves the tools on standard input and output until the harness closes them.
pub async fn serve(client: MailClient) -> Result<(), String> {
  let running = rmcp::ServiceExt::serve(MailServer::new(client), rmcp::transport::stdio())
    .await
    .map_err(|why| format!("the MCP session did not start: {why}"))?;

  running
    .waiting()
    .await
    .map(|_| ())
    .map_err(|why| format!("the MCP session ended in a failure: {why}"))
}
