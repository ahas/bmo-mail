//! `bmo-mail`: the service and its command in one binary (`docs/api.md`, 「The command」).

use std::process::ExitCode;

use bmo_mail::NOUNS;
use bmo_mail::settings::Settings;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "bmo-mail", version, about = "Mailboxes read, written and watched behind one API")]
struct Command {
  #[command(subcommand)]
  word: Word,
}

#[derive(Subcommand)]
enum Word {
  /// Opens the store, starts the watchers and answers the routes.
  Serve,
  /// Whether the service the settings name answers ready; ends 0 where it does.
  Health,
  /// The nouns this command carries, one a line, as `bmo` asks a unit command: none.
  Nouns,
}

#[tokio::main]
async fn main() -> ExitCode {
  // Usage the command does not take ends 1; help and the version end 0.
  let command = match Command::try_parse() {
    Ok(command) => command,
    Err(why) => {
      let _ = why.print();
      return if why.use_stderr() { ExitCode::FAILURE } else { ExitCode::SUCCESS };
    }
  };

  if let Word::Nouns = command.word {
    NOUNS.iter().for_each(|noun| println!("{noun}"));
    return ExitCode::SUCCESS;
  }

  let settings = match Settings::load() {
    Ok(settings) => settings,
    Err(why) => {
      eprintln!("bmo-mail: {why}");
      return ExitCode::FAILURE;
    }
  };
  let done = match command.word {
    Word::Serve => bmo_mail::serve(settings).await,
    Word::Health => bmo_mail::health::ask(&settings).await,
    Word::Nouns => Ok(()),
  };

  match done {
    Ok(()) => ExitCode::SUCCESS,
    Err(why) => {
      eprintln!("bmo-mail: {why}");
      ExitCode::FAILURE
    }
  }
}
