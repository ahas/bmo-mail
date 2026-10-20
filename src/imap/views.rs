//! What a page of messages reads (`docs/mail.md`, 「Reading」, 「Search」): the folders a view stands over and
//! what it asks of each of them.

use sqlx::SqlitePool;

use crate::imap::folders::{folders_of, role_folders, searched_folders};
use crate::imap::messages::{Cursor, messages_page};
use crate::imap::search::{Search, all_of, parse_mail_query};
use crate::imap::session::MailSession;
use crate::labels::{label_by_name, label_keyword, list_labels};
use crate::mail::{FolderRole, MailFolder, MailMessage, MailView};
use crate::routes::requests::{Asked, MailFault};
use crate::wire::MessageQuery;

/// How many messages a page holds where none is asked, and the most it may.
pub const PAGE_SIZE: u32 = 50;
pub const MOST_PAGE_SIZE: u32 = 100;

/// The folders `in:` names: a role (`spam` being the junk role's everyday name), `starred`, or a folder by
/// its name or path. The second answer says whether the scope holds starred mail alone.
fn scoped_folders(folders: &[MailFolder], scope: &str) -> (Vec<MailFolder>, bool) {
  let role = if scope == "spam" { "junk" } else { scope };

  if let Some(role) = FolderRole::named(role) {
    return (role_folders(folders, role), false);
  }

  if scope == "starred" {
    return (searched_folders(folders), true);
  }

  let named = folders
    .iter()
    .filter(|folder| folder.path.to_lowercase() == scope || folder.name.to_lowercase() == scope)
    .cloned()
    .collect();

  (named, false)
}

/// The folders a view stands over and what it asks of each of them.
pub struct ViewScope {
  pub folders: Vec<MailFolder>,
  pub search: Search,
}

/// The one folder a view is held to, or a 404 where the mailbox holds none of that name.
fn held_to(folders: &[MailFolder], path: &str) -> Result<Vec<MailFolder>, MailFault> {
  let found: Vec<MailFolder> = folders.iter().filter(|folder| folder.path == path).cloned().collect();

  if found.is_empty() {
    return Err(Asked::not_found("the mailbox holds no folder of that name").into());
  }

  Ok(found)
}

/// The scope of a view. A search that names no message at all — a label the account does not have — has no
/// scope rather than an empty one, so a reader of this is never left searching every folder for nothing.
pub async fn view_scope(
  pool: &SqlitePool,
  folders: &[MailFolder],
  account: &str,
  view: &MailView,
) -> Result<Option<ViewScope>, MailFault> {
  let (query, held) = match view {
    MailView::Role(role) => {
      let found = role_folders(folders, *role);

      return Ok((!found.is_empty()).then_some(ViewScope {
        folders: found,
        search: Search::All,
      }));
    }
    MailView::Folder(path) => {
      return Ok(Some(ViewScope {
        folders: held_to(folders, path)?,
        search: Search::All,
      }));
    }
    MailView::Label { id, folder } => {
      if !list_labels(pool, account).await?.iter().any(|label| &label.id == id) {
        return Err(Asked::not_found("no label of the account has that id").into());
      }

      return Ok(Some(ViewScope {
        folders: match folder {
          Some(path) => held_to(folders, path)?,
          None => searched_folders(folders),
        },
        search: Search::Keyword(label_keyword(id)),
      }));
    }
    MailView::Search { query, folder } => (parse_mail_query(query), folder),
  };
  let mut terms = query.terms;
  let mut scope = searched_folders(folders);

  if let Some(named) = &query.scope {
    let (scoped, starred) = scoped_folders(folders, named);

    scope = scoped;

    if starred {
      terms.push(Search::Flagged(true));
    }
  }

  if let Some(path) = held {
    scope = held_to(folders, path)?;
  }

  for name in &query.labels {
    let Some(label) = label_by_name(pool, account, name).await? else {
      return Ok(None);
    };

    terms.push(Search::Keyword(label_keyword(&label.id)));
  }

  Ok(Some(ViewScope {
    folders: scope,
    search: all_of(terms),
  }))
}

/// The view a page's query asks for: words, a label, a folder, or the inbox.
fn view_of(query: &MessageQuery) -> MailView {
  let folder = query.folder_name.clone();

  match (&query.q, &query.label_id) {
    (Some(words), _) if !words.trim().is_empty() => MailView::Search {
      query: words.clone(),
      folder,
    },
    (_, Some(id)) => MailView::Label {
      id: id.clone(),
      folder,
    },
    _ => match folder {
      Some(path) => MailView::Folder(path),
      None => MailView::Role(FolderRole::Inbox),
    },
  }
}

/// One page of the messages a query asks for, without their bodies, and whether more stand past it.
pub async fn page_of(
  pool: &SqlitePool,
  session: &MailSession,
  account: &str,
  query: &MessageQuery,
) -> Result<(Vec<MailMessage>, bool), MailFault> {
  let cursor = match (query.before, query.after) {
    (Some(_), Some(_)) => return Err(Asked::bad_request("a page takes before or after, not both").into()),
    (Some(before), None) => Cursor::Before(before),
    (None, Some(after)) => Cursor::After(after),
    (None, None) => Cursor::Newest,
  };
  let limit = query.limit.unwrap_or(PAGE_SIZE).clamp(1, MOST_PAGE_SIZE) as usize;
  let folders = folders_of(session).await?;
  let view = view_of(query);
  let Some(mut scope) = view_scope(pool, &folders, account, &view).await? else {
    return Ok((Vec::new(), false));
  };

  // Words and a label together are the words searched within the label.
  if let (MailView::Search { .. }, Some(id)) = (&view, &query.label_id) {
    if !list_labels(pool, account).await?.iter().any(|label| &label.id == id) {
      return Err(Asked::not_found("no label of the account has that id").into());
    }

    scope.search = all_of(vec![scope.search, Search::Keyword(label_keyword(id))]);
  }

  messages_page(session, &scope.folders, &scope.search, cursor, limit).await
}
