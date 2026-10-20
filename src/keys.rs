//! The opaque keys a path names a message and a conversation by (`docs/api.md`, 「Keys」). A message's id
//! carries its folder and its UID there, a thread's the conversation's own key, each written in URL-safe
//! base64 so it stands in a path as it is.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

use crate::mail::MailRef;

/// A message's id: its UID and its folder.
pub fn message_id(at: &MailRef) -> String {
  URL_SAFE_NO_PAD.encode(format!("{}:{}", at.uid, at.folder))
}

/// The message an id names, or none for an id that is not one.
pub fn message_ref(id: &str) -> Option<MailRef> {
  let text = String::from_utf8(URL_SAFE_NO_PAD.decode(id).ok()?).ok()?;
  let (uid, folder) = text.split_once(':')?;
  let uid = uid.parse::<u32>().ok().filter(|uid| *uid > 0)?;

  (!folder.is_empty()).then(|| MailRef {
    folder: folder.to_owned(),
    uid,
  })
}

/// A thread's id: the conversation's key.
pub fn thread_id(key: &str) -> String {
  URL_SAFE_NO_PAD.encode(key)
}

/// The conversation's key a thread's id names.
pub fn thread_key(id: &str) -> Option<String> {
  String::from_utf8(URL_SAFE_NO_PAD.decode(id).ok()?).ok().filter(|key| !key.is_empty())
}
