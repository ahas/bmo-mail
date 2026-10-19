//! The event stream (`docs/api.md`, 「The event stream」): what changed and what arrived, broadcast to every
//! consumer holding `GET /events` open. Nothing is kept for a consumer that was not listening; it reads the
//! summary when it opens the stream again.

use std::convert::Infallible;
use std::time::Duration;

use axum::http::Method;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures::StreamExt;
use serde_json::Value;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;

use crate::http::{Call, Refused, Route};

// How many events a slow consumer may fall behind before it misses the oldest of them.
const BACKLOG: usize = 256;
// How long an idle stream waits before it carries a comment line, so a proxy holds it open.
const IDLE: Duration = Duration::from_secs(15);

/// One event: its name and its data.
#[derive(Clone, Debug)]
pub struct Published {
  pub name: &'static str,
  pub data: Value,
}

/// The broadcast every event goes out on.
#[derive(Debug)]
pub struct Events {
  sender: broadcast::Sender<Published>,
}

impl Default for Events {
  fn default() -> Self {
    Self {
      sender: broadcast::channel(BACKLOG).0,
    }
  }
}

impl Events {
  /// Says `name` with `data` to every consumer listening now; with none listening it is said to nobody.
  pub fn publish(&self, name: &'static str, data: Value) {
    let _ = self.sender.send(Published { name, data });
  }

  fn subscribe(&self) -> broadcast::Receiver<Published> {
    self.sender.subscribe()
  }
}

/// `GET /events`: the stream itself.
pub struct EventStream;

impl Route for EventStream {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/events";

  async fn answer(&self, call: Call) -> Result<Response, Refused> {
    let stream =
      BroadcastStream::new(call.standing().events.subscribe()).filter_map(|published| async move {
        // A consumer that fell behind misses what it fell behind by, and reads the summary as it would on
        // opening the stream again.
        let published = published.ok()?;

        Some(Ok::<Event, Infallible>(
          Event::default().event(published.name).data(published.data.to_string()),
        ))
      });

    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(IDLE)).into_response())
  }
}

crate::route!(EventStream);
