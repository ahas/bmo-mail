//! A POP3 account's watcher (`docs/mail.md`, 「A POP3 account」): it polls. At each interval it takes its turn at
//! a session, and every message the store did not hold before stands above the high-water mark; each of them,
//! oldest first, is read whole once and handled as an IMAP arrival is — rules first, then the mark moved past it
//! and its arrival owed, then its sender recorded — and the arrivals are then said on the event stream. A rule's
//! moves are passed over, since mail on a POP3 account never moves.

use std::sync::Arc;

use tokio::sync::watch;

use crate::accounts::Account;
use crate::adapters::Mailbox;
use crate::announcements::{MESSAGE_CREATED, folder_now, message_now};
use crate::http::Standing;
use crate::imap::messages::heads;
use crate::mail::MailRef;
use crate::pop3::maildrop::{Maildrop, keep_flags, rows};
use crate::pop3::{INBOX, VALIDITY};
use crate::routes::requests::MailFault;
use crate::rules::{message_facts, plan_rules, rule_context};
use crate::watcher::marks::{MailMark, owe, owed, read_mark, settled, write_mark};
use crate::writing::addresses::record_addresses;

/// Resolves once the word to stop is given, holding nothing of it after.
async fn stopped(stop: &mut watch::Receiver<bool>) {
  let _ = stop.wait_for(|stopped| *stopped).await;
}

/// One POP3 account's watcher: the account, its mailbox the events are read through, and its maildrop.
pub struct Pop3Watcher {
  pub standing: Arc<Standing>,
  pub account: Account,
  pub mailbox: Arc<dyn Mailbox>,
  pub maildrop: Arc<Maildrop>,
}

impl Pop3Watcher {
  fn log(&self, line: &str) {
    eprintln!("mail watcher {}: {line}", self.account.address);
  }

  /// One poll: a session's turn, every message above the mark handled, oldest first. A first poll with no mark
  /// sets it past the newest message and handles nothing older. Answers whether any message was handled.
  async fn poll(&self) -> Result<bool, MailFault> {
    let pool = self.standing.store.pool();
    let key = &self.account.store_key();
    let mut turn = self.maildrop.turn().await?;
    let held = rows(pool, key).await?;
    let mut mark = match read_mark(pool, key, INBOX).await? {
      Some(mark) if mark.validity == VALIDITY => mark,
      _ => {
        let mark = MailMark {
          validity: VALIDITY.to_owned(),
          last_uid: held.last().map_or(0, |row| row.id),
        };

        write_mark(pool, key, INBOX, &mark).await?;
        turn.done().await;

        return Ok(false);
      }
    };
    let arrived: Vec<u32> = held.iter().map(|row| row.id).filter(|id| *id > mark.last_uid).collect();

    if arrived.is_empty() {
      turn.done().await;

      return Ok(false);
    }

    let (rules, spam_senders) = rule_context(pool, key).await?;

    for id in arrived {
      mark.last_uid = id;

      let Some((row, _)) = turn.whole(id).await? else {
        write_mark(pool, key, INBOX, &mark).await?;
        continue;
      };
      let plan = plan_rules(&rules, &spam_senders, &message_facts(Some(&row.head)));

      if !plan.flags.is_empty() {
        let mut flags = row.flags.clone();

        flags.extend(plan.flags.into_iter().filter(|flag| !row.flags.contains(flag)));
        keep_flags(pool, key, id, &flags).await?;
      }

      owe(pool, key, INBOX, VALIDITY, id).await?;
      write_mark(pool, key, INBOX, &mark).await?;

      let from = heads(Some(&row.head)).from;

      record_addresses(pool, key, &self.account.address, &from, chrono::Utc::now().timestamp_millis())
        .await?;
    }

    turn.done().await;

    Ok(true)
  }

  /// Says every arrival still owed its event, letting each go once said; one whose message could not be read
  /// stays owed for the next poll.
  async fn say_owed(&self) -> Result<(), MailFault> {
    let pool = self.standing.store.pool();
    let key = &self.account.store_key();

    for uid in owed(pool, key, INBOX, VALIDITY).await? {
      let at = MailRef {
        folder: INBOX.to_owned(),
        uid,
      };

      if message_now(&self.standing, &*self.mailbox, &self.account, MESSAGE_CREATED, &at).await {
        settled(pool, key, INBOX, VALIDITY, uid).await?;
      } else {
        self.log(&format!("message {uid} stays owed its arrival event; it is said at the next poll"));
      }
    }

    Ok(())
  }

  /// One poll and what it owes the event stream.
  async fn round(&self) -> Result<(), MailFault> {
    let took = self.poll().await?;

    self.say_owed().await?;

    if took {
      folder_now(&self.standing, &*self.mailbox, &self.account, INBOX).await;
    }

    Ok(())
  }

  /// Polls at the settings' interval until told to stop, saying in the journal when a poll fails and, once, when
  /// the server answers again.
  pub async fn run(self, mut stop: watch::Receiver<bool>) {
    let every = self.standing.settings.pop3.poll();
    let mut failing = false;

    loop {
      tokio::select! {
        () = stopped(&mut stop) => return,
        done = self.round() => match done {
          Ok(()) if failing => {
            failing = false;
            self.log("the POP3 server answers again");
          }
          Ok(()) => {}
          Err(why) => {
            let why = match why {
              MailFault::Server(why) => why,
              MailFault::Asked(asked) => asked.error,
            };

            failing = true;
            self.log(&format!("could not poll the maildrop ({why}); polling again in {} ms", every.as_millis()));
          }
        }
      }

      tokio::select! {
        () = stopped(&mut stop) => return,
        () = tokio::time::sleep(every) => {}
      }
    }
  }
}
