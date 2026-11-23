//! One POP3 session (`docs/mail.md`, 「A POP3 account」): opened over TLS from its first byte, or plain where the
//! account's TLS name is `-`, signed in with `USER` and `PASS`, and ended with `QUIT`. The service never sends
//! `DELE`, so the end of a session leaves the maildrop as it was. The POP3 client reads every answer.

use async_pop::Client;
use async_pop::request::Request;
use async_pop::response::Response;
use async_pop::response::types::DataType;
use async_pop::response::uidl::UidlResponse;

use crate::adapters::servers::{MailAccount, MailEndpoint, MailStream};
use crate::routes::requests::MailFault;

/// One message of the maildrop as a session lists it: its number in this session, its `UIDL` and its size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
  pub number: usize,
  pub uidl: String,
  pub size: u64,
}

/// A POP3 session, signed in.
pub struct Pop3Session {
  client: Client<Box<dyn MailStream>>,
}

/// Whether an answer was past the most the POP3 client reads in one, 20 MiB.
fn too_large(why: &async_pop::error::Error) -> bool {
  matches!(why.kind(), async_pop::error::ErrorKind::ResponseTooLarge)
}

fn fault(why: async_pop::error::Error) -> MailFault {
  MailFault::server(format!("{why:?}"))
}

/// A command with its arguments, as the POP3 client sends it.
fn command(name: &str, args: &[usize]) -> Result<Request, MailFault> {
  let mut request: Request = name.parse().map_err(fault)?;

  for arg in args {
    request.add_arg(arg);
  }

  Ok(request)
}

/// A multi-line answer with its byte-stuffing undone: the leading dot each line of it was sent with is taken
/// off (RFC 1939, section 3). The POP3 client hands such an answer back with the stuffing in place, and no other
/// library here reads it, so this one step of the framing is undone here.
fn unstuffed(bytes: &[u8]) -> Vec<u8> {
  let mut out = Vec::with_capacity(bytes.len());
  let mut line_start = true;

  for &byte in bytes {
    if line_start && byte == b'.' {
      line_start = false;
      continue;
    }

    line_start = byte == b'\n';
    out.push(byte);
  }

  out
}

/// Opens a session with the account's POP3 server and signs in.
pub async fn open(endpoint: &MailEndpoint, account: &MailAccount) -> Result<Pop3Session, MailFault> {
  let stream = endpoint.connect_secured().await?;
  let mut client = async_pop::new(stream).await.map_err(fault)?;

  client.login(&account.login, &account.password).await.map_err(fault)?;

  Ok(Pop3Session { client })
}

impl Pop3Session {
  /// Every message of the maildrop, in the maildrop's order: `UIDL` for what tells them apart, `LIST` for their
  /// sizes. Both are asked whatever the server's capabilities say, since a server of RFC 1939 alone names none.
  pub async fn listing(&mut self) -> Result<Vec<Listed>, MailFault> {
    let ids = match self.client.send_request(command("UIDL", &[])?).await.map_err(fault)? {
      Response::Uidl(UidlResponse::Multiple(all)) => all
        .items()
        .iter()
        .filter_map(|one| Some((one.index().value().ok()?, one.id().value().ok()?)))
        .collect::<Vec<(usize, String)>>(),
      _ => return Err(MailFault::server("the server did not answer UIDL with a listing")),
    };
    let sizes = match self.client.send_request(command("LIST", &[])?).await.map_err(fault)? {
      Response::List(all) => all
        .items()
        .iter()
        .filter_map(|one| Some((one.counter().value().ok()?, one.size().value().ok()?)))
        .collect::<Vec<(usize, usize)>>(),
      _ => Vec::new(),
    };

    Ok(
      ids
        .into_iter()
        .map(|(number, uidl)| Listed {
          number,
          uidl,
          size: sizes.iter().find(|(listed, _)| *listed == number).map_or(0, |(_, size)| *size as u64),
        })
        .collect(),
    )
  }

  /// A message's head: asked with `TOP`, and read from the whole message where the server refuses `TOP`. None
  /// where the answer is past what the POP3 client reads in one, which leaves the session spent.
  pub async fn head(&mut self, number: usize) -> Result<Option<Vec<u8>>, MailFault> {
    match self.client.send_request(command("TOP", &[number, 0])?).await {
      Ok(Response::Bytes(bytes)) => Ok(Some(unstuffed(&bytes))),
      Err(why) if too_large(&why) => Ok(None),
      _ => self.message(number).await,
    }
  }

  /// A whole message. None where it is past what the POP3 client reads in one answer: the rest of the answer is
  /// still on the wire, so the session is spent and is opened again before it is asked anything more.
  pub async fn message(&mut self, number: usize) -> Result<Option<Vec<u8>>, MailFault> {
    match self.client.retr(number).await {
      Ok(bytes) => Ok(Some(unstuffed(&bytes))),
      Err(why) if too_large(&why) => Ok(None),
      Err(why) => Err(fault(why)),
    }
  }

  /// Ends the session with `QUIT`, which deletes nothing since nothing was marked deleted.
  pub async fn quit(mut self) {
    let _ = self.client.quit().await;
  }
}
