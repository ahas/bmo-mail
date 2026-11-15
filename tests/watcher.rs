//! The watcher and the work connection over a dropped connection (`docs/mail.md`, 「Connections and the
//! watcher」): a stand-in IMAP server closes the watch connection while a message arrives and closes the work
//! connection the message's event is read through; the watcher opens its connection again, takes the message
//! above its mark, and the read is asked once more on a connection opened again, so the arrival is said.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bmo_mail::accounts::Keeping;
use bmo_mail::http::Standing;
use bmo_mail::settings::Settings;
use bmo_mail::store::Store;
use bmo_mail::wire::Fields;
use bmo_mail::wording::Wording;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

const HEADER: &str = "From: Someone <someone@example.com>\r\nTo: box@example.com\r\nSubject: Hello\r\n\
                      Message-ID: <one@example.com>\r\nDate: Tue, 27 Oct 2026 12:00:00 +0900\r\n\r\n";
const BODY: &str = "hello";

/// What the stand-in server holds and how it misbehaves: the inbox's UIDs, the SELECT, counted from 1, that is
/// answered by closing the connection, whether the next IDLE closes it and delivers a message as it does, and
/// how many connections were opened.
#[derive(Default)]
struct Server {
  inbox: Vec<u32>,
  selects: usize,
  drop_select: usize,
  drop_idle_delivering: Option<u32>,
  connections: usize,
}

fn scratch(name: &str) -> PathBuf {
  let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
  let folder = std::env::temp_dir().join(format!("bmo-mail-watcher-{}-{name}", std::process::id()));
  let _ = std::fs::remove_dir_all(&folder);

  std::fs::create_dir_all(&folder).expect("the scratch folder is made");

  folder
}

fn settings(folder: &Path) -> Settings {
  let environment = HashMap::from([("BMO_MAIL_SERVER__TOKEN".to_owned(), "test-token".to_owned())]);

  Settings::read(folder, Some(environment)).expect("the settings read")
}

fn fields(pairs: &[(&str, &str)]) -> Fields {
  pairs.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect()
}

async fn write(stream: &mut TcpStream, text: &str) -> bool {
  stream.write_all(text.as_bytes()).await.is_ok()
}

// The section a FETCH asks for, as the server names it in its answer.
fn section(command: &str) -> Option<String> {
  let start = command.find("BODY.PEEK[")? + "BODY.PEEK[".len();
  let end = start + command[start..].find(']')?;
  let partial = command[end + 1..].starts_with("<0.");

  Some(format!("BODY[{}]{}", &command[start..end], if partial { "<0>" } else { "" }))
}

// One connection of the stand-in server, until it closes it or the client goes.
async fn serve_one(mut stream: TcpStream, server: Arc<Mutex<Server>>) {
  server.lock().unwrap().connections += 1;

  if !write(&mut stream, "* OK stand-in ready\r\n").await {
    return;
  }

  let mut reader = BufReader::new(stream);
  let mut line = String::new();

  loop {
    line.clear();

    if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
      return;
    }

    let command = line.trim_end().to_owned();
    let Some((tag, rest)) = command.split_once(' ') else { continue };
    let upper = rest.to_ascii_uppercase();
    let stream = reader.get_mut();
    let answer = if upper.starts_with("CAPABILITY") {
      format!("* CAPABILITY IMAP4rev1 IDLE\r\n{tag} OK done\r\n")
    } else if upper.starts_with("LOGIN") || upper.starts_with("NOOP") {
      format!("{tag} OK done\r\n")
    } else if upper.starts_with("LOGOUT") {
      let _ = write(stream, &format!("* BYE bye\r\n{tag} OK done\r\n")).await;
      return;
    } else if upper.starts_with("SELECT") || upper.starts_with("EXAMINE") {
      let (dropped, exists, next) = {
        let mut held = server.lock().unwrap();

        held.selects += 1;

        let next = held.inbox.iter().max().copied().unwrap_or(0) + 1;

        (held.selects == held.drop_select, held.inbox.len(), next)
      };

      if dropped {
        let _ = write(stream, "* BYE Disconnected for inactivity.\r\n").await;
        return;
      }

      format!(
        "* {exists} EXISTS\r\n* 0 RECENT\r\n* OK [UIDVALIDITY 5] ok\r\n* OK [UIDNEXT {next}] ok\r\n\
         * FLAGS (\\Seen)\r\n{tag} OK [READ-WRITE] done\r\n"
      )
    } else if upper.starts_with("UID SEARCH") {
      let from: u32 = upper
        .split_whitespace()
        .last()
        .and_then(|range| range.split(':').next())
        .and_then(|first| first.parse().ok())
        .unwrap_or(1);
      let found: Vec<String> = {
        let held = server.lock().unwrap();

        held.inbox.iter().filter(|uid| **uid >= from).map(u32::to_string).collect()
      };

      format!("* SEARCH {}\r\n{tag} OK done\r\n", found.join(" "))
    } else if upper.starts_with("UID FETCH") {
      let uid: u32 = upper.split_whitespace().nth(2).and_then(|uid| uid.parse().ok()).unwrap_or(0);
      let number = {
        let held = server.lock().unwrap();

        held.inbox.iter().position(|one| *one == uid).map(|at| at + 1)
      };
      let Some(number) = number else {
        let _ = write(stream, &format!("{tag} OK done\r\n")).await;
        continue;
      };
      let named = section(rest).unwrap_or_else(|| "BODY[HEADER]".to_owned());
      let text = if named.starts_with("BODY[1]") { BODY } else { HEADER };
      let more = if upper.contains("BODYSTRUCTURE") {
        " FLAGS () INTERNALDATE \"27-Oct-2026 12:00:00 +0900\" \
         BODYSTRUCTURE (\"TEXT\" \"PLAIN\" (\"CHARSET\" \"UTF-8\") NIL NIL \"7BIT\" 5 1)"
      } else {
        ""
      };

      format!(
        "* {number} FETCH (UID {uid}{more} {named} {{{}}}\r\n{text})\r\n{tag} OK done\r\n",
        text.len()
      )
    } else if upper.starts_with("LIST") {
      format!("* LIST (\\HasNoChildren) \"/\" INBOX\r\n{tag} OK done\r\n")
    } else if upper.starts_with("STATUS") {
      let count = server.lock().unwrap().inbox.len();

      format!("* STATUS INBOX (MESSAGES {count} UNSEEN {count})\r\n{tag} OK done\r\n")
    } else if upper.starts_with("IDLE") {
      if !write(stream, "+ idling\r\n").await {
        return;
      }

      let delivering = server.lock().unwrap().drop_idle_delivering.take();

      if let Some(uid) = delivering {
        tokio::time::sleep(Duration::from_millis(200)).await;
        server.lock().unwrap().inbox.push(uid);

        let _ = write(reader.get_mut(), "* BYE Disconnected for inactivity.\r\n").await;
        return;
      }

      line.clear();

      if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
        return;
      }

      format!("{tag} OK idle done\r\n")
    } else {
      format!("{tag} BAD not known here\r\n")
    };

    if !write(reader.get_mut(), &answer).await {
      return;
    }
  }
}

async fn stand_in(server: Arc<Mutex<Server>>) -> u16 {
  let listener = TcpListener::bind("127.0.0.1:0").await.expect("the stand-in listens");
  let port = listener.local_addr().expect("the port is known").port();

  tokio::spawn(async move {
    while let Ok((stream, _)) = listener.accept().await {
      tokio::spawn(serve_one(stream, Arc::clone(&server)));
    }
  });

  port
}

#[tokio::test]
async fn a_message_that_arrives_while_both_connections_drop_is_still_said() {
  let folder = scratch("dropped");
  let settings = settings(&folder);
  let store = Store::open(&settings.store.path).await.expect("the store opens");
  let wording = Wording::load(&settings).expect("the words load");
  let secrets = bmo_mail::secrets::Secrets::new(&[7; 32]);
  let standing = Arc::new(Standing::new(settings, store, wording, Some(secrets)));
  // The first watch connection's IDLE closes as message 11 arrives; the third SELECT, the work connection's
  // first, is answered by closing it too.
  let server = Arc::new(Mutex::new(Server {
    inbox: vec![10],
    drop_select: 3,
    drop_idle_delivering: Some(11),
    ..Server::default()
  }));
  let port = stand_in(Arc::clone(&server)).await.to_string();
  let account = standing
    .accounts
    .add(
      "owner-one",
      &Keeping {
        name: "box".to_owned(),
        kind: "imap-smtp".to_owned(),
        address: "box@example.com".to_owned(),
        settings: fields(&[
          ("imapHost", "127.0.0.1"),
          ("imapPort", &port),
          ("imapTlsName", "-"),
          ("smtpHost", "127.0.0.1"),
          ("smtpPort", &port),
          ("smtpTlsName", "-"),
        ]),
        secrets: fields(&[("password", "stand-in")]),
      },
    )
    .await
    .expect("the account is kept");
  let pool = standing.store.pool().clone();

  sqlx::query("INSERT INTO marks (account, folder, uid_validity, last_uid) VALUES (?, 'INBOX', '5', 10)")
    .bind(account.store_key())
    .execute(&pool)
    .await
    .expect("the mark is kept");

  let (stop, stopping) = tokio::sync::watch::channel(false);
  let watching = tokio::spawn(bmo_mail::watcher::watch(Arc::clone(&standing), stopping));
  // The watcher says the arrival first and lets it go after, so the wait ends only once both are done.
  let settled = async {
    loop {
      let kinds: Vec<String> = sqlx::query_scalar("SELECT kind FROM events ORDER BY id")
        .fetch_all(&pool)
        .await
        .expect("the events read");
      let owed: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM arrivals")
        .fetch_one(&pool)
        .await
        .expect("the arrivals count");

      if kinds.iter().any(|kind| kind == "message.created") && owed == 0 {
        return kinds;
      }

      tokio::time::sleep(Duration::from_millis(100)).await;
    }
  };
  let kinds = tokio::time::timeout(Duration::from_secs(30), settled)
    .await
    .expect("the arrival is said and then let go in time");
  let last: i64 = sqlx::query_scalar("SELECT last_uid FROM marks")
    .fetch_one(&pool)
    .await
    .expect("the mark reads");
  let data: String = sqlx::query_scalar("SELECT data FROM events WHERE kind = 'message.created'")
    .fetch_one(&pool)
    .await
    .expect("the event reads");

  let _ = stop.send(true);
  let _ = watching.await;

  assert_eq!(last, 11, "the mark moved past the message");
  assert!(data.contains("Hello"), "the event carries the message: {data}");
  assert!(kinds.contains(&"folder.changed".to_owned()) || kinds.len() == 1, "{kinds:?}");
  assert!(server.lock().unwrap().connections >= 4, "both connections were opened again");
}
