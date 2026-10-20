//! The folders of one mailbox and the role each plays (`docs/mail.md`, 「The mail server」). A role is found
//! by the special-use mark the server puts on a folder, and by the folder's usual name where the server
//! marks nothing. A role whose folder does not exist yet is made the first time it is needed, under the
//! first of its usual names.

use async_imap::types::NameAttribute;
use futures::TryStreamExt;

use crate::imap::session::{Connection, MailSession};
use crate::imap::{path_of, wire_name};
use crate::mail::{FolderRole, MailFolder};
use crate::routes::requests::MailFault;

impl FolderRole {
  /// The names a folder of the role goes by where nothing marks it, the first being the one a missing
  /// folder is made under.
  fn usual_names(self) -> &'static [&'static str] {
    match self {
      Self::Inbox => &["INBOX"],
      Self::Sent => &["Sent", "Sent Messages", "Sent Items"],
      Self::Drafts => &["Drafts"],
      Self::Junk => &["Junk", "Spam"],
      Self::Trash => &["Trash", "Deleted Messages", "Deleted Items"],
    }
  }

  /// The role a special-use mark gives.
  fn marked(attribute: &NameAttribute<'_>) -> Option<Self> {
    match attribute {
      NameAttribute::Sent => Some(Self::Sent),
      NameAttribute::Drafts => Some(Self::Drafts),
      NameAttribute::Junk => Some(Self::Junk),
      NameAttribute::Trash => Some(Self::Trash),
      NameAttribute::Extension(mark) if mark.eq_ignore_ascii_case("\\Inbox") => Some(Self::Inbox),
      _ => None,
    }
  }
}

/// The role a folder plays: the inbox by its path, else by its mark, else by its usual name.
fn folder_role(path: &str, name: &str, attributes: &[NameAttribute<'_>]) -> Option<FolderRole> {
  if path.eq_ignore_ascii_case("INBOX") {
    return Some(FolderRole::Inbox);
  }

  if let Some(role) = attributes.iter().find_map(FolderRole::marked) {
    return Some(role);
  }

  [FolderRole::Sent, FolderRole::Drafts, FolderRole::Junk, FolderRole::Trash]
    .into_iter()
    .find(|role| role.usual_names().iter().any(|usual| usual.eq_ignore_ascii_case(name)))
}

/// Every folder of the mailbox that can be selected.
pub async fn list_folders(connection: &mut Connection) -> Result<Vec<MailFolder>, MailFault> {
  let names: Vec<_> = connection
    .session()
    .list(Some(""), Some("*"))
    .await
    .map_err(MailFault::server)?
    .try_collect()
    .await
    .map_err(MailFault::server)?;

  Ok(
    names
      .iter()
      .filter(|name| {
        !name.attributes().iter().any(|attribute| {
          matches!(attribute, NameAttribute::NoSelect)
            || matches!(attribute, NameAttribute::Extension(mark) if mark.eq_ignore_ascii_case("\\NonExistent"))
        })
      })
      .map(|name| {
        let path = path_of(name.name());
        let last = match name.delimiter() {
          Some(delimiter) if !delimiter.is_empty() => path.rsplit(delimiter).next().unwrap_or(&path).to_owned(),
          _ => path.clone(),
        };
        let role = folder_role(&path, &last, name.attributes());

        MailFolder { path, name: last, role }
      })
      .collect(),
  )
}

/// Every folder of the mailbox, listed on the account's connection.
pub async fn folders_of(session: &MailSession) -> Result<Vec<MailFolder>, MailFault> {
  let mut connection = session.connection().await?;
  let listed = list_folders(&mut connection).await;

  connection.settle(listed)
}

/// Every folder of the role that exists — the sent role may be two folders, `Sent` and `Sent Messages`,
/// and both are read.
pub fn role_folders(folders: &[MailFolder], role: FolderRole) -> Vec<MailFolder> {
  folders.iter().filter(|folder| folder.role == Some(role)).cloned().collect()
}

/// The folder of the role new mail of that role goes into: the first that exists, or a new one made under
/// the role's first name. It works on whichever connection the caller holds.
pub async fn ensure_role(connection: &mut Connection, role: FolderRole) -> Result<String, MailFault> {
  if let Some(existing) = role_folders(&list_folders(connection).await?, role).into_iter().next() {
    return Ok(existing.path);
  }

  if role == FolderRole::Inbox {
    return Ok("INBOX".to_owned());
  }

  let name = role.usual_names()[0];

  connection.session().create(wire_name(name)).await.map_err(MailFault::server)?;

  Ok(name.to_owned())
}

/// The folder of the role, found or made on the account's connection.
pub async fn ensure_role_folder(session: &MailSession, role: FolderRole) -> Result<String, MailFault> {
  let mut connection = session.connection().await?;
  let ensured = ensure_role(&mut connection, role).await;

  connection.settle(ensured)
}

/// The folders a view across folders reads: every folder but the junk and the trash.
pub fn searched_folders(folders: &[MailFolder]) -> Vec<MailFolder> {
  folders
    .iter()
    .filter(|folder| folder.role != Some(FolderRole::Junk) && folder.role != Some(FolderRole::Trash))
    .cloned()
    .collect()
}
