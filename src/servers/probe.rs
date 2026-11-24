//! A connection test (`docs/mail.md`, 「Mail servers」): a connection opened to a server with its security, its
//! greeting read, and where a login is given, a sign-in; then let go. It answers whether the server was reached and
//! whether it took the sign-in, never what it was signed in with.

use crate::adapters::servers::{MailAccount, MailEndpoint};
use crate::imap::session::connected as imap_connected;
use crate::pop3::session::{connected as pop3_connected, sign_in_and_quit};
use crate::routes::requests::MailFault;
use crate::wire::{Protocol, ServerTest};
use crate::writing::submission::conversation;

/// The code of a test that reached no server.
const UNREACHABLE: &str = "unreachable";
/// The code of a test whose sign-in the server refused.
const SIGN_IN_FAILED: &str = "sign-in-failed";

/// What one step of a test came to.
enum Step {
  Unreached(MailFault),
  Refused(MailFault),
}

async fn imap(endpoint: &MailEndpoint, login: Option<&MailAccount>) -> Result<(), Step> {
  let client = imap_connected(endpoint).await.map_err(Step::Unreached)?;
  let Some(login) = login else {
    return Ok(());
  };
  let mut session = client
    .login(&login.login, &login.password)
    .await
    .map_err(|(why, _)| Step::Refused(MailFault::server(why)))?;
  let _ = session.logout().await;

  Ok(())
}

async fn pop3(endpoint: &MailEndpoint, login: Option<&MailAccount>) -> Result<(), Step> {
  let client = pop3_connected(endpoint).await.map_err(Step::Unreached)?;

  match login {
    Some(login) => sign_in_and_quit(client, login).await.map_err(Step::Refused),
    None => Ok(()),
  }
}

async fn smtp(endpoint: &MailEndpoint, login: Option<&MailAccount>) -> Result<(), Step> {
  let (client, _) = conversation(endpoint, None).await.map_err(Step::Unreached)?;
  let _ = client.quit().await;

  if let Some(login) = login.filter(|_| endpoint.signs_in()) {
    let (client, _) = conversation(endpoint, Some(login)).await.map_err(Step::Refused)?;
    let _ = client.quit().await;
  }

  Ok(())
}

/// Tests a server of a protocol, signing in as `login` where one is given.
pub async fn test(protocol: Protocol, endpoint: &MailEndpoint, login: Option<&MailAccount>) -> ServerTest {
  let done = match protocol {
    Protocol::Imap => imap(endpoint, login).await,
    Protocol::Pop3 => pop3(endpoint, login).await,
    Protocol::Smtp => smtp(endpoint, login).await,
  };
  let signs_in = login.is_some() && endpoint.signs_in();

  match done {
    Ok(()) => ServerTest {
      reached: true,
      signed_in: signs_in.then_some(true),
      failure: None,
    },
    Err(Step::Unreached(why)) => {
      eprintln!(
        "mail: a connection test of {}:{} reached no server: {why:?}",
        endpoint.host, endpoint.port
      );

      ServerTest {
        reached: false,
        signed_in: signs_in.then_some(false),
        failure: Some(UNREACHABLE.to_owned()),
      }
    }
    Err(Step::Refused(why)) => {
      eprintln!(
        "mail: a connection test of {}:{} was refused its sign-in: {why:?}",
        endpoint.host, endpoint.port
      );

      ServerTest {
        reached: true,
        signed_in: Some(false),
        failure: Some(SIGN_IN_FAILED.to_owned()),
      }
    }
  }
}
