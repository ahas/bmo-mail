//! A POP3 account (`docs/mail.md`, 「A POP3 account」): a stand-in POP3 server on this machine holds the maildrop
//! and a stand-in submission server takes what is sent. The account is added through the routes, its inbox listed
//! from the heads the store keeps, a message read whole with its attachment served by its number, new mail told
//! from seen mail by `UIDL` and said once on the event stream, a message another client took off the server
//! forgotten, a reply written, kept in the store and sent, a read receipt sent, the rules and a label carried on
//! the service's own rows, and an account removed with every row the store held for it. A server of RFC 1939
//! alone, answering neither `CAPA` nor `TOP`, is read all the same; nothing is ever deleted from the maildrop.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bmo_mail::client::{ClientFailure, MailClient};
use bmo_mail::http::{Standing, router};
use bmo_mail::mail::{RuleAction, RuleCondition, RuleField, RuleMatch};
use bmo_mail::secrets::Secrets;
use bmo_mail::settings::Settings;
use bmo_mail::store::Store;
use bmo_mail::wire::{
  DraftPatch, EventQuery, Fields, Mailbox, MessagePatch, MessageQuery, NewAccount, NewDraft, NewLabel,
  NewReceipt, NewRule, NewRuleRun, NewSend, PartKind, Role, RuleMatching, SendState,
};
use bmo_mail::wording::Wording;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

const TOKEN: &str = "test-token";
const OWNER: &str = "owner-one";
const PASSWORD: &str = "stand-in";
/// How often the watcher polls in these tests, so a poll's seconds go and what it checks still holds.
const POLL_MS: &str = "20";

const HELLO: &str = "Received: from relay.example.com by mx.example.com; Tue, 27 Oct 2026 12:00:00 +0900\r\n\
                     From: Someone <someone@example.com>\r\nTo: box@example.com\r\nSubject: Hello\r\n\
                     Message-ID: <hello@example.com>\r\nDate: Tue, 27 Oct 2026 11:59:00 +0900\r\n\r\n\
                     hello there\r\n.a line that starts with a dot\r\n";

const ATTACHED: &str = "Received: from relay.example.com by mx.example.com; Wed, 28 Oct 2026 09:00:00 +0900\r\n\
                        From: Other <other@example.com>\r\nTo: box@example.com\r\nSubject: Notes\r\n\
                        Message-ID: <notes@example.com>\r\nDate: Wed, 28 Oct 2026 08:59:00 +0900\r\n\
                        MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"cut\"\r\n\r\n\
                        --cut\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nsee the notes\r\n\
                        --cut\r\nContent-Type: text/plain; name=\"notes.txt\"\r\n\
                        Content-Disposition: attachment; filename=\"notes.txt\"\r\n\
                        Content-Transfer-Encoding: base64\r\n\r\naGVsbG8gZmlsZQ==\r\n--cut--\r\n";

const FRESH: &str = "Received: from relay.example.com by mx.example.com; Thu, 29 Oct 2026 10:00:00 +0900\r\n\
                     From: Someone <someone@example.com>\r\nTo: box@example.com\r\nSubject: Fresh\r\n\
                     Message-ID: <fresh@example.com>\r\nDate: Thu, 29 Oct 2026 09:59:00 +0900\r\n\r\nnew\r\n";

const ASKING: &str = "Received: from relay.example.com by mx.example.com; Tue, 20 Oct 2026 12:00:00 +0900\r\n\
                      From: Asker <asker@example.com>\r\nTo: box@example.com\r\nSubject: Did it arrive\r\n\
                      Message-ID: <asking@example.com>\r\nDisposition-Notification-To: asker@example.com\r\n\
                      Date: Tue, 20 Oct 2026 11:59:00 +0900\r\n\r\nplease confirm\r\n";

/// What the stand-in POP3 server holds and saw: the maildrop's messages by `UIDL`, whether it speaks RFC 1939
/// alone and refuses `CAPA` and `TOP`, how many sessions signed in, and every `DELE` it was sent.
#[derive(Default)]
struct Drop {
  messages: Vec<(String, String)>,
  bare: bool,
  sessions: usize,
  deletes: usize,
}

/// What the stand-in submission server took: each message's envelope recipients and its bytes.
#[derive(Default)]
struct Submitted {
  messages: Vec<(Vec<String>, String)>,
}

fn scratch(name: &str) -> PathBuf {
  let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
  let folder = std::env::temp_dir().join(format!("bmo-mail-pop3-{}-{name}", std::process::id()));
  let _ = std::fs::remove_dir_all(&folder);
  std::fs::create_dir_all(&folder).expect("the scratch folder is made");

  folder
}

fn settings(folder: &Path) -> Settings {
  let environment = HashMap::from([
    ("BMO_MAIL_SERVER__TOKEN".to_owned(), TOKEN.to_owned()),
    ("BMO_MAIL_POP3__POLL_MS".to_owned(), POLL_MS.to_owned()),
  ]);

  Settings::read(folder, Some(environment)).expect("the settings read")
}

fn fields(pairs: &[(&str, &str)]) -> Fields {
  pairs.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect()
}

async fn write(stream: &mut TcpStream, text: &str) -> bool {
  stream.write_all(text.as_bytes()).await.is_ok()
}

/// A multi-line answer: `+OK`, the lines with a leading dot doubled, and the line of one dot.
fn multi_line(body: &str) -> String {
  let mut answer = "+OK\r\n".to_owned();

  for line in body.split_terminator("\r\n") {
    if line.starts_with('.') {
      answer.push('.');
    }

    answer.push_str(line);
    answer.push_str("\r\n");
  }

  answer.push_str(".\r\n");
  answer
}

/// One POP3 session with the stand-in: the maildrop as it stood when it signed in.
async fn serve_pop3(stream: TcpStream, drop: Arc<Mutex<Drop>>) {
  let mut reader = BufReader::new(stream);
  let mut line = String::new();
  let mut held: Vec<(String, String)> = Vec::new();
  let bare = drop.lock().unwrap().bare;

  if !write(reader.get_mut(), "+OK stand-in ready\r\n").await {
    return;
  }

  loop {
    line.clear();

    if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
      return;
    }

    let words: Vec<&str> = line.split_whitespace().collect();
    let number = |at: usize| words.get(at).and_then(|word| word.parse::<usize>().ok());
    let message =
      |held: &[(String, String)], at: usize| number(at).and_then(|n| held.get(n.wrapping_sub(1))).cloned();
    let answer = match words.first().map(|word| word.to_ascii_uppercase()).as_deref() {
      Some("CAPA" | "TOP") if bare => "-ERR not known here\r\n".to_owned(),
      Some("CAPA") => "+OK\r\nUSER\r\nUIDL\r\nTOP\r\n.\r\n".to_owned(),
      Some("USER") => "+OK\r\n".to_owned(),
      Some("PASS") if words.get(1) == Some(&PASSWORD) => {
        let mut drop = drop.lock().unwrap();

        drop.sessions += 1;
        held = drop.messages.clone();
        "+OK signed in\r\n".to_owned()
      }
      Some("PASS") => "-ERR not that password\r\n".to_owned(),
      Some("UIDL") => {
        let lines: String = held
          .iter()
          .enumerate()
          .map(|(at, (uidl, _))| format!("{} {uidl}\r\n", at + 1))
          .collect();

        format!("+OK\r\n{lines}.\r\n")
      }
      Some("LIST") => {
        let lines: String = held
          .iter()
          .enumerate()
          .map(|(at, (_, body))| format!("{} {}\r\n", at + 1, body.len()))
          .collect();

        format!("+OK\r\n{lines}.\r\n")
      }
      Some("TOP") => match message(&held, 1) {
        Some((_, body)) => multi_line(&format!("{}\r\n", body.split("\r\n\r\n").next().unwrap_or(""))),
        None => "-ERR no such message\r\n".to_owned(),
      },
      Some("RETR") => match message(&held, 1) {
        Some((_, body)) => multi_line(&body),
        None => "-ERR no such message\r\n".to_owned(),
      },
      Some("DELE") => {
        drop.lock().unwrap().deletes += 1;
        "+OK\r\n".to_owned()
      }
      Some("QUIT") => {
        let _ = write(reader.get_mut(), "+OK bye\r\n").await;
        return;
      }
      _ => "-ERR not known here\r\n".to_owned(),
    };

    if !write(reader.get_mut(), &answer).await {
      return;
    }
  }
}

/// One submission conversation with the stand-in: it takes `AUTH PLAIN` and keeps each message it is handed.
async fn serve_smtp(stream: TcpStream, submitted: Arc<Mutex<Submitted>>) {
  let mut reader = BufReader::new(stream);
  let mut line = String::new();
  let mut recipients: Vec<String> = Vec::new();

  if !write(reader.get_mut(), "220 stand-in ready\r\n").await {
    return;
  }

  loop {
    line.clear();

    if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
      return;
    }

    let upper = line.to_ascii_uppercase();
    let answer = if upper.starts_with("EHLO") {
      "250-stand-in\r\n250-AUTH PLAIN\r\n250 SIZE 10000000\r\n".to_owned()
    } else if upper.starts_with("AUTH PLAIN") {
      "235 accepted\r\n".to_owned()
    } else if upper.starts_with("MAIL FROM") {
      recipients.clear();
      "250 ok\r\n".to_owned()
    } else if upper.starts_with("RCPT TO") {
      recipients.push(line[8..].trim().trim_start_matches(':').trim().trim_matches(['<', '>']).to_owned());
      "250 ok\r\n".to_owned()
    } else if upper.starts_with("DATA") {
      if !write(reader.get_mut(), "354 go on\r\n").await {
        return;
      }

      let mut data = Vec::new();

      while !data.ends_with(b"\r\n.\r\n") {
        let mut byte = [0u8; 1];

        if reader.read_exact(&mut byte).await.is_err() {
          return;
        }

        data.push(byte[0]);
      }

      submitted
        .lock()
        .unwrap()
        .messages
        .push((recipients.clone(), String::from_utf8_lossy(&data).into_owned()));
      "250 taken\r\n".to_owned()
    } else if upper.starts_with("QUIT") {
      let _ = write(reader.get_mut(), "221 bye\r\n").await;
      return;
    } else {
      "250 ok\r\n".to_owned()
    };

    if !write(reader.get_mut(), &answer).await {
      return;
    }
  }
}

/// The two stand-ins on ports of their own, served until the test's runtime ends.
async fn stand_ins(drop: Arc<Mutex<Drop>>, submitted: Arc<Mutex<Submitted>>) -> (u16, u16) {
  let pop3 = TcpListener::bind("127.0.0.1:0").await.expect("the POP3 stand-in listens");
  let smtp = TcpListener::bind("127.0.0.1:0").await.expect("the submission stand-in listens");
  let ports = (
    pop3.local_addr().expect("the port is known").port(),
    smtp.local_addr().expect("the port is known").port(),
  );

  tokio::spawn(async move {
    while let Ok((stream, _)) = pop3.accept().await {
      tokio::spawn(serve_pop3(stream, Arc::clone(&drop)));
    }
  });
  tokio::spawn(async move {
    while let Ok((stream, _)) = smtp.accept().await {
      tokio::spawn(serve_smtp(stream, Arc::clone(&submitted)));
    }
  });

  ports
}

/// The service answering on a port of its own, its client acting for the owner, and the stand-ins holding the
/// given maildrop; the account `box` added through the routes, signing in to both stand-ins.
struct Fixture {
  standing: Arc<Standing>,
  client: MailClient,
  drop: Arc<Mutex<Drop>>,
  submitted: Arc<Mutex<Submitted>>,
}

async fn fixture(name: &str, messages: &[(&str, &str)], bare: bool) -> Fixture {
  let folder = scratch(name);
  let settings = settings(&folder);
  let store = Store::open(&settings.store.path).await.expect("the store opens");
  let wording = Wording::load(&settings).expect("the words load");
  let standing = Arc::new(Standing::new(settings, store, wording, Some(Secrets::new(&[7; 32]))));
  let app = router(Arc::clone(&standing)).expect("the routes stand");
  let listener = TcpListener::bind("127.0.0.1:0").await.expect("the service listens");
  let address = format!("http://{}", listener.local_addr().expect("the address is known"));

  tokio::spawn(async move { axum::serve(listener, app).await });

  let drop = Arc::new(Mutex::new(Drop {
    messages: messages.iter().map(|(uidl, body)| ((*uidl).to_owned(), (*body).to_owned())).collect(),
    bare,
    ..Drop::default()
  }));
  let submitted = Arc::new(Mutex::new(Submitted::default()));
  let (pop3, smtp) = stand_ins(Arc::clone(&drop), Arc::clone(&submitted)).await;
  let client = MailClient::new(address, TOKEN).acting_for(OWNER, Role::Member);
  let (pop3, smtp) = (pop3.to_string(), smtp.to_string());

  client
    .create_account(&NewAccount {
      kind: "pop3-smtp".to_owned(),
      name: Some("box".to_owned()),
      address: Some("box@example.com".to_owned()),
      settings: Some(fields(&[
        ("pop3Host", "127.0.0.1"),
        ("pop3Port", &pop3),
        ("pop3TlsName", "-"),
        ("smtpHost", "127.0.0.1"),
        ("smtpPort", &smtp),
        ("smtpTlsName", "-"),
      ])),
      secrets: Some(fields(&[("password", PASSWORD)])),
      authorization: None,
    })
    .await
    .expect("the account signs in to both stand-ins and is kept");

  Fixture {
    standing,
    client,
    drop,
    submitted,
  }
}

/// Waits for a condition the service reaches in its own time, asking again every few milliseconds; the test
/// fails where it is not reached within the bound.
async fn reached<T, F: Future<Output = Option<T>>>(what: &str, mut ask: impl FnMut() -> F) -> T {
  let waiting = async {
    loop {
      if let Some(found) = ask().await {
        return found;
      }

      tokio::time::sleep(Duration::from_millis(20)).await;
    }
  };

  tokio::time::timeout(Duration::from_secs(20), waiting)
    .await
    .unwrap_or_else(|_| panic!("{what} in time"))
}

#[tokio::test]
async fn a_pop3_inbox_is_listed_read_and_its_attachment_served_while_mail_never_moves() {
  let fixture = fixture("read", &[("u-hello", HELLO), ("u-notes", ATTACHED)], false).await;
  let client = &fixture.client;
  let kinds = client.account_kinds().await.expect("the kinds read").items;
  let page = client.messages("box", &MessageQuery::default()).await.expect("the inbox lists");
  let subjects: Vec<&str> = page.items.iter().map(|one| one.subject.as_str()).collect();

  assert_eq!(kinds.iter().find(|kind| kind.name == "pop3-smtp").map(|kind| kind.moves), Some(false));
  assert_eq!(subjects, ["Notes", "Hello"], "newest first by the topmost Received line");
  assert!(page.items.iter().all(|one| one.folder_name == "INBOX" && one.text.is_none()));

  let notes = &page.items[0];
  let read = client.message("box", &notes.id).await.expect("the message reads whole");
  let attachment = read.parts.iter().find(|part| part.kind == PartKind::Attachment).expect("an attachment");
  let bytes = client
    .part_content("box", &notes.id, &attachment.part_id)
    .await
    .expect("the attachment serves");

  assert_eq!(read.text.as_deref().map(str::trim), Some("see the notes"));
  assert_eq!(attachment.file_name.as_deref(), Some("notes.txt"));
  assert_eq!(bytes, b"hello file");

  let hello = client.message("box", &page.items[1].id).await.expect("the plain message reads");

  assert!(
    hello
      .text
      .as_deref()
      .is_some_and(|text| text.contains("\n.a line that starts with a dot"))
  );

  let seen = client
    .change_message(
      "box",
      &notes.id,
      &MessagePatch {
        flags: vec!["\\Seen".to_owned()],
        label_ids: Vec::new(),
        folder_name: None,
      },
    )
    .await
    .expect("the flag is kept")
    .expect("the message stands where it was");
  let moved = client
    .change_message(
      "box",
      &notes.id,
      &MessagePatch {
        flags: vec!["\\Seen".to_owned()],
        label_ids: Vec::new(),
        folder_name: Some("Drafts".to_owned()),
      },
    )
    .await;
  let unread = client
    .messages(
      "box",
      &MessageQuery {
        q: Some("is:unread".to_owned()),
        ..MessageQuery::default()
      },
    )
    .await
    .expect("the unread filter lists");
  let listed_again = client.messages("box", &MessageQuery::default()).await.expect("the inbox lists again");

  assert!(seen.flags.contains(&"\\Seen".to_owned()));
  assert!(
    matches!(&moved, Err(ClientFailure::Answered { status: 422, answer: Some(answer), .. }) if answer.code == "not-offered"),
    "{moved:?}"
  );
  assert_eq!(unread.items.iter().map(|one| one.subject.as_str()).collect::<Vec<_>>(), ["Hello"]);
  assert!(
    listed_again.items[0].parts.iter().any(|part| part.kind == PartKind::Attachment),
    "the parts are kept"
  );

  let label = client
    .create_label(
      "box",
      &NewLabel {
        name: "Notes".to_owned(),
        colour: "#3366cc".to_owned(),
      },
    )
    .await
    .expect("the label is made");

  client
    .create_rule(
      "box",
      &NewRule {
        matching: RuleMatching {
          kind: RuleMatch::All,
          conditions: vec![RuleCondition {
            field: RuleField::Subject,
            header: None,
            contains: "notes".to_owned(),
          }],
        },
        actions: vec![RuleAction::Label {
          label_id: label.id.clone(),
        }],
        on: true,
      },
    )
    .await
    .expect("the rule is made");

  let run = client
    .create_rule_run(
      "box",
      &NewRuleRun {
        folder_name: "INBOX".to_owned(),
      },
    )
    .await
    .expect("the rules run over the inbox");
  let labelled = client.message("box", &notes.id).await.expect("the labelled message reads");

  client.delete_label("box", &label.id).await.expect("the label is deleted");

  let unlabelled = client.message("box", &notes.id).await.expect("the message reads again");

  assert_eq!((run.moved_count, run.labelled_count), (0, 1));
  assert_eq!(labelled.label_ids, std::slice::from_ref(&label.id));
  assert!(unlabelled.label_ids.is_empty(), "the label's keyword is taken off with it");

  client.create_draft("box", &NewDraft::default()).await.expect("a draft is kept");
  client.delete_account("box").await.expect("the account is removed");

  let pool = fixture.standing.store.pool();
  let messages: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pop3_messages")
    .fetch_one(pool)
    .await
    .expect("the rows count");
  let drafts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM drafts")
    .fetch_one(pool)
    .await
    .expect("the drafts count");

  assert_eq!((messages, drafts), (0, 0), "the account's rows go with it");
  assert_eq!(fixture.drop.lock().unwrap().deletes, 0, "nothing is deleted from the maildrop");
}

#[tokio::test]
async fn new_mail_is_told_by_uidl_and_said_once_while_mail_already_there_is_not() {
  let fixture = fixture("watch", &[("u-hello", HELLO)], false).await;
  let pool = fixture.standing.store.pool().clone();
  let (stop, stopping) = tokio::sync::watch::channel(false);
  let watching = tokio::spawn(bmo_mail::watcher::watch(Arc::clone(&fixture.standing), stopping));

  // The first poll sets the mark past what the maildrop holds, handling none of it.
  reached("the first poll sets the mark", || {
    let pool = pool.clone();

    async move {
      sqlx::query_scalar::<_, i64>("SELECT last_uid FROM marks WHERE folder = 'INBOX'")
        .fetch_optional(&pool)
        .await
        .expect("the mark reads")
    }
  })
  .await;
  fixture.drop.lock().unwrap().messages.push(("u-fresh".to_owned(), FRESH.to_owned()));

  let query = EventQuery {
    kinds: Some("message.created".to_owned()),
    ..EventQuery::default()
  };
  let created = reached("the new mail is said", || async {
    let page = fixture.client.events(&query).await.expect("the events read");

    (!page.items.is_empty()).then_some(page.items)
  })
  .await;
  // The watcher says an arrival first and lets it go after, so the wait ends only once both are done.
  let owed: i64 = reached("the arrival is let go", || {
    let pool = pool.clone();

    async move {
      let owed: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM arrivals")
        .fetch_one(&pool)
        .await
        .expect("the arrivals count");

      (owed == 0).then_some(owed)
    }
  })
  .await;
  let sessions_then = fixture.drop.lock().unwrap().sessions;

  reached("another poll passes", || async {
    (fixture.drop.lock().unwrap().sessions > sessions_then + 1).then_some(())
  })
  .await;

  let said_again = fixture.client.events(&query).await.expect("the events read again").items;

  // Another client takes the first message off the server; the next poll forgets it.
  fixture.drop.lock().unwrap().messages.retain(|(uidl, _)| uidl != "u-hello");

  let kept = reached("the message taken off the server is forgotten", || {
    let pool = pool.clone();

    async move {
      let kept: Vec<String> = sqlx::query_scalar("SELECT uidl FROM pop3_messages")
        .fetch_all(&pool)
        .await
        .expect("the rows read");

      (!kept.iter().any(|uidl| uidl == "u-hello")).then_some(kept)
    }
  })
  .await;

  let _ = stop.send(true);
  let _ = watching.await;

  assert_eq!(created.len(), 1, "only the message new since the mark is new mail: {created:?}");
  assert_eq!(created[0].data["subject"], "Fresh");
  assert_eq!(owed, 0, "the arrival is let go once said");
  assert_eq!(said_again.len(), 1, "a message is said once across polls");
  assert_eq!(kept, ["u-fresh"]);
  assert_eq!(fixture.drop.lock().unwrap().deletes, 0, "nothing is deleted from the maildrop");
}

#[tokio::test]
async fn a_reply_is_kept_as_a_draft_in_the_store_and_sent_over_submission() {
  let fixture = fixture("send", &[("u-hello", HELLO), ("u-asking", ASKING)], false).await;
  let client = &fixture.client;
  let hello = client
    .messages("box", &MessageQuery::default())
    .await
    .expect("the inbox lists")
    .items
    .remove(0);
  let draft = client
    .create_draft(
      "box",
      &NewDraft {
        reply_to: Some(hello.id.clone()),
        ..NewDraft::default()
      },
    )
    .await
    .expect("the reply's draft is kept");
  let changed = client
    .change_draft(
      "box",
      &draft.id,
      &DraftPatch {
        to: Some(vec![Mailbox {
          name: None,
          address: "someone@example.com".to_owned(),
        }]),
        body: Some("<p>thanks</p>".to_owned()),
        ..DraftPatch::default()
      },
    )
    .await
    .expect("the draft is saved again");
  let drafts = client
    .messages(
      "box",
      &MessageQuery {
        folder_name: Some("Drafts".to_owned()),
        ..MessageQuery::default()
      },
    )
    .await
    .expect("the drafts folder lists");

  assert_eq!(changed.id, draft.id, "a draft keeps its id across saves");
  assert_eq!(drafts.items.len(), 1);
  assert_eq!(drafts.items[0].subject, "Re: Hello");

  let send = client
    .create_send(
      "box",
      &NewSend {
        draft_id: draft.id.clone(),
        send_at: Some(chrono::Utc::now().timestamp_millis()),
      },
    )
    .await
    .expect("the send is made");
  let ended = reached("the send ends", || async {
    let now = client.send_of("box", &send.id).await.expect("the send reads");

    (now.state != SendState::Waiting).then_some(now)
  })
  .await;
  let submitted = fixture.submitted.lock().unwrap().messages.clone();
  let gone = client.draft("box", &draft.id).await;
  let original = client.message("box", &hello.id).await.expect("the original reads");

  assert_eq!(ended.state, SendState::Sent, "{:?}", ended.failure);
  assert_eq!(submitted.len(), 1);
  assert_eq!(submitted[0].0, ["someone@example.com"]);
  assert!(submitted[0].1.contains("Subject: Re: Hello"), "{}", submitted[0].1);
  assert!(
    matches!(gone, Err(ClientFailure::Answered { status: 404, .. })),
    "the draft is let go once sent"
  );
  assert!(original.flags.contains(&"\\Answered".to_owned()), "the original is marked answered");

  let asking = client
    .messages("box", &MessageQuery::default())
    .await
    .expect("the inbox lists")
    .items
    .into_iter()
    .find(|one| one.subject == "Did it arrive")
    .expect("the message asking for a receipt stands");
  let receipt = client
    .create_receipt(
      "box",
      &NewReceipt {
        message_id: asking.id.clone(),
      },
    )
    .await
    .expect("the receipt is sent");
  let answered = client.message("box", &asking.id).await.expect("the asking message reads");
  let submitted = fixture.submitted.lock().unwrap().messages.clone();

  assert!(receipt.sent);
  assert_eq!(submitted.len(), 2);
  assert_eq!(submitted[1].0, ["asker@example.com"]);
  assert!(
    answered.flags.contains(&"$MDNSent".to_owned()),
    "the receipt is marked on the message's row"
  );
}

#[tokio::test]
async fn a_server_of_rfc_1939_alone_answering_neither_capa_nor_top_is_read_all_the_same() {
  let fixture = fixture("bare", &[("u-hello", HELLO), ("u-notes", ATTACHED)], true).await;
  let client = &fixture.client;
  let page = client
    .messages("box", &MessageQuery::default())
    .await
    .expect("the inbox lists from heads read whole");
  let subjects: Vec<&str> = page.items.iter().map(|one| one.subject.as_str()).collect();
  let notes = client.message("box", &page.items[0].id).await.expect("the message reads whole");

  assert_eq!(subjects, ["Notes", "Hello"]);
  assert_eq!(notes.text.as_deref().map(str::trim), Some("see the notes"));
  assert_eq!(fixture.drop.lock().unwrap().deletes, 0, "nothing is deleted from the maildrop");
}
