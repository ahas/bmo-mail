//! The mail service: each owner's own mailboxes, read, written and watched through the adapter of each
//! account's kind behind one API, with an event stream saying when an inbox changed and what arrived
//! (`docs/mail.md`, `docs/api.md`).

pub mod account_settings;
pub mod accounts;
pub mod adapters;
pub mod announcements;
pub mod avatars;
pub mod client;
pub mod clients;
pub mod console;
pub mod events;
pub mod health;
pub mod http;
pub mod imap;
pub mod import;
pub mod keys;
pub mod labels;
pub mod mail;
pub mod mcp;
pub mod members;
pub mod openapi;
pub mod pop3;
pub mod routes;
pub mod rules;
pub mod secrets;
pub mod sent_mail;
pub mod servers;
pub mod settings;
pub mod store;
pub mod watcher;
pub mod wire;
pub mod wording;
pub mod writing;

use std::sync::Arc;

use tokio::net::TcpListener;

use crate::accounts::import::{Imported, import_accounts_file};
use crate::http::Standing;
use crate::secrets::Secrets;
use crate::settings::Settings;
use crate::store::Store;
use crate::wording::Wording;

/// The version the mail service's health, its MCP server and its command name: the set's version the build
/// was made at, which the testbed hands the build as `BMO_VERSION`, else the manifest's placeholder
/// (bmo-chat's `docs/conventions.md`, 「Versions」).
pub const VERSION: &str = match option_env!("BMO_VERSION") {
  Some(version) => version,
  None => env!("CARGO_PKG_VERSION"),
};

/// The nouns a host dispatches to this command: none, since it carries no noun of its own.
pub const NOUNS: [&str; 0] = [];

/// The key the settings' key file holds; none where they name none. A key file named but unreadable stops
/// the start.
fn secrets_of(settings: &Settings) -> Result<Option<Secrets>, String> {
  settings
    .secrets
    .key_file
    .as_deref()
    .map(Secrets::read)
    .transpose()
    .map_err(|why| why.to_string())
}

/// Opens the store and the key, imports the accounts file where it waits, starts the watchers, then answers the
/// routes on the settings' address until the process is told to stop; then ends the event streams, waits for
/// the calls in flight, and lets the watchers and every account's connection go.
pub async fn serve(settings: Settings) -> Result<(), String> {
  let store = Store::open(&settings.store.path).await.map_err(|why| why.to_string())?;
  let listen = settings.server.listen;
  let wording = Wording::load(&settings).map_err(|why| why.to_string())?;
  let secrets = secrets_of(&settings)?;
  let standing = Arc::new(Standing::new(settings, store, wording, secrets));

  if !standing.accounts.sealing() && standing.accounts.count().await.map_err(|why| format!("{why:?}"))? > 0
  {
    return Err("the store holds accounts and the settings name no key file to open them with".to_owned());
  }

  if standing.accounts.all().await.is_err() {
    return Err("the accounts in the store do not open under the key file's key".to_owned());
  }

  match import_accounts_file(standing.store.pool(), &standing.accounts, &standing.settings).await {
    Ok(Imported::Now(count)) => eprintln!("mail: imported {count} accounts from the accounts file"),
    Ok(Imported::Before) => {}
    Ok(Imported::Waiting(why)) if standing.settings.accounts.file.is_some() => {
      eprintln!("mail: the accounts file waits to be imported: {why}");
    }
    Ok(Imported::Waiting(_)) => {}
    Err(why) => eprintln!("mail: the accounts file could not be imported: {why:?}"),
  }

  let declaring = Arc::clone(&standing);

  tokio::spawn(async move { declaring.organisation.declare().await });

  let router = http::router(Arc::clone(&standing)).map_err(|faults| faults.join("; "))?;
  let listener = TcpListener::bind(listen)
    .await
    .map_err(|why| format!("{listen} could not be listened on: {why}"))?;
  let (stop, stopping) = tokio::sync::watch::channel(false);
  let watching = tokio::spawn(watcher::watch(Arc::clone(&standing), stopping));
  let closing = Arc::clone(&standing);
  let served = bmo_app_service::serve_until(listener, router, async move {
    bmo_app_service::shutdown_signal().await;
    closing.events.close();
  })
  .await
  .map_err(|why| why.to_string());

  let _ = stop.send(true);
  let _ = watching.await;
  standing.mailboxes.close_all().await;

  served
}

