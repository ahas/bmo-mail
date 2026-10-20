//! Labels (`docs/mail.md`, 「Where each piece of state lives」, 「The mail server」): a label on a message is
//! an IMAP keyword of the service's own form, and the name, colour and order a reader shows it by are the
//! store's.

use rand::RngCore;
use sqlx::SqlitePool;

use crate::imap::folders::folders_of;
use crate::imap::search::Search;
use crate::imap::session::MailSession;
use crate::imap::{store_flags, uid_search};
use crate::routes::requests::{Asked, MailFault};
use crate::wire::Label;

/// The keyword names Maildir keeps per folder in its files; a keyword past them lives only in the server's
/// index and is lost when the index is rebuilt.
const KEYWORDS_PER_FOLDER: usize = 26;

/// The keywords the service sets itself, which count against the folder's keywords as much as a label's.
pub const OWN_KEYWORDS: [&str; 4] = ["$Forwarded", "$Junk", "$NotJunk", "$MDNSent"];

/// The most labels an account may hold.
pub const MAX_LABELS: usize = KEYWORDS_PER_FOLDER - OWN_KEYWORDS.len();

/// What a label's keyword begins with: a keyword is an ASCII atom and a label's name is not. It is the form
/// the labels on the mailboxes are already written in, so it stays as the servers hold it.
const KEYWORD_PREFIX: &str = "bmo_";
/// The longest name a label may have.
const NAME_LIMIT: usize = 60;

/// The keyword a label is carried by.
pub fn label_keyword(id: &str) -> String {
  format!("{KEYWORD_PREFIX}{id}")
}

/// The label a keyword stands for, or none for any other flag: the prefix, then one to sixteen lower-case
/// letters and digits.
pub fn label_id_of(flag: &str) -> Option<&str> {
  let id = flag.strip_prefix(KEYWORD_PREFIX)?;

  (!id.is_empty() && id.len() <= 16 && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()))
    .then_some(id)
}

/// The account's labels, in their order, each at its place in it from 1.
pub async fn list_labels(pool: &SqlitePool, account: &str) -> Result<Vec<Label>, MailFault> {
  let rows: Vec<(String, String, String, i64)> = sqlx::query_as(
    "SELECT id, name, color, position FROM labels WHERE account = ? ORDER BY position, name",
  )
  .bind(account)
  .fetch_all(pool)
  .await?;

  Ok(
    rows
      .into_iter()
      .enumerate()
      .map(|(index, (id, name, colour, _))| Label {
        id,
        name,
        colour,
        position: index as i64 + 1,
      })
      .collect(),
  )
}

fn checked_name(name: &str) -> Result<String, MailFault> {
  let trimmed = name.trim();

  if trimmed.is_empty() || trimmed.encode_utf16().count() > NAME_LIMIT {
    return Err(Asked::bad_request("a label's name is empty or longer than 60 characters").into());
  }

  Ok(trimmed.to_owned())
}

/// A new label, placed after the others.
pub async fn create_label(
  pool: &SqlitePool,
  account: &str,
  name: &str,
  colour: String,
) -> Result<Label, MailFault> {
  let labels = list_labels(pool, account).await?;
  let wanted = checked_name(name)?;

  if labels.len() >= MAX_LABELS {
    return Err(
      Asked::rule("too-many-labels", format!("the account holds the most labels it may, {MAX_LABELS}"))
        .into(),
    );
  }

  if labels.iter().any(|label| label.name == wanted) {
    return Err(Asked::conflict("the account has a label of that name").into());
  }

  let mut id = [0u8; 5];

  rand::rng().fill_bytes(&mut id);

  let label = Label {
    id: id.iter().map(|byte| format!("{byte:02x}")).collect(),
    name: wanted,
    colour,
    position: labels.len() as i64 + 1,
  };

  sqlx::query(
    "INSERT INTO labels (account, id, name, color, position, created_at) VALUES (?, ?, ?, ?, ?, ?)",
  )
  .bind(account)
  .bind(&label.id)
  .bind(&label.name)
  .bind(&label.colour)
  .bind(label.position)
  .bind(chrono::Utc::now().timestamp_millis())
  .execute(pool)
  .await?;

  Ok(label)
}

/// A label renamed, given another colour or moved to another place among the others, which close up around
/// it; the label as it now stands.
pub async fn update_label(
  pool: &SqlitePool,
  account: &str,
  id: &str,
  name: Option<&str>,
  colour: Option<String>,
  position: Option<i64>,
) -> Result<Label, MailFault> {
  let mut labels = list_labels(pool, account).await?;
  let Some(at) = labels.iter().position(|one| one.id == id) else {
    return Err(Asked::not_found("no label of the account has that id").into());
  };
  let name = match name {
    Some(name) => checked_name(name)?,
    None => labels[at].name.clone(),
  };

  if labels.iter().any(|one| one.id != id && one.name == name) {
    return Err(Asked::conflict("the account has a label of that name").into());
  }

  let mut label = labels.remove(at);

  label.name = name;

  if let Some(colour) = colour {
    label.colour = colour;
  }

  let place = position.map_or(at, |position| (position.max(1) as usize - 1).min(labels.len()));

  labels.insert(place, label);

  for (index, one) in labels.iter_mut().enumerate() {
    one.position = index as i64 + 1;

    sqlx::query("UPDATE labels SET name = ?, color = ?, position = ? WHERE account = ? AND id = ?")
      .bind(&one.name)
      .bind(&one.colour)
      .bind(one.position)
      .bind(account)
      .bind(&one.id)
      .execute(pool)
      .await?;
  }

  Ok(labels.swap_remove(place))
}

/// Deletes the label and takes its keyword off every message of every folder that carries it.
pub async fn delete_label(
  pool: &SqlitePool,
  session: &MailSession,
  account: &str,
  id: &str,
) -> Result<(), MailFault> {
  if !list_labels(pool, account).await?.iter().any(|label| label.id == id) {
    return Err(Asked::not_found("no label of the account has that id").into());
  }

  let keyword = label_keyword(id);
  let folders = folders_of(session).await?;

  for folder in folders {
    let mut connection = session.folder(&folder.path).await?;
    let done = async {
      let uids = uid_search(connection.session(), &Search::Keyword(keyword.clone()).command()).await?;

      store_flags(connection.session(), &uids, "-", std::slice::from_ref(&keyword)).await
    }
    .await;

    connection.settle(done)?;
  }

  sqlx::query("DELETE FROM labels WHERE account = ? AND id = ?")
    .bind(account)
    .bind(id)
    .execute(pool)
    .await?;

  Ok(())
}

/// The label of that name in the account, for a search's `label:`.
pub async fn label_by_name(
  pool: &SqlitePool,
  account: &str,
  name: &str,
) -> Result<Option<Label>, MailFault> {
  let wanted = name.trim().to_lowercase();

  Ok(
    list_labels(pool, account)
      .await?
      .into_iter()
      .find(|label| label.name.to_lowercase() == wanted),
  )
}
