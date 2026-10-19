//! Turning a body part's bytes into text: its transfer encoding undone, then its charset read. A charset
//! the encoding standard knows by another name is read under that name; one nobody can read is read as
//! UTF-8 rather than refused, so a message always shows something.

use base64::Engine as _;
use base64::alphabet::STANDARD;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use encoding_rs::{Encoding, UTF_8};

/// The names mailers give a charset that the encoding standard knows under another: Korean mail still
/// arrives as `ks_c_5601-1987`, which mail clients mean as EUC-KR.
fn alias(label: &str) -> Option<&'static str> {
  match label {
    "ks_c_5601-1987" | "ks_c_5601" | "ks_c_5601-1989" | "cp949" | "x-windows-949" => Some("euc-kr"),
    "unicode-1-1-utf-8" => Some("utf-8"),
    _ => None,
  }
}

/// The charset's label as the encoding standard is asked it.
pub fn charset_label(charset: Option<&str>) -> String {
  let label = charset.unwrap_or("").trim().trim_matches(|c| c == '"' || c == '\'').to_lowercase();

  if label.is_empty() || label == "us-ascii" || label == "ascii" {
    return "utf-8".to_owned();
  }

  alias(&label).map_or(label, str::to_owned)
}

/// The text the bytes are in their charset.
pub fn decode_text(bytes: &[u8], charset: Option<&str>) -> String {
  let encoding = Encoding::for_label(charset_label(charset).as_bytes()).unwrap_or(UTF_8);

  encoding.decode_with_bom_removal(bytes).0.into_owned()
}

/// Base64 that reads whatever a mailer left in it: line breaks and stray characters passed over, and a part
/// cut short — a snippet takes only its first bytes — read as far as its whole groups go.
const LENIENT: GeneralPurpose = GeneralPurpose::new(
  &STANDARD,
  GeneralPurposeConfig::new()
    .with_decode_padding_mode(DecodePaddingMode::Indifferent)
    .with_decode_allow_trailing_bits(true),
);

pub fn decode_base64(bytes: &[u8]) -> Vec<u8> {
  let mut clean: Vec<u8> = bytes
    .iter()
    .copied()
    .filter(|byte| byte.is_ascii_alphanumeric() || *byte == b'+' || *byte == b'/')
    .collect();

  // A last group of one character holds no whole byte.
  if clean.len() % 4 == 1 {
    clean.pop();
  }

  LENIENT.decode(&clean).unwrap_or_default()
}

/// The bytes with their transfer encoding undone.
pub fn decode_transfer(bytes: &[u8], encoding: Option<&str>) -> Vec<u8> {
  match encoding.unwrap_or("").trim().to_lowercase().as_str() {
    "base64" => decode_base64(bytes),
    "quoted-printable" => quoted_printable::decode(bytes, quoted_printable::ParseMode::Robust)
      .unwrap_or_else(|_| bytes.to_vec()),
    _ => bytes.to_vec(),
  }
}

/// One part's text from the bytes the server sent for it.
pub fn part_text(bytes: &[u8], encoding: Option<&str>, charset: Option<&str>) -> String {
  decode_text(&decode_transfer(bytes, encoding), charset)
}
