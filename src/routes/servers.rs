//! The mail servers (`docs/api.md`, 「Servers」): the servers an owner reaches, a link's servers, each one kept,
//! changed and removed, and a connection test. A server of a link is changed only once the delegation chain took
//! the link's new list in the owner's name, so who may change it is the account service's word.

use axum::http::{Method, StatusCode};
use axum::response::Response;

use crate::adapters::servers::MailAccount;
use crate::http::{
  Answer, Call, Guard, Operation, Refused, Route, Standing, answer, inline, nothing, schema,
};
use crate::routes::requests::{Asked, MailFault};
use crate::servers::organisation::Offering;
use crate::servers::{
  MailServer, ServerDraft, ServerFields, account_count, chain_link, change, ids_held, keep, own_holder,
  probe, remove,
};
use crate::wire::{
  NewServer, NewServerTest, OutgoingServers, Page, Server, ServerHolder, ServerPatch, ServerQuery,
  ServerTest,
};

const SERVER: (&str, &str) = ("serverId", "The server's id.");

/// The link `own` names a person's own servers by in an answer.
const OWN_LINK: &str = "own";

/// A server as an answer carries it, with who holds it and whether the owner may change it.
async fn member(
  standing: &Standing,
  server: &MailServer,
  holder: ServerHolder,
  may_change: bool,
) -> Result<Server, MailFault> {
  let draft = &server.draft;

  Ok(Server {
    id: server.id,
    protocol: draft.protocol,
    direction: draft.protocol.direction(),
    name: draft.name.clone(),
    host: draft.host.clone(),
    port: draft.port,
    security: draft.security,
    tls_name: draft.tls_name.clone(),
    auth: draft.auth,
    holder,
    may_change,
    account_count: standing.servers.account_count(server.id).await?,
  })
}

/// Who holds a server, as an answer names it to an owner.
fn holder_of(server: &MailServer, offering: &[Offering]) -> ServerHolder {
  if server.owner().is_some() {
    return ServerHolder {
      link: OWN_LINK.to_owned(),
      name: None,
    };
  }

  ServerHolder {
    link: server.holder.clone(),
    name: offering.iter().find(|one| one.link == server.holder).and_then(|one| one.name.clone()),
  }
}

/// The servers an owner reaches: their organisation's, the platform's and their own, in that order.
async fn reached(standing: &Standing, owner: &str) -> Result<(Vec<MailServer>, Vec<Offering>), MailFault> {
  let offering = standing.organisation.offering(owner).await?;
  let holders: Vec<String> =
    offering.iter().map(|one| one.link.clone()).chain([own_holder(owner)]).collect();

  Ok((standing.servers.held_by(&holders, None).await?, offering))
}

/// A server the owner reaches, or a refusal as one they do not, for an account to stand on.
pub async fn reached_server(standing: &Standing, owner: &str, id: i64) -> Result<MailServer, MailFault> {
  let unknown = || MailFault::from(Asked::rule("unknown-server", "the owner reaches no server of that id"));
  let server = standing.servers.get(id).await?.ok_or_else(unknown)?;

  if server.owner() == Some(owner) {
    return Ok(server);
  }

  let (servers, _) = reached(standing, owner).await?;

  servers.into_iter().find(|one| one.id == id).ok_or_else(unknown)
}

/// How the owner stands to a kept server: whether they may read it and whether they may change it. A server of
/// another owner, or of a link they neither reach nor may read, is none.
async fn standing_to(
  standing: &Standing,
  owner: &str,
  server: &MailServer,
) -> Result<Option<bool>, MailFault> {
  if let Some(holder) = server.owner() {
    return Ok((holder == owner).then_some(true));
  }

  match standing.organisation.writable(&server.holder, owner).await {
    Ok(writable) => Ok(Some(writable)),
    Err(MailFault::Asked(asked)) if asked.status == StatusCode::FORBIDDEN => {
      let (servers, _) = reached(standing, owner).await?;

      Ok(servers.iter().any(|one| one.id == server.id).then_some(false))
    }
    Err(fault) => Err(fault),
  }
}

/// The kept server the path names, as the owner may read it, and whether they may change it.
async fn named_server(call: &mut Call) -> Result<(String, MailServer, bool), Refused> {
  let owner = call.owner()?;
  let id: i64 = call
    .key("serverId")
    .await?
    .parse()
    .map_err(|_| Refused::not_found("no server has that id"))?;
  let standing = call.standing();
  let server = standing
    .servers
    .get(id)
    .await?
    .ok_or_else(|| Refused::not_found("no server has that id"))?;
  let may_change = standing_to(standing, &owner, &server)
    .await?
    .ok_or_else(|| Refused::not_found("no server has that id"))?;

  Ok((owner, server, may_change))
}

/// The holder a link names for a change, its servers written on the chain in the owner's name; none for the
/// owner's own.
async fn write_link(standing: &Standing, holder: &str, owner: &str, ids: &[i64]) -> Result<(), MailFault> {
  if holder.starts_with("owner:") {
    return Ok(());
  }

  standing.organisation.write(holder, owner, ids).await
}

/// `GET /servers`.
pub struct ListServers;

impl Route for ListServers {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/servers";
  const OPERATION: Operation = Operation {
    id: "listServers",
    summary: "The servers the owner reaches, or the servers a link of the delegation chain holds.",
    tag: "servers",
    keys: &[],
    query: Some(inline::<ServerQuery>),
    body: None,
    status: 200,
    answer: Answer::Json(schema::<Page<Server>>),
    codes: &["forbidden", "not-ready", "upstream-failed"],
    guard: Guard::Screen,
  };

  async fn answer(&self, call: Call) -> Result<Response, Refused> {
    let owner = call.owner()?;
    let query: ServerQuery = call.query()?;
    let standing = call.standing();
    let mut items = Vec::new();

    match query.link.as_deref() {
      None => {
        let (servers, offering) = reached(standing, &owner).await?;

        for server in servers
          .iter()
          .filter(|one| query.direction.is_none_or(|direction| one.protocol().direction() == direction))
        {
          let mine = server.owner().is_some();

          items.push(member(standing, server, holder_of(server, &offering), mine).await?);
        }
      }
      Some(link) => {
        let link =
          chain_link(link).ok_or_else(|| Refused::bad_request("a link is platform or unit:<id>"))?;
        let writable = standing.organisation.writable(&link, &owner).await?;

        for server in standing.servers.held_by(std::slice::from_ref(&link), query.direction).await? {
          let holder = ServerHolder {
            link: link.clone(),
            name: None,
          };

          items.push(member(standing, &server, holder, writable).await?);
        }
      }
    }

    Ok(answer(StatusCode::OK, &Page { items, more: false }))
  }
}

crate::route!(ListServers);

/// `POST /servers`.
pub struct CreateServer;

impl Route for CreateServer {
  const METHOD: Method = Method::POST;
  const PATH: &'static str = "/servers";
  const OPERATION: Operation = Operation {
    id: "createServers",
    summary: "Keep a server of the owner's own, or of a link the owner may write on the delegation chain.",
    tag: "servers",
    keys: &[],
    query: None,
    body: Some(schema::<NewServer>),
    status: 201,
    answer: Answer::Json(schema::<Server>),
    codes: &["forbidden", "locked", "not-found", "not-ready", "upstream-failed"],
    guard: Guard::Screen,
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let owner = call.owner()?;
    let new: NewServer = call.body().await?;
    let standing = call.standing();
    let draft = ServerDraft::checked(
      new.protocol,
      ServerFields {
        name: new.name,
        host: Some(new.host),
        port: new.port,
        security: new.security,
        tls_name: new.tls_name,
        auth: new.auth,
      },
    )
    .map_err(MailFault::from)?;
    let holder = match new.link.as_deref() {
      None => own_holder(&owner),
      Some(link) => {
        chain_link(link).ok_or_else(|| Refused::bad_request("a link is platform or unit:<id>"))?
      }
    };
    let mut transaction = standing.servers.pool().begin().await.map_err(MailFault::from)?;
    let id = keep(&mut transaction, &holder, &draft).await?;
    let ids = ids_held(&mut transaction, &holder).await?;

    write_link(standing, &holder, &owner, &ids).await?;
    transaction.commit().await.map_err(MailFault::from)?;

    let server = standing
      .servers
      .get(id)
      .await?
      .ok_or_else(|| Refused::failed("the kept server could not be read"))?;
    let holder = holder_of(&server, &[]);

    Ok(answer(StatusCode::CREATED, &member(standing, &server, holder, true).await?))
  }
}

crate::route!(CreateServer);

/// `GET /servers/{serverId}`.
pub struct ReadServer;

impl Route for ReadServer {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/servers/{serverId}";
  const OPERATION: Operation = Operation {
    id: "readServersByServerId",
    summary: "One server the owner reaches or may read.",
    tag: "servers",
    keys: &[SERVER],
    query: None,
    body: None,
    status: 200,
    answer: Answer::Json(schema::<Server>),
    codes: &["not-found", "upstream-failed"],
    guard: Guard::Screen,
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let (_, server, may_change) = named_server(&mut call).await?;
    let holder = holder_of(&server, &[]);

    Ok(answer(StatusCode::OK, &member(call.standing(), &server, holder, may_change).await?))
  }
}

crate::route!(ReadServer);

/// `PATCH /servers/{serverId}`: its settings changed, reaching every account on it at its next connection.
pub struct ChangeServer;

impl Route for ChangeServer {
  const METHOD: Method = Method::PATCH;
  const PATH: &'static str = "/servers/{serverId}";
  const OPERATION: Operation = Operation {
    id: "changeServersByServerId",
    summary: "Change a server's settings; its protocol stays.",
    tag: "servers",
    keys: &[SERVER],
    query: None,
    body: Some(schema::<ServerPatch>),
    status: 200,
    answer: Answer::Json(schema::<Server>),
    codes: &["not-found", "forbidden", "locked", "upstream-failed"],
    guard: Guard::Screen,
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let patch: ServerPatch = call.body().await?;
    let (owner, server, may_change) = named_server(&mut call).await?;

    if !may_change {
      return Err(Refused::forbidden("the owner may not change the server"));
    }

    let mut fields = server.draft.fields();

    if patch.name.is_some() {
      fields.name = patch.name;
    }
    if patch.host.is_some() {
      fields.host = patch.host;
    }
    if patch.port.is_some() {
      fields.port = patch.port;
    }
    if patch.security.is_some() {
      fields.security = patch.security;
    }
    if patch.tls_name.is_some() {
      fields.tls_name = patch.tls_name;
    }
    if patch.auth.is_some() {
      fields.auth = patch.auth;
    }

    let draft = ServerDraft::checked(server.protocol(), fields).map_err(MailFault::from)?;
    let standing = call.standing();
    let mut transaction = standing.servers.pool().begin().await.map_err(MailFault::from)?;

    change(&mut transaction, server.id, &draft).await?;

    let ids = ids_held(&mut transaction, &server.holder).await?;

    write_link(standing, &server.holder, &owner, &ids).await?;
    transaction.commit().await.map_err(MailFault::from)?;
    standing.accounts.touch();

    let changed = standing
      .servers
      .get(server.id)
      .await?
      .ok_or_else(|| Refused::not_found("no server has that id"))?;
    let holder = holder_of(&changed, &[]);

    Ok(answer(StatusCode::OK, &member(standing, &changed, holder, true).await?))
  }
}

crate::route!(ChangeServer);

/// `DELETE /servers/{serverId}`: refused while an account stands on it.
pub struct DeleteServer;

impl Route for DeleteServer {
  const METHOD: Method = Method::DELETE;
  const PATH: &'static str = "/servers/{serverId}";
  const OPERATION: Operation = Operation {
    id: "deleteServersByServerId",
    summary: "Remove a server no account stands on.",
    tag: "servers",
    keys: &[SERVER],
    query: None,
    body: None,
    status: 204,
    answer: Answer::Nothing,
    codes: &["not-found", "forbidden", "locked", "server-in-use", "upstream-failed"],
    guard: Guard::Screen,
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let (owner, server, may_change) = named_server(&mut call).await?;

    if !may_change {
      return Err(Refused::forbidden("the owner may not remove the server"));
    }

    let standing = call.standing();
    let mut transaction = standing.servers.pool().begin().await.map_err(MailFault::from)?;

    if account_count(&mut transaction, server.id).await? > 0 {
      return Err(Refused::new(StatusCode::CONFLICT, "server-in-use", "an account stands on the server"));
    }

    remove(&mut transaction, server.id).await?;

    let ids = ids_held(&mut transaction, &server.holder).await?;

    write_link(standing, &server.holder, &owner, &ids).await?;
    transaction.commit().await.map_err(MailFault::from)?;

    Ok(nothing())
  }
}

crate::route!(DeleteServer);

/// `POST /server-tests`: a connection test of a kept server or of a form's fields; nothing is kept.
pub struct CreateServerTest;

impl Route for CreateServerTest {
  const METHOD: Method = Method::POST;
  const PATH: &'static str = "/server-tests";
  const OPERATION: Operation = Operation {
    id: "createServerTests",
    summary: "Test a connection to a server, signing in where a login and a password are given.",
    tag: "servers",
    keys: &[],
    query: None,
    body: Some(schema::<NewServerTest>),
    status: 200,
    answer: Answer::Json(schema::<ServerTest>),
    codes: &["not-found", "upstream-failed"],
    guard: Guard::Screen,
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let owner = call.owner()?;
    let asked: NewServerTest = call.body().await?;
    let standing = call.standing();
    let draft = match asked.server_id {
      Some(id) => {
        let server = standing
          .servers
          .get(id)
          .await?
          .ok_or_else(|| Refused::not_found("no server has that id"))?;

        standing_to(standing, &owner, &server)
          .await?
          .ok_or_else(|| Refused::not_found("no server has that id"))?;
        server.draft
      }
      None => {
        let protocol = asked
          .protocol
          .ok_or_else(|| Refused::bad_request("a test names a server or its protocol"))?;

        ServerDraft::checked(
          protocol,
          ServerFields {
            host: asked.host,
            port: asked.port,
            security: asked.security,
            tls_name: asked.tls_name,
            ..ServerFields::default()
          },
        )
        .map_err(MailFault::from)?
      }
    };
    let login = match (asked.login, asked.password) {
      (Some(login), Some(password)) if !login.trim().is_empty() && !password.is_empty() => {
        Some(MailAccount {
          address: login.trim().to_owned(),
          login: login.trim().to_owned(),
          password,
        })
      }
      _ => None,
    };
    let tested = probe::test(draft.protocol, &draft.endpoint(), login.as_ref()).await;

    Ok(answer(StatusCode::OK, &tested))
  }
}

crate::route!(CreateServerTest);

/// The platform's outgoing servers, as the account service chooses the system mail's.
pub async fn platform_outgoing(standing: &Standing) -> Result<OutgoingServers, MailFault> {
  let servers = standing
    .servers
    .held_by(&[crate::servers::PLATFORM.to_owned()], Some(crate::wire::Direction::Outgoing))
    .await?;

  Ok(OutgoingServers {
    items: servers
      .into_iter()
      .map(|server| crate::wire::OutgoingServer {
        id: server.id,
        name: server.draft.name,
        host: server.draft.host,
        port: server.draft.port,
        security: server.draft.security,
        auth: server.draft.auth,
      })
      .collect(),
  })
}
