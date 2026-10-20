//! The mail service: the mailboxes an accounts file names, read, written and watched over IMAP and
//! submission behind one API, with an event stream saying when an inbox changed and what arrived
//! (`docs/mail.md`, `docs/api.md`).

pub mod account_settings;
pub mod accounts;
pub mod announcements;
pub mod avatars;
pub mod client;
pub mod events;
pub mod health;
pub mod http;
pub mod imap;
pub mod keys;
pub mod labels;
pub mod mail;
pub mod mcp;
pub mod members;
pub mod openapi;
pub mod routes;
pub mod rules;
pub mod settings;
pub mod store;
pub mod watcher;
pub mod wire;
pub mod wording;
pub mod writing;

use std::io;
use std::sync::Arc;

use tokio::net::TcpListener;

use crate::http::Standing;
use crate::settings::Settings;
use crate::store::Store;
use crate::wording::Wording;

/// The nouns a host dispatches to this command: none, since it carries no noun of its own.
pub const NOUNS: [&str; 0] = [];

/// Opens the store, starts the watchers, then answers the routes on the settings' address until the process
/// is told to stop, and lets the watchers and every account's connection go.
pub async fn serve(settings: Settings) -> Result<(), String> {
  let store = Store::open(&settings.store.path).await.map_err(|why| why.to_string())?;
  let listen = settings.server.listen;
  let wording = Wording::load(&settings).map_err(|why| why.to_string())?;
  let standing = Arc::new(Standing::new(settings, store, wording));
  let router = http::router(Arc::clone(&standing)).map_err(|faults| faults.join("; "))?;
  let listener = TcpListener::bind(listen)
    .await
    .map_err(|why| format!("{listen} could not be listened on: {why}"))?;
  let (stop, stopping) = tokio::sync::watch::channel(false);
  let watching = tokio::spawn(watcher::watch(Arc::clone(&standing), stopping));
  let served = axum::serve(listener, router)
    .with_graceful_shutdown(stopped())
    .await
    .map_err(|why: io::Error| why.to_string());

  let _ = stop.send(true);
  let _ = watching.await;
  standing.sessions.close_all().await;

  served
}

/// Resolves once the process is told to stop: an interrupt, or the service manager's terminate.
async fn stopped() {
  let interrupt = async {
    let _ = tokio::signal::ctrl_c().await;
  };
  let terminate = async {
    match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
      Ok(mut signal) => {
        signal.recv().await;
      }
      Err(_) => std::future::pending().await,
    }
  };

  tokio::select! {
    () = interrupt => {}
    () = terminate => {}
  }
}
