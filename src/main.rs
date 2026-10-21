//! `bmo-mail`: the service and its command in one binary (`docs/api.md`, 「The command」).

use std::path::PathBuf;
use std::process::ExitCode;

use bmo_mail::NOUNS;
use bmo_mail::client::MailClient;
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
  /// The harness plugin's MCP server on standard input and output, calling the service through its client.
  Mcp {
    /// The service's address; `http://` and the settings' `server.listen` where none is given.
    #[arg(long, env = "BMO_MAIL_ADDRESS")]
    address: Option<String>,
    /// The service's bearer token; the settings' `server.token` where none is given.
    #[arg(long, env = "BMO_MAIL_TOKEN")]
    token: Option<String>,
  },
  /// The nouns this command carries for a host to dispatch, one a line: none.
  Nouns,
  /// Moves the old archive's seven mail tables from the item's export into this service's store (TODO #1923).
  Import {
    /// The export directory the item's export hand wrote.
    #[arg(long)]
    export: PathBuf,
    /// The store's SQLite file; made and migrated where it does not stand.
    #[arg(long)]
    database: PathBuf,
    /// Reads the export and reports what it would write, writing nothing.
    #[arg(long)]
    dry_run: bool,
  },
}

#[tokio::main]
async fn main() -> ExitCode {
  // Every TLS connection the service opens, to the mail servers and to the open web, runs on one crypto
  // provider, chosen here before any is opened.
  let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();

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

  // The import reads its own arguments and never the deployment's settings.
  if let Word::Import {
    export,
    database,
    dry_run,
  } = &command.word
  {
    return match bmo_mail::import::run(database, export, *dry_run).await {
      Ok(report) => {
        report.lines().iter().for_each(|line| println!("{line}"));
        if report.sample.failures.is_empty() { ExitCode::SUCCESS } else { ExitCode::FAILURE }
      }
      Err(why) => {
        eprintln!("bmo-mail: {why}");
        ExitCode::FAILURE
      }
    };
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
    Word::Mcp { address, token } => {
      let address = address.unwrap_or_else(|| format!("http://{}", settings.server.listen));
      let token = token.unwrap_or_else(|| settings.server.token.clone());

      bmo_mail::mcp::serve(MailClient::new(address, token)).await
    }
    Word::Nouns => Ok(()),
    Word::Import { .. } => unreachable!("the import is answered before the settings are read"),
  };

  match done {
    Ok(()) => ExitCode::SUCCESS,
    Err(why) => {
      eprintln!("bmo-mail: {why}");
      ExitCode::FAILURE
    }
  }
}
