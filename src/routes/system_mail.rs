//! The platform's system mail (`docs/api.md`, 「System mail」): the account service's sends of the platform's own
//! mail, under a credential of its own. A send names the platform's outgoing server, the sender, the address of the
//! secret it signs in with, the recipient and the words; the mail goes at once and nothing of it is kept.

use std::path::{Path, PathBuf};

use axum::http::{Method, StatusCode};
use axum::response::Response;
use mail_builder::MessageBuilder;
use mail_builder::headers::address::Address;

use crate::adapters::servers::MailAccount;
use crate::http::{Answer, Call, Guard, Operation, Refused, Route, UPSTREAM_FAILED, answer, schema};
use crate::routes::requests::{Asked, MailFault, address_shaped, one_line};
use crate::routes::servers::platform_outgoing;
use crate::servers::PLATFORM;
use crate::wire::{NewSystemMail, OutgoingServers, Protocol, ServerAuth, SystemMailSent};
use crate::writing::compose::new_message_id;
use crate::writing::submission::submit;

/// The longest a kind may be.
const KIND_LONGEST: usize = 64;

/// `GET /outgoing-servers`.
pub struct ListOutgoingServers;

impl Route for ListOutgoingServers {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/outgoing-servers";
  const OPERATION: Operation = Operation {
    id: "listOutgoingServers",
    summary: "The platform's outgoing servers, which the account service chooses the system mail's from.",
    tag: "system-mail",
    keys: &[],
    query: None,
    body: None,
    status: 200,
    answer: Answer::Json(schema::<OutgoingServers>),
    codes: &[],
    guard: Guard::SystemMail,
  };

  async fn answer(&self, call: Call) -> Result<Response, Refused> {
    Ok(answer(StatusCode::OK, &platform_outgoing(call.standing()).await?))
  }
}

crate::route!(ListOutgoingServers);

/// The folder below the secret store's own that an organisation unit's secrets stand in, named by the unit's id.
const UNIT_SPACE: &str = "units";

/// Whether `name` is one plain file name of the secret store: letters, digits, `.`, `_` and `-`, with no leading
/// dot.
fn plain_name(name: &str) -> bool {
  !name.is_empty()
    && !name.starts_with('.')
    && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// The file a secret's address stands for, or none where it is no address the store gives: a plain file name in
/// its folder, which is the platform's space, or `units/<unit>/<name>` in a unit's space below it, `<unit>` the
/// unit's id in decimal digits. Every part is judged on its own and the path is built from those parts alone, so
/// an address never names a file outside the store's folder.
fn secret_file(folder: &Path, address: &str) -> Option<PathBuf> {
  if let Some((space, rest)) = address.split_once('/') {
    if space != UNIT_SPACE {
      return None;
    }

    let (unit, name) = rest.split_once('/')?;
    let shaped = !unit.is_empty() && unit.chars().all(|c| c.is_ascii_digit()) && plain_name(name);

    return shaped.then(|| folder.join(UNIT_SPACE).join(unit).join(name));
  }

  plain_name(address).then(|| folder.join(address))
}

/// A secret's login and password, read from the file its address stands for: each line's key before its first
/// `=` and its value exactly as written after it. The secret store's own line syntax, read here.
fn secret_login(folder: Option<&Path>, address: &str, from: &str) -> Result<MailAccount, Asked> {
  let unknown = || Asked::rule("unknown-secret", format!("no secret named {address} holds a password"));
  let file = folder.and_then(|folder| secret_file(folder, address)).ok_or_else(unknown)?;
  let text = std::fs::read_to_string(file).map_err(|_| unknown())?;
  let value = |key: &str| {
    text
      .lines()
      .filter_map(|line| line.split_once('='))
      .find(|(found, _)| found.trim() == key)
      .map(|(_, value)| value.trim_end_matches('\r').to_owned())
  };
  let password = value("PASSWORD").filter(|password| !password.is_empty()).ok_or_else(unknown)?;
  let login = value("USERNAME")
    .filter(|login| !login.trim().is_empty())
    .unwrap_or_else(|| from.to_owned());

  Ok(MailAccount {
    address: from.to_owned(),
    login,
    password,
  })
}

/// `POST /system-mails`.
pub struct CreateSystemMail;

impl Route for CreateSystemMail {
  const METHOD: Method = Method::POST;
  const PATH: &'static str = "/system-mails";
  const OPERATION: Operation = Operation {
    id: "createSystemMails",
    summary: "Send one mail of the platform's own through one of its outgoing servers, at once.",
    tag: "system-mail",
    keys: &[],
    query: None,
    body: Some(schema::<NewSystemMail>),
    status: 201,
    answer: Answer::Json(schema::<SystemMailSent>),
    codes: &["unknown-server", "unknown-secret", "sign-in-failed", "upstream-failed"],
    guard: Guard::SystemMail,
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let mail: NewSystemMail = call.body().await?;
    let standing = call.standing();
    let from = mail.from.trim().to_lowercase();
    let to = mail.to.trim().to_owned();

    if !address_shaped(&from) || !address_shaped(&to) {
      return Err(Refused::bad_request("the sender or the recipient is not an address"));
    }

    let kind = mail.kind.trim();

    if kind.is_empty()
      || kind.len() > KIND_LONGEST
      || !kind.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
      return Err(Refused::bad_request("a system mail's kind is a kebab-case word"));
    }

    let server = standing
      .servers
      .get(mail.server)
      .await?
      .filter(|server| server.holder == PLATFORM && server.protocol() == Protocol::Smtp)
      .ok_or_else(|| {
        MailFault::from(Asked::rule(
          "unknown-server",
          "the server is none of the platform's outgoing servers",
        ))
      })?;
    let endpoint = server.endpoint();
    let login = match mail.secret.as_deref().map(str::trim).filter(|name| !name.is_empty()) {
      Some(name) => secret_login(standing.settings.system_mail.secrets_dir.as_deref(), name, &from)
        .map_err(MailFault::from)?,
      None if endpoint.auth == ServerAuth::None => MailAccount {
        address: from.clone(),
        login: from.clone(),
        password: String::new(),
      },
      None => {
        return Err(
          MailFault::from(Asked::rule(
            "unknown-secret",
            "the server signs in and the send names no secret",
          ))
          .into(),
        );
      }
    };
    let message_id = new_message_id(&from);
    let sender = match mail.from_name.as_deref().map(one_line).filter(|name| !name.trim().is_empty()) {
      Some(name) => Address::from((name, from.clone())),
      None => Address::from(from.clone()),
    };
    let mut message = MessageBuilder::new()
      .from(sender)
      .to(Address::from(to.clone()))
      .subject(one_line(&mail.subject))
      .date(chrono::Utc::now().timestamp())
      .message_id(message_id.clone())
      .text_body(mail.text);

    if let Some(html) = mail.html.filter(|html| !html.trim().is_empty()) {
      message = message.html_body(html);
    }

    let raw = message.write_to_vec().map_err(MailFault::server)?;

    submit(&endpoint, &login, &from, &[to], &raw).await.map_err(|fault| match fault {
      MailFault::Server(why) => {
        eprintln!("mail: a system mail of the kind {kind} did not go through server {}: {why}", server.id);

        Refused::new(
          StatusCode::BAD_GATEWAY,
          UPSTREAM_FAILED,
          format!("the mail server did not take the mail: {why}"),
        )
      }
      asked => Refused::from(asked),
    })?;
    eprintln!("mail: a system mail of the kind {kind} went through server {}", server.id);

    Ok(answer(
      StatusCode::CREATED,
      &SystemMailSent {
        sent: true,
        message_id,
      },
    ))
  }
}

crate::route!(CreateSystemMail);
