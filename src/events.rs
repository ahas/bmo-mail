//! The event stream (`docs/api.md`, 「The event stream」): each event numbered and kept in the store for a day,
//! at most 10,000, so a reader resumes where it stopped; served as server-sent events, or as a page to a
//! reader asking for JSON. Each event is an owner's, and a reader reads the events of the owner key it carries
//! alone.

use std::convert::Infallible;
use std::time::Duration;

use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::sse::{Event as Sent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures::{StreamExt, stream};
use serde::Serialize;
use serde_json::Value;
use sqlx::SqlitePool;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;

use crate::http::{Answer, Call, GONE, Operation, Refused, Route, answer, inline, schema};
use crate::wire::{Event, EventQuery, Page};

// How many live events a slow reader may fall behind before it misses the oldest of them; it resumes from the
// store when it reconnects.
const BACKLOG: usize = 256;
// How long an idle stream waits before it carries a comment line, so a proxy holds it open.
const IDLE: Duration = Duration::from_secs(15);
// The window of events the store keeps: a day, and at most this many.
const KEPT_MS: i64 = 24 * 60 * 60 * 1000;
const KEPT_COUNT: i64 = 10_000;
// A page's size where none is asked, and the most one holds.
const PAGE: u32 = 100;
const MOST_PAGE: u32 = 1_000;

/// An event, its owner and the account it is of, as the broadcast carries it.
#[derive(Clone, Debug)]
struct Published {
  event: Event,
  owner: String,
  account: Option<String>,
}

/// The stream's window in the store and the broadcast of what is published now.
#[derive(Debug)]
pub struct Events {
  pool: SqlitePool,
  sender: broadcast::Sender<Published>,
}

/// What a reader narrows the stream to: its owner's events, and of those the kinds and the account it asks.
struct Narrowing {
  owner: String,
  kinds: Option<Vec<String>>,
  account: Option<String>,
}

impl Narrowing {
  fn of(owner: String, query: &EventQuery) -> Self {
    Self {
      owner,
      kinds: query.kinds.as_deref().map(|kinds| {
        kinds
          .split(',')
          .map(str::trim)
          .filter(|kind| !kind.is_empty())
          .map(str::to_owned)
          .collect()
      }),
      account: query.account_name.as_deref().map(str::to_lowercase),
    }
  }

  fn admits(&self, owner: Option<&str>, kind: &str, account: Option<&str>) -> bool {
    let kind_admitted = self.kinds.as_ref().is_none_or(|kinds| kinds.iter().any(|one| one == kind));
    let account_admitted = self.account.as_deref().is_none_or(|wanted| account == Some(wanted));

    owner == Some(self.owner.as_str()) && kind_admitted && account_admitted
  }
}

/// A row of the event window: its number, kind, owner, account, time and data.
type EventRow = (i64, String, Option<String>, Option<String>, i64, String);

/// Why a reader cannot resume where it asked.
struct Gone;

impl Events {
  pub fn new(pool: SqlitePool) -> Self {
    Self {
      pool,
      sender: broadcast::channel(BACKLOG).0,
    }
  }

  /// Publishes an event of `kind` about `data`, the member it is of, under the owner and the account it
  /// belongs to. An event that cannot be kept is said in the log and published to nobody, since a reader
  /// resuming would never find it.
  pub async fn publish(
    &self,
    kind: &'static str,
    owner: &str,
    account: Option<&str>,
    data: &impl Serialize,
  ) {
    let data = serde_json::to_value(data).unwrap_or(Value::Null);
    let at = chrono::Utc::now().timestamp_millis();
    let kept = sqlx::query_scalar::<_, i64>(
      "INSERT INTO events (kind, owner, account, at, data) VALUES (?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(kind)
    .bind(owner)
    .bind(account)
    .bind(at)
    .bind(data.to_string())
    .fetch_one(&self.pool)
    .await;
    let id = match kept {
      Ok(id) => id,
      Err(why) => {
        eprintln!("mail: an event could not be kept: {why}");
        return;
      }
    };
    let trimmed = sqlx::query("DELETE FROM events WHERE at < ? OR id <= ?")
      .bind(at - KEPT_MS)
      .bind(id - KEPT_COUNT)
      .execute(&self.pool)
      .await;

    if let Err(why) = trimmed {
      eprintln!("mail: the event window could not be trimmed: {why}");
    }

    let _ = self.sender.send(Published {
      event: Event {
        id,
        kind: kind.to_owned(),
        at,
        data,
      },
      owner: owner.to_owned(),
      account: account.map(str::to_owned),
    });
  }

  /// The events after `after` the narrowing admits, from the oldest kept where it names none, at most
  /// `limit`, and whether more stand past them; gone where the window no longer holds the events right after
  /// `after`.
  async fn read(
    &self,
    after: Option<i64>,
    narrowing: &Narrowing,
    limit: u32,
  ) -> Result<Result<(Vec<Event>, bool), Gone>, sqlx::Error> {
    let first: Option<i64> = sqlx::query_scalar("SELECT MIN(id) FROM events").fetch_one(&self.pool).await?;
    let last: Option<i64> = sqlx::query_scalar("SELECT seq FROM sqlite_sequence WHERE name = 'events'")
      .fetch_optional(&self.pool)
      .await?;
    let first_kept = first.unwrap_or(last.unwrap_or(0) + 1);
    let after = match after {
      Some(after) if after + 1 < first_kept => return Ok(Err(Gone)),
      Some(after) => after,
      None => first_kept - 1,
    };

    let rows: Vec<EventRow> = sqlx::query_as(
      "SELECT id, kind, owner, account, at, data FROM events WHERE id > ? AND owner = ? ORDER BY id",
    )
    .bind(after)
    .bind(&narrowing.owner)
    .fetch_all(&self.pool)
    .await?;
    let mut admitted = rows
      .into_iter()
      .filter(|(_, kind, owner, account, _, _)| {
        narrowing.admits(owner.as_deref(), kind, account.as_deref())
      })
      .map(|(id, kind, _, _, at, data)| Event {
        id,
        kind,
        at,
        data: serde_json::from_str(&data).unwrap_or(Value::Null),
      });
    let events: Vec<Event> = admitted.by_ref().take(limit as usize).collect();
    let more = admitted.next().is_some();

    Ok(Ok((events, more)))
  }
}

/// `GET /events`: the stream, or a page of it.
pub struct EventStream;

/// Where a reader resumes: `Last-Event-ID` where it sent one, else `after`, else from now on.
fn resumed_after(headers: &HeaderMap, query: &EventQuery) -> Option<i64> {
  headers
    .get("last-event-id")
    .and_then(|value| value.to_str().ok())
    .and_then(|value| value.trim().parse::<i64>().ok())
    .or(query.after)
}

fn sent(event: &Event) -> Result<Sent, Infallible> {
  let data = serde_json::to_string(event).unwrap_or_default();

  Ok(Sent::default().event(event.kind.clone()).id(event.id.to_string()).data(data))
}

fn gone() -> Refused {
  Refused::new(StatusCode::GONE, GONE, "the events after that number are no longer kept")
}

impl Route for EventStream {
  const METHOD: Method = Method::GET;
  const PATH: &'static str = "/events";
  const OPERATION: Operation = Operation {
    id: "listEvents",
    summary: "The service's one event stream (SSE), or a page of it with Accept: application/json.",
    tag: "events",
    keys: &[],
    query: Some(inline::<EventQuery>),
    body: None,
    status: 200,
    answer: Answer::Events(schema::<Page<Event>>),
    codes: &["gone"],
    owned: true,
  };

  async fn answer(&self, call: Call) -> Result<Response, Refused> {
    let query: EventQuery = call.query()?;
    let headers = call.request.headers();
    let narrowing = Narrowing::of(call.owner()?, &query);
    let events = &call.standing().events;
    let paged = headers
      .get(header::ACCEPT)
      .and_then(|value| value.to_str().ok())
      .is_some_and(|accept| accept.contains("application/json"));

    if paged {
      let limit = query.limit.unwrap_or(PAGE).clamp(1, MOST_PAGE);
      let read = events.read(query.after, &narrowing, limit).await.map_err(Refused::failed)?;
      let (items, more) = read.map_err(|Gone| gone())?;

      return Ok(answer(StatusCode::OK, &Page { items, more }));
    }

    // The live events are listened to before the window is read, so none published between the two is lost;
    // one read in both is sent once.
    let live = BroadcastStream::new(events.sender.subscribe());
    let backlog = match resumed_after(headers, &query) {
      None => Vec::new(),
      Some(after) => {
        let read = events.read(Some(after), &narrowing, u32::MAX).await.map_err(Refused::failed)?;

        read.map_err(|Gone| gone())?.0
      }
    };
    let mut last = backlog.last().map_or(i64::MIN, |event| event.id);
    let live = live.filter_map(move |published| {
      let admitted = published
        .ok()
        .filter(|published| published.event.id > last)
        .filter(|published| {
          narrowing.admits(Some(&published.owner), &published.event.kind, published.account.as_deref())
        })
        .map(|published| {
          last = published.event.id;
          sent(&published.event)
        });

      async move { admitted }
    });
    let stream = stream::iter(backlog.iter().map(sent).collect::<Vec<_>>()).chain(live);

    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(IDLE)).into_response())
  }
}

crate::route!(EventStream);
