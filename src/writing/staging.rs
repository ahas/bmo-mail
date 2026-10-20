//! Attachments uploaded while a message is written (`docs/mail.md`, 「Writing」). A file waits in the
//! staging folder until the draft carrying it is saved; from then on the draft on the server holds it.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use rand::RngCore;

use crate::mail::StagedFile;

/// How long a staged file is kept: a draft saved since holds its own copy, and one never saved was
/// abandoned.
const KEPT: Duration = Duration::from_secs(24 * 60 * 60);

/// What a message's headers, bodies and part boundaries may take besides its attachments. A body written in
/// a composer is far smaller; the margin is what keeps a message that fits by a hair from being refused at
/// the server instead of here.
const MESSAGE_MARGIN: u64 = 1024 * 1024;

/// The longest filename a staged file keeps.
const NAME_LIMIT: usize = 200;

/// A staged file's bytes and what is known of it, by its id: twenty-four hex digits and nothing else.
fn paths(dir: &Path, id: &str) -> Option<(PathBuf, PathBuf)> {
  (id.len() == 24 && id.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()))
    .then(|| (dir.join(id), dir.join(format!("{id}.json"))))
}

/// How many bytes a file of this size takes once base64 has encoded it for a message: four for every three,
/// and a line break every seventy-six.
pub fn encoded_size(bytes: u64) -> u64 {
  let encoded = bytes.div_ceil(3) * 4;

  encoded + encoded.div_ceil(76) * 2
}

/// Whether attachments of these sizes still fit a message the server takes. A server that advertises no
/// limit takes anything.
pub fn attachments_fit(sizes: &[u64], limit: Option<u64>) -> bool {
  match limit {
    None => true,
    Some(limit) => sizes.iter().map(|size| encoded_size(*size)).sum::<u64>() + MESSAGE_MARGIN <= limit,
  }
}

/// Keeps an uploaded file until its draft is saved, and says what it is.
pub fn stage_file(dir: &Path, filename: &str, kind: &str, content: &[u8]) -> std::io::Result<StagedFile> {
  std::fs::create_dir_all(dir)?;

  let mut id = [0u8; 12];

  rand::rng().fill_bytes(&mut id);

  let cleaned: String = filename
    .chars()
    .map(|c| if matches!(c, '\r' | '\n' | '/' | '\\') { '_' } else { c })
    .collect::<Vec<_>>()
    .into_iter()
    .scan(0, |taken, c| {
      *taken += c.len_utf16();
      (*taken <= NAME_LIMIT).then_some(c)
    })
    .collect();
  let file = StagedFile {
    id: id.iter().map(|byte| format!("{byte:02x}")).collect(),
    filename: if cleaned.is_empty() { "attachment".to_owned() } else { cleaned },
    kind: if kind.is_empty() { "application/octet-stream".to_owned() } else { kind.to_owned() },
    size: content.len() as u64,
  };
  let (data, meta) = paths(dir, &file.id).expect("a made id has the shape of one");

  std::fs::write(data, content)?;
  std::fs::write(meta, serde_json::to_vec(&file)?)?;

  Ok(file)
}

/// A staged file with its bytes.
pub struct Staged {
  pub filename: String,
  pub kind: String,
  pub content: Vec<u8>,
}

/// A staged file, or none where it is gone or was never there.
pub fn read_staged(dir: &Path, id: &str) -> Option<Staged> {
  let (data, meta) = paths(dir, id)?;
  let known: serde_json::Value = serde_json::from_slice(&std::fs::read(meta).ok()?).ok()?;

  Some(Staged {
    filename: known.get("filename")?.as_str()?.to_owned(),
    kind: known.get("type")?.as_str()?.to_owned(),
    content: std::fs::read(data).ok()?,
  })
}

/// Removes every staged file older than a day, and says how many.
pub fn sweep_staging(dir: &Path, now: SystemTime) -> usize {
  let Ok(entries) = std::fs::read_dir(dir) else {
    return 0;
  };
  let mut removed = 0;

  for entry in entries.flatten() {
    let old = entry
      .metadata()
      .and_then(|meta| meta.modified())
      .ok()
      .and_then(|modified| now.duration_since(modified).ok())
      .is_some_and(|age| age > KEPT);

    if old && std::fs::remove_file(entry.path()).is_ok() {
      removed += 1;
    }
  }

  removed
}
