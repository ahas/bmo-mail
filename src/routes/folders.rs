//! An account's folders (`docs/api.md`, 「Folders」): listed with their counts, and marked read or unread
//! whole.

use axum::http::{Method, StatusCode};
use axum::response::Response;

use crate::announcements::folder_now;
use crate::http::{Answer, Call, Operation, Refused, Route, answer, inline, schema};
use crate::imap::messages::{folder_state, folder_states, mark_folder};
use crate::members::folder;
use crate::routes::requests::{Asked, with_account};
use crate::wire::{Folder, FolderPatch, ListQuery, Page};

const ACCOUNT: (&str, &str) = ("accountName", "The account's name: its address's local part.");
const FOLDER: (&str, &str) = ("folderName", "The folder's full name, percent-encoded.");

/// `GET /accounts/{accountName}/folders`.
pub struct Folders;

impl Route for Folders {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/accounts/{accountName}/folders";
  const OPERATION: Operation = Operation {
    id: "listAccountsFoldersByAccountName",
    summary: "An account's folders.",
    tag: "accounts",
    keys: &[ACCOUNT],
    query: Some(inline::<ListQuery>),
    body: None,
    status: 200,
    answer: Answer::Json(schema::<Page<Folder>>),
    codes: &["not-found"],
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let query: ListQuery = call.query()?;

    with_account(&mut call, |mail| async move {
      let folders: Vec<Folder> = folder_states(&mail.session)
        .await?
        .into_iter()
        .enumerate()
        .map(|(index, state)| folder(mail.name(), index, state))
        .collect();

      Ok(answer(StatusCode::OK, &Page::by_position(folders, |folder| folder.position, &query)))
    })
    .await
  }
}

crate::route!(Folders);

/// `PATCH /accounts/{accountName}/folders/{folderName}`: every message of the folder marked read, or every
/// one unread.
pub struct ChangeFolder;

impl Route for ChangeFolder {
  const METHOD: Method = Method::PATCH;
  const PATH: &'static str = "/accounts/{accountName}/folders/{folderName}";
  const OPERATION: Operation = Operation {
    id: "changeAccountsFoldersByAccountNameFolderName",
    summary: "Mark every message of a folder read, or every one unread.",
    tag: "accounts",
    keys: &[ACCOUNT, FOLDER],
    query: None,
    body: Some(schema::<FolderPatch>),
    status: 200,
    answer: Answer::Json(schema::<Folder>),
    codes: &["not-found"],
  };

  async fn answer(&self, mut call: Call) -> Result<Response, Refused> {
    let path = call.key("folderName").await?;
    let patch: FolderPatch = call.body().await?;

    with_account(&mut call, |mail| async move {
      if folder_state(&mail.session, &path).await?.is_none() {
        return Err(Asked::not_found("the mailbox holds no folder of that name").into());
      }

      mark_folder(&mail.session, &path, patch.read).await?;
      folder_now(&mail.standing, &mail.session, mail.name(), &path).await;

      let Some((index, state)) = folder_state(&mail.session, &path).await? else {
        return Err(Asked::not_found("the mailbox holds no folder of that name").into());
      };

      Ok(answer(StatusCode::OK, &folder(mail.name(), index, state)))
    })
    .await
  }
}

crate::route!(ChangeFolder);
