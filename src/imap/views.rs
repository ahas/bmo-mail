//! What a list shows (`docs/mail.md`, 「Reading」, 「Search」): the folders a view stands over, what it asks
//! of each, its page, and marking the whole of it read.

use sqlx::SqlitePool;

use crate::imap::folders::{folders_of, role_folders, searched_folders};
use crate::imap::messages::{NameOrder, folder_page, search_page};
use crate::imap::search::{Search, all_of, parse_mail_query};
use crate::imap::session::MailSession;
use crate::imap::{store_flags, uid_search};
use crate::labels::{label_by_name, label_keyword};
use crate::routes::requests::MailFault;
use crate::wire::{FolderRole, MailFolder, MailPage, MailSort, MailSortBy, MailView};

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

/// The scope of a view. A view that names no message at all — a label the account does not have — has no
/// scope rather than an empty one, so a reader of this is never left searching every folder for nothing.
pub async fn view_scope(
  pool: &SqlitePool,
  folders: &[MailFolder],
  account: &str,
  view: &MailView,
) -> Result<Option<ViewScope>, MailFault> {
  let query = match view {
    MailView::Role(role) => {
      let found = role_folders(folders, *role);

      return Ok((!found.is_empty()).then_some(ViewScope {
        folders: found,
        search: Search::All,
      }));
    }
    MailView::Folder(path) => {
      let found: Vec<MailFolder> = folders.iter().filter(|folder| &folder.path == path).cloned().collect();

      return Ok((!found.is_empty()).then_some(ViewScope {
        folders: found,
        search: Search::All,
      }));
    }
    MailView::Starred => {
      return Ok(Some(ViewScope {
        folders: searched_folders(folders),
        search: Search::Flagged(true),
      }));
    }
    MailView::Label(id) => {
      return Ok(Some(ViewScope {
        folders: searched_folders(folders),
        search: Search::Keyword(label_keyword(id)),
      }));
    }
    MailView::Search(query) => parse_mail_query(query),
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

/// One page of a view.
pub async fn view_page(
  pool: &SqlitePool,
  session: &MailSession,
  account: &str,
  view: &MailView,
  offset: usize,
  sort: MailSort,
  order: &NameOrder,
) -> Result<MailPage, MailFault> {
  let folders = folders_of(session).await?;
  let Some(scope) = view_scope(pool, &folders, account, view).await? else {
    return Ok(MailPage {
      total: 0,
      offset,
      conversations: Vec::new(),
    });
  };
  let one_folder = matches!(view, MailView::Role(_) | MailView::Folder(_)) && scope.folders.len() == 1;

  // The sent role may be two folders; one folder standing in its own arrival order is read by place, two
  // are merged by search. A list ordered by a name or a subject, or holding back what was read, is a search
  // too: neither is the order the folder itself stands in.
  if one_folder && sort.by == MailSortBy::Date && !sort.unread {
    return folder_page(session, &scope.folders[0].path, offset, sort.ascending).await;
  }

  let search = if sort.unread { all_of(vec![scope.search, Search::Seen(false)]) } else { scope.search };

  search_page(session, &scope.folders, &search, offset, sort, order).await
}

/// Every unread message the view holds, marked read where it lies, and how many there were. It is the
/// view's own scope rather than the page on the screen, so a folder read to its end is read to its end and
/// not only as far as the list had been paged.
pub async fn mark_view_read(
  pool: &SqlitePool,
  session: &MailSession,
  account: &str,
  view: &MailView,
) -> Result<usize, MailFault> {
  let folders = folders_of(session).await?;
  let Some(scope) = view_scope(pool, &folders, account, view).await? else {
    return Ok(0);
  };
  let search = all_of(vec![scope.search, Search::Seen(false)]);
  let mut marked = 0;

  for folder in &scope.folders {
    let mut connection = session.folder(&folder.path).await?;
    let done = async {
      let uids = uid_search(connection.session(), &search.command()).await?;

      store_flags(connection.session(), &uids, "+", &["\\Seen".to_owned()]).await?;

      Ok(uids.len())
    }
    .await;

    marked += connection.settle(done)?;
  }

  Ok(marked)
}
