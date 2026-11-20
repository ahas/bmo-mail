//! The Outlook road (`docs/mail.md`, 「A person's sent mail, read on their own machine」): a PowerShell read run
//! on the person's node through the node service, which attaches to the classic Outlook already running there and
//! never starts one, and answers the day's sent mail items compressed and encoded as plain characters.

use std::collections::BTreeMap;
use std::io::Read;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use flate2::read::GzDecoder;
use serde::Deserialize;

use super::{Day, SentMessage};
use crate::clients::node::{NodeService, RunAsk};
use crate::settings::Upstream;

/// The variable the day's first moment is handed in, in epoch milliseconds.
pub const FROM_VARIABLE: &str = "BMO_MAIL_FROM";
/// The variable the next day's first moment is handed in, in epoch milliseconds.
pub const TO_VARIABLE: &str = "BMO_MAIL_TO";
/// The shell the read runs in on a Windows host.
pub const SHELL: &str = "powershell";

/// The read. Windows PowerShell's extra type data on arrays is dropped first, since with it `ConvertTo-Json`
/// writes an array as an object holding its items and its count. Where no Outlook process runs it answers so without asking for Outlook's object, since asking for
/// it where none is registered would start one. Attached, it walks the default mailbox's sent folder newest
/// first, from the day's end back to its start, and answers each mail item (class 43) with its recipients, an
/// Exchange recipient by the SMTP address its entry holds (`PR_SMTP_ADDRESS`) where one is held.
pub const SCRIPT: &str = r#"$ErrorActionPreference = 'Stop'
Remove-TypeData System.Array -ErrorAction SilentlyContinue
function Send-Answer($answer) {
  $json = ConvertTo-Json -InputObject $answer -Depth 6 -Compress
  $bytes = [Text.Encoding]::UTF8.GetBytes($json)
  $packed = New-Object IO.MemoryStream
  $gzip = New-Object IO.Compression.GZipStream($packed, [IO.Compression.CompressionMode]::Compress)
  $gzip.Write($bytes, 0, $bytes.Length)
  $gzip.Close()
  [Console]::Out.Write([Convert]::ToBase64String($packed.ToArray()))
}
if (-not (Get-Process -Name OUTLOOK -ErrorAction SilentlyContinue)) {
  Send-Answer @{ standing = $false; reason = 'no Outlook is running' }
  exit 0
}
try {
  $outlook = [Runtime.InteropServices.Marshal]::GetActiveObject('Outlook.Application')
} catch {
  Send-Answer @{ standing = $false; reason = "the running Outlook could not be attached to: $($_.Exception.Message)" }
  exit 0
}
$from = [DateTimeOffset]::FromUnixTimeMilliseconds([int64]$env:BMO_MAIL_FROM).UtcDateTime
$to = [DateTimeOffset]::FromUnixTimeMilliseconds([int64]$env:BMO_MAIL_TO).UtcDateTime
$kinds = @{ 1 = 'to'; 2 = 'cc'; 3 = 'bcc' }
$items = $outlook.Session.GetDefaultFolder(5).Items
$items.Sort('[SentOn]', $true)
$messages = New-Object Collections.ArrayList
foreach ($item in $items) {
  if ($item.Class -ne 43) { continue }
  $sent = $item.SentOn.ToUniversalTime()
  if ($sent -ge $to) { continue }
  if ($sent -lt $from) { break }
  $recipients = New-Object Collections.ArrayList
  foreach ($recipient in $item.Recipients) {
    $address = $recipient.Address
    try {
      $smtp = $recipient.PropertyAccessor.GetProperty('http://schemas.microsoft.com/mapi/proptag/0x39FE001E')
      if ($smtp) { $address = $smtp }
    } catch {}
    $kind = $kinds[[int]$recipient.Type]
    if (-not $kind) { $kind = 'to' }
    [void]$recipients.Add(@{ name = [string]$recipient.Name; address = $address; kind = $kind })
  }
  [void]$messages.Add(@{
    subject = [string]$item.Subject
    sentAt = ([DateTimeOffset]$sent).ToUnixTimeMilliseconds()
    recipients = $recipients.ToArray()
  })
}
Send-Answer @{ standing = $true; messages = $messages.ToArray() }
"#;

/// What the read answers: the messages where Outlook stood, or why it did not.
#[derive(Deserialize)]
struct Answer {
  standing: bool,
  #[serde(default)]
  reason: Option<String>,
  #[serde(default)]
  messages: Vec<SentMessage>,
}

/// The read's answer out of its standard output: base64 of the gzip of its JSON.
fn unpack(stdout: &str) -> Result<Answer, String> {
  let packed = STANDARD
    .decode(stdout.trim())
    .map_err(|why| format!("the read's answer is not base64: {why}"))?;
  let mut json = Vec::new();

  GzDecoder::new(packed.as_slice())
    .read_to_end(&mut json)
    .map_err(|why| format!("the read's answer does not unpack: {why}"))?;

  serde_json::from_slice(&json).map_err(|why| format!("the read's answer does not read: {why}"))
}

/// The day's sent mail as the running Outlook on `node` holds it, read for `account`; why the road does not
/// stand otherwise.
pub async fn read(
  upstream: &Upstream,
  http: &reqwest::Client,
  node: &str,
  account: &str,
  day: &Day,
) -> Result<Vec<SentMessage>, String> {
  let service = NodeService::new(upstream, http.clone()).map_err(|why| why.to_string())?;
  let ask = RunAsk {
    account,
    command: SCRIPT,
    shell: SHELL,
    env: BTreeMap::from([
      (FROM_VARIABLE, day.from.timestamp_millis().to_string()),
      (TO_VARIABLE, day.to.timestamp_millis().to_string()),
    ]),
  };
  let run = service.run(node, &ask).await.map_err(|why| why.to_string())?;

  if run.exit_code != Some(0) {
    let last = run
      .stderr
      .lines()
      .rev()
      .find(|line| !line.trim().is_empty())
      .unwrap_or("nothing on standard error");
    let code = run.exit_code.map_or_else(|| "with no code".to_owned(), |code| format!("with {code}"));

    return Err(format!("the read on {node} ended {code}: {}", last.trim()));
  }
  if run.stdout_cut {
    return Err(format!("the read's answer on {node} was cut at the hub's ceiling"));
  }

  let answer = unpack(&run.stdout)?;

  if answer.standing {
    Ok(answer.messages)
  } else {
    Err(answer.reason.unwrap_or_else(|| "the read said Outlook does not stand".to_owned()))
  }
}
