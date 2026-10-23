//! An account's secrets sealed at rest (`docs/mail.md`, 「Accounts and credentials」): AES-256-GCM under the
//! key the key file holds, a fresh random nonce for each sealing and the account's id bound in as associated
//! data, so a sealed secret opens for its own account alone and a copy moved onto another row reads as
//! nothing. The key file holds 32 random bytes written as base64 on one line; it is the deployment's and
//! never the repository's.

use std::fmt;
use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};

/// How long the key is, in bytes.
pub const KEY_LEN: usize = 32;

/// The key the secrets are sealed with.
pub struct Secrets {
  key: LessSafeKey,
  random: SystemRandom,
}

// The key is never written anywhere, a log line included.
impl fmt::Debug for Secrets {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.debug_struct("Secrets").finish_non_exhaustive()
  }
}

/// Why the key file does not give a key, or a sealed secret does not open. Neither names the key nor the
/// secret.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SecretsError {
  #[error("the key file {0} could not be read")]
  Unread(String),
  #[error("the key file {0} does not hold 32 bytes written as base64")]
  Shape(String),
  #[error("a secret could not be sealed")]
  Seal,
  #[error("a sealed secret does not open under this key for this account")]
  Open,
}

/// The associated data a secret is sealed under: the account it is the secret of.
fn bound(account: i64) -> [u8; 8] {
  account.to_be_bytes()
}

impl Secrets {
  /// The key 32 bytes give.
  pub fn new(key: &[u8; KEY_LEN]) -> Self {
    let unbound = UnboundKey::new(&AES_256_GCM, key).expect("a key of 32 bytes is an AES-256-GCM key");

    Self {
      key: LessSafeKey::new(unbound),
      random: SystemRandom::new(),
    }
  }

  /// The key a key file's text gives: 32 bytes as base64, the whitespace around them passed over.
  pub fn from_text(text: &str, named: &str) -> Result<Self, SecretsError> {
    let bytes = STANDARD.decode(text.trim()).map_err(|_| SecretsError::Shape(named.to_owned()))?;
    let key: [u8; KEY_LEN] = bytes.try_into().map_err(|_| SecretsError::Shape(named.to_owned()))?;

    Ok(Self::new(&key))
  }

  /// The key the key file at `path` holds.
  pub fn read(path: &Path) -> Result<Self, SecretsError> {
    let named = path.display().to_string();
    let text = std::fs::read_to_string(path).map_err(|_| SecretsError::Unread(named.clone()))?;

    Self::from_text(&text, &named)
  }

  /// The secret sealed for one account: the nonce, then the sealed bytes with their tag.
  pub fn seal(&self, account: i64, secret: &[u8]) -> Result<Vec<u8>, SecretsError> {
    let mut nonce = [0u8; NONCE_LEN];

    self.random.fill(&mut nonce).map_err(|_| SecretsError::Seal)?;

    let mut sealed = secret.to_vec();

    self
      .key
      .seal_in_place_append_tag(Nonce::assume_unique_for_key(nonce), Aad::from(bound(account)), &mut sealed)
      .map_err(|_| SecretsError::Seal)?;

    Ok([nonce.as_slice(), &sealed].concat())
  }

  /// A secret sealed for one account, opened.
  pub fn open(&self, account: i64, sealed: &[u8]) -> Result<Vec<u8>, SecretsError> {
    if sealed.len() < NONCE_LEN {
      return Err(SecretsError::Open);
    }

    let (nonce, rest) = sealed.split_at(NONCE_LEN);
    let nonce = Nonce::try_assume_unique_for_key(nonce).map_err(|_| SecretsError::Open)?;
    let mut opening = rest.to_vec();
    let opened = self
      .key
      .open_in_place(nonce, Aad::from(bound(account)), &mut opening)
      .map_err(|_| SecretsError::Open)?;

    Ok(opened.to_vec())
  }
}
