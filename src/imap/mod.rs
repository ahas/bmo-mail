//! The mail server as IMAP speaks it: the connections and what is read and changed through them
//! (`docs/mail.md`, 「The mail server」, 「Reading」).

pub mod charset;
pub mod conversation;
pub mod folders;
pub mod messages;
pub mod moved_in;
pub mod search;
pub mod session;
pub mod structure;
pub mod views;
pub mod watcher;

use async_imap::imap_proto::rfc4315::UidSetMember;
use async_imap::imap_proto::{Response, ResponseCode, Status};
use async_imap::types::{Fetch, Flag};
use futures::TryStreamExt;

use crate::imap::session::ImapSession;
use crate::routes::requests::MailFault;

/// A folder's path as the server names it: modified UTF-7.
pub fn wire_name(path: &str) -> String {
  utf7_imap::encode_utf7_imap(path.to_owned())
}

/// A folder's path as the server names it, read as text.
pub fn path_of(wire: &str) -> String {
  utf7_imap::decode_utf7_imap(wire.to_owned())
}

/// A text as an IMAP quoted string. A line break cannot stand in one and is left out, so a text a request
/// gave can never write past its own string.
pub fn quoted(text: &str) -> String {
  let kept: String = text.chars().filter(|c| *c != '\r' && *c != '\n').collect();

  format!("\"{}\"", kept.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Whether a text is a part number: numbers joined by dots.
pub fn part_number(text: &str) -> bool {
  !text.is_empty()
    && text
      .split('.')
      .all(|number| !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()))
}

/// UIDs as an IMAP sequence set.
pub fn uid_set(uids: &[u32]) -> String {
  uids.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
}

/// A flag as IMAP writes it: a system flag with its backslash, a keyword as it is.
pub fn flag_name(flag: &Flag<'_>) -> String {
  match flag {
    Flag::Seen => "\\Seen".to_owned(),
    Flag::Answered => "\\Answered".to_owned(),
    Flag::Flagged => "\\Flagged".to_owned(),
    Flag::Deleted => "\\Deleted".to_owned(),
    Flag::Draft => "\\Draft".to_owned(),
    Flag::Recent => "\\Recent".to_owned(),
    Flag::MayCreate => "\\*".to_owned(),
    Flag::Custom(name) => name.to_string(),
  }
}

/// The messages a `UID FETCH` answered for. While another client changes the same folder the server may
/// report a flag change unasked in the middle of a fetch, and such a report can come without its UID; it is
/// not one of the messages asked for.
pub async fn uid_fetch(
  session: &mut ImapSession,
  uids: &[u32],
  items: &str,
) -> Result<Vec<Fetch>, MailFault> {
  if uids.is_empty() {
    return Ok(Vec::new());
  }

  let fetched: Vec<Fetch> = session
    .uid_fetch(uid_set(uids), items)
    .await
    .map_err(MailFault::server)?
    .try_collect()
    .await
    .map_err(MailFault::server)?;

  Ok(fetched.into_iter().filter(|fetch| fetch.uid.is_some_and(|uid| uid > 0)).collect())
}

/// The UIDs a `UID SEARCH` found, in no order.
pub async fn uid_search(session: &mut ImapSession, query: &str) -> Result<Vec<u32>, MailFault> {
  Ok(session.uid_search(query).await.map_err(MailFault::server)?.into_iter().collect())
}

/// Sets or clears flags on messages of the selected folder, the server's answer of each passed over.
pub async fn store_flags(
  session: &mut ImapSession,
  uids: &[u32],
  change: &str,
  flags: &[String],
) -> Result<(), MailFault> {
  if uids.is_empty() || flags.is_empty() {
    return Ok(());
  }

  let query = format!("{change}FLAGS.SILENT ({})", flags.join(" "));
  let _: Vec<Fetch> = session
    .uid_store(uid_set(uids), query)
    .await
    .map_err(MailFault::server)?
    .try_collect()
    .await
    .map_err(MailFault::server)?;

  Ok(())
}

/// Removes the messages of the selected folder marked deleted: those named alone where the server has
/// UIDPLUS, every one of the folder where it has not.
pub async fn expunge(session: &mut ImapSession, uids: &[u32], uidplus: bool) -> Result<(), MailFault> {
  if uidplus {
    let _: Vec<u32> = session
      .uid_expunge(uid_set(uids))
      .await
      .map_err(MailFault::server)?
      .try_collect()
      .await
      .map_err(MailFault::server)?;
  } else {
    let _: Vec<u32> = session
      .expunge()
      .await
      .map_err(MailFault::server)?
      .try_collect()
      .await
      .map_err(MailFault::server)?;
  }

  Ok(())
}

/// Where a copy or a move put the messages, from the `COPYUID` the server answered with: the destination's
/// UID validity and the new UIDs.
#[derive(Clone, Debug, Default)]
pub struct Landed {
  pub validity: String,
  pub uids: Vec<u32>,
}

fn uids_of(members: &[UidSetMember]) -> Vec<u32> {
  members
    .iter()
    .flat_map(|member| match member {
      UidSetMember::Uid(uid) => vec![*uid],
      UidSetMember::UidRange(range) => range.clone().collect(),
    })
    .collect()
}

/// Appends a message to a folder and says where it landed where the server said (`APPENDUID`). The IMAP
/// library writes `APPEND` and passes its answer over, so the command is written here as the library writes
/// it: the folder, the flags, the internal date where one is given, and the message as a literal sent once
/// the server asks for it.
pub async fn append_landing(
  session: &mut ImapSession,
  folder: &str,
  flags: &str,
  internal_date: Option<&str>,
  content: &[u8],
) -> Result<Option<Landed>, MailFault> {
  use tokio::io::AsyncWriteExt;

  let date = internal_date.map(|date| format!(" {}", quoted(date))).unwrap_or_default();
  let command = format!("APPEND {} ({flags}){date} {{{}}}", quoted(&wire_name(folder)), content.len());
  let id = session.run_command(&command).await.map_err(MailFault::server)?;
  let asked = session.read_response().await.map_err(MailFault::server)?;

  if !matches!(asked.as_ref().map(|response| response.parsed()), Some(Response::Continue { .. })) {
    return Err(MailFault::server("the mail server refused the message"));
  }

  let stream: &mut Box<dyn crate::adapters::servers::MailStream> = session.as_mut();

  stream.write_all(content).await.map_err(MailFault::server)?;
  stream.write_all(b"\r\n").await.map_err(MailFault::server)?;
  stream.flush().await.map_err(MailFault::server)?;

  loop {
    let Some(response) = session.read_response().await.map_err(MailFault::server)? else {
      return Err(MailFault::server("the mail server closed the connection"));
    };

    if let Response::Done {
      tag,
      status,
      outcome,
    } = response.parsed()
      && *tag == id
    {
      let landed = match &outcome.code {
        Some(ResponseCode::AppendUid(validity, uids)) => Some(Landed {
          validity: validity.to_string(),
          uids: uids_of(uids),
        }),
        _ => None,
      };

      return match status {
        Status::Ok => Ok(landed),
        _ => Err(MailFault::server(format!(
          "APPEND: {}",
          outcome.information.as_deref().unwrap_or("refused")
        ))),
      };
    }
  }
}

/// Runs a command whose answer is read here, and gives where it put messages where the server said
/// (`COPYUID`, tagged or not). Everything else it answered is passed over. A `NO` or a `BAD` is a fault.
pub async fn run_landing(session: &mut ImapSession, command: &str) -> Result<Option<Landed>, MailFault> {
  let id = session.run_command(command).await.map_err(MailFault::server)?;
  let mut landed = None;

  loop {
    let Some(response) = session.read_response().await.map_err(MailFault::server)? else {
      return Err(MailFault::server("the mail server closed the connection"));
    };

    match response.parsed() {
      Response::Done {
        tag,
        status,
        outcome,
      } if *tag == id => {
        if let Some(ResponseCode::CopyUid(validity, _, to)) = &outcome.code {
          landed = Some(Landed {
            validity: validity.to_string(),
            uids: uids_of(to),
          });
        }

        return match status {
          Status::Ok => Ok(landed),
          _ => Err(MailFault::server(format!(
            "{command}: {}",
            outcome.information.as_deref().unwrap_or("refused")
          ))),
        };
      }
      Response::Data { outcome, .. } => {
        if let Some(ResponseCode::CopyUid(validity, _, to)) = &outcome.code {
          landed = Some(Landed {
            validity: validity.to_string(),
            uids: uids_of(to),
          });
        }
      }
      _ => {}
    }
  }
}
