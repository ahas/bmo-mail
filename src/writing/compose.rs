//! Writing a message (`docs/mail.md`, 「Writing」): what a composer opens with — the signature, and for a
//! reply or a forward the original quoted under it — and the MIME message a draft or a send is built into.

use std::cell::RefCell;
use std::sync::LazyLock;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use ego_tree::NodeRef;
use lol_html::{RewriteStrSettings, element, rewrite_str};
use mail_builder::MessageBuilder;
use mail_builder::headers::address::Address;
use mail_builder::headers::content_type::ContentType;
use mail_builder::headers::message_id::MessageId;
use mail_builder::headers::text::Text;
use mail_builder::mime::{BodyPart, MimePart};
use rand::RngCore;
use regex::Regex;
use scraper::{Html, Node};

use crate::wire::{AttachmentRef, Draft, DraftKind, MailAddress, MailMessage, MailSettings};
use crate::wording::{Wording, fill};

/// An attachment's bytes, ready to be written into a message.
pub struct ResolvedAttachment {
  pub filename: String,
  pub kind: String,
  pub content: Vec<u8>,
}

/// The header a saved draft carries to say what it was written as. A message that goes out never carries
/// it.
pub const DRAFT_KIND_HEADER: &str = "X-BMO-Draft-Kind";
/// The header a saved draft carries to say what it answers or forwards.
pub const DRAFT_ORIGINAL_HEADER: &str = "X-BMO-Draft-Original";

/// A text as HTML writes it, its markup characters escaped.
pub fn escape_html(text: &str) -> String {
  let mut escaped = String::with_capacity(text.len());

  for c in text.chars() {
    match c {
      '&' => escaped.push_str("&amp;"),
      '<' => escaped.push_str("&lt;"),
      '>' => escaped.push_str("&gt;"),
      '"' => escaped.push_str("&quot;"),
      '\'' => escaped.push_str("&#39;"),
      other => escaped.push(other),
    }
  }

  escaped
}

/// A plain text as HTML: escaped, each line break a `<br>`.
fn text_to_html(text: &str) -> String {
  escape_html(&text.replace("\r\n", "\n")).replace('\n', "<br>")
}

/// A reply's prefix in Korean and English alike, with a count such as `[2]`.
static REPLIED: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"(?i)^\s*(re|답장|회신)\s*(\[\d+\])?\s*:").expect("the reply pattern reads")
});
/// A forward's prefix in Korean and English alike.
static FORWARDED: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"(?i)^\s*(fwd?|전달)\s*:").expect("the forward pattern reads"));

/// `Re:` in front of a subject once, never stacked.
pub fn reply_subject(subject: &str) -> String {
  if REPLIED.is_match(subject) {
    subject.trim().to_owned()
  } else {
    format!("Re: {}", subject.trim())
  }
}

/// `Fwd:` in front of a subject once, never stacked.
pub fn forward_subject(subject: &str) -> String {
  if FORWARDED.is_match(subject) {
    subject.trim().to_owned()
  } else {
    format!("Fwd: {}", subject.trim())
  }
}

/// Whether a text holds markup: an element of its own once read as HTML.
fn holds_markup(text: &str) -> bool {
  let fragment = Html::parse_fragment(text);
  let root = fragment.root_element();

  root.descendants().any(|node| node.id() != root.id() && node.value().is_element())
}

/// The signature under a new message. It is written in the mail editor and so is markup; one written as
/// plain text is taken for text, so it still reads as it was written.
pub fn signature_html(signature: &str) -> String {
  let said = signature.trim();

  if said.is_empty() {
    return String::new();
  }

  let body = if holds_markup(said) {
    said.to_owned()
  } else {
    format!("<p>{}</p>", text_to_html(said))
  };

  format!("<p>-- </p>{body}")
}

fn named(address: &MailAddress) -> String {
  if address.name.is_empty() {
    address.address.clone()
  } else {
    format!("{} <{}>", address.name, address.address)
  }
}

fn named_list(addresses: &[MailAddress]) -> String {
  escape_html(&addresses.iter().map(named).collect::<Vec<_>>().join(", "))
}

fn original_body(original: &MailMessage) -> String {
  original
    .html
    .clone()
    .unwrap_or_else(|| text_to_html(original.text.as_deref().unwrap_or("")))
}

/// A new message: an empty line to write on, the signature under it.
pub fn new_draft(from: &str, settings: &MailSettings) -> Draft {
  Draft {
    from: from.to_owned(),
    to: Vec::new(),
    cc: Vec::new(),
    bcc: Vec::new(),
    subject: String::new(),
    html: format!("<p></p>{}", signature_html(&settings.signature)),
    attachments: Vec::new(),
    receipt: settings.receipt_default,
    kind: DraftKind::New,
    original: None,
    in_reply_to: None,
    references: Vec::new(),
    saved: None,
  }
}

fn same(a: &MailAddress, b: &MailAddress) -> bool {
  a.address.eq_ignore_ascii_case(&b.address)
}

/// The addresses of a list, each once, none of those left out.
fn distinct(list: &[MailAddress], without: &[MailAddress]) -> Vec<MailAddress> {
  let mut kept: Vec<MailAddress> = Vec::new();

  for one in list {
    if !without.iter().any(|other| same(other, one)) && !kept.iter().any(|other| same(other, one)) {
      kept.push(one.clone());
    }
  }

  kept
}

/// The most message IDs a reply's `References` carries.
const REFERENCES_KEPT: usize = 20;

/// A reply, or a reply to all: to the sender (or where the message asks replies to go), the others copied
/// for a reply to all, the account itself never among them. A reply to the account's own message follows it
/// up: to its recipients, with its copies for a reply to all.
pub fn reply_draft(
  original: &MailMessage,
  from: &str,
  all: bool,
  settings: &MailSettings,
  wording: &Wording,
) -> Draft {
  let me = MailAddress {
    name: String::new(),
    address: from.to_owned(),
  };
  let own = original.listed.from.iter().any(|one| same(one, &me));
  let to = if own {
    distinct(&original.listed.to, std::slice::from_ref(&me))
  } else {
    let asked = if original.reply_to.is_empty() { &original.listed.from } else { &original.reply_to };

    distinct(asked, std::slice::from_ref(&me))
  };
  let cc = if all {
    let copied: Vec<MailAddress> = if own {
      original.cc.clone()
    } else {
      original.listed.to.iter().chain(&original.cc).cloned().collect()
    };
    let without: Vec<MailAddress> = std::iter::once(me.clone()).chain(to.iter().cloned()).collect();

    distinct(&copied, &without)
  } else {
    Vec::new()
  };
  let said = fill(
    &wording.reply.attribution,
    &[
      ("date", &wording.date(original.listed.date)),
      ("from", &named_list(&original.listed.from)),
    ],
  );
  let mut references: Vec<String> = original.references.clone();

  references.extend(original.listed.message_id.clone());

  let skip = references.len().saturating_sub(REFERENCES_KEPT);

  Draft {
    to,
    cc,
    subject: reply_subject(&original.listed.subject),
    html: format!(
      "<p></p>{}<p>{said}</p><blockquote>{}</blockquote>",
      signature_html(&settings.signature),
      original_body(original)
    ),
    kind: DraftKind::Reply,
    original: original.listed.message_id.clone(),
    in_reply_to: original.listed.message_id.clone(),
    references: references.into_iter().skip(skip).collect(),
    ..new_draft(from, settings)
  }
}

/// A forward: the original's headers over its body, its attachments carried along.
pub fn forward_draft(
  original: &MailMessage,
  from: &str,
  settings: &MailSettings,
  wording: &Wording,
) -> Draft {
  let words = &wording.forward;
  let mut lines = vec![
    words.header.clone(),
    fill(&words.from, &[("from", &named_list(&original.listed.from))]),
    fill(&words.date, &[("date", &wording.date(original.listed.date))]),
    fill(&words.subject, &[("subject", &escape_html(&original.listed.subject))]),
    fill(&words.to, &[("to", &named_list(&original.listed.to))]),
  ];

  if !original.cc.is_empty() {
    lines.push(fill(&words.cc, &[("cc", &named_list(&original.cc))]));
  }

  Draft {
    subject: forward_subject(&original.listed.subject),
    html: format!(
      "<p></p>{}<div>{}<br><br>{}</div>",
      signature_html(&settings.signature),
      lines.join("<br>"),
      original_body(original)
    ),
    attachments: original
      .attachments
      .iter()
      .map(|part| AttachmentRef::Part {
        folder: original.listed.folder.clone(),
        uid: original.listed.uid,
        part: part.part.clone(),
        filename: part.filename.clone().unwrap_or_else(|| "attachment".to_owned()),
        kind: part.kind.clone(),
        size: part.decoded_size(),
      })
      .collect(),
    kind: DraftKind::Forward,
    original: original.listed.message_id.clone(),
    references: original.listed.message_id.clone().into_iter().collect(),
    ..new_draft(from, settings)
  }
}

/// What a body's plain text never carries of its markup.
const UNREAD_TAGS: [&str; 3] = ["style", "script", "head"];
/// The elements whose end ends a line.
const LINE_ENDS: [&str; 10] = ["p", "div", "li", "h1", "h2", "h3", "h4", "h5", "h6", "tr"];

fn markup_text<'a>(nodes: impl Iterator<Item = NodeRef<'a, Node>>) -> String {
  nodes.map(node_text).collect()
}

fn node_text(node: NodeRef<'_, Node>) -> String {
  match node.value() {
    Node::Text(text) => text.to_string(),
    Node::Element(element) => {
      let name = element.name();

      if UNREAD_TAGS.contains(&name) {
        return String::new();
      }

      if name == "br" {
        return "\n".to_owned();
      }

      // A quote inside a quote is prefixed twice.
      if name == "blockquote" {
        let quoted: Vec<String> = block_text(node.children())
          .split('\n')
          .map(|line| if line.is_empty() { ">".to_owned() } else { format!("> {line}") })
          .collect();

        return format!("\n{}\n", quoted.join("\n"));
      }

      let inner = format!("{}{}", if name == "li" { "- " } else { "" }, markup_text(node.children()));

      if LINE_ENDS.contains(&name) { format!("{inner}\n") } else { inner }
    }
    Node::Document | Node::Fragment => markup_text(node.children()),
    _ => String::new(),
  }
}

static TRAILING_SPACES: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"[ \t]+\n").expect("the spaces pattern reads"));
static BLANK_RUNS: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"\n{3,}").expect("the blank lines pattern reads"));

/// An HTML body's blocks as lines and its quotes as `> ` lines.
fn block_text<'a>(nodes: impl Iterator<Item = NodeRef<'a, Node>>) -> String {
  let text = markup_text(nodes);
  let text = TRAILING_SPACES.replace_all(&text, "\n");

  BLANK_RUNS.replace_all(&text, "\n\n").trim().to_owned()
}

/// The plain-text alternative of an HTML body: its blocks as lines, a quote as `> ` lines, and a no-break
/// space as an ordinary one.
pub fn html_to_text(html: &str) -> String {
  let document = Html::parse_document(html);

  block_text(document.tree.root().children()).replace('\u{a0}', " ")
}

/// A picture written into the markup, as the message carries it.
pub struct InlineImage {
  pub cid: String,
  pub filename: String,
  pub kind: String,
  pub content: Vec<u8>,
}

/// The suffix a picture's file is named with, by its type.
fn image_suffix(kind: &str) -> &'static str {
  match kind {
    "image/png" => "png",
    "image/jpeg" => "jpg",
    "image/gif" => "gif",
    "image/webp" => "webp",
    "image/svg+xml" => "svg",
    _ => "bin",
  }
}

/// A `data:` address of a picture carried as base64: its type and its bytes.
fn data_image(address: &str) -> Option<(String, Vec<u8>)> {
  let rest = address
    .get(..5)
    .filter(|scheme| scheme.eq_ignore_ascii_case("data:"))
    .map(|_| &address[5..])?;
  let (kind, data) = rest.split_once(";base64,").or_else(|| rest.split_once(";BASE64,"))?;
  let kind = kind.to_lowercase();
  let subtype = kind.strip_prefix("image/")?;

  if subtype.is_empty()
    || !subtype.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '-'))
  {
    return None;
  }

  let clean: String = data.chars().filter(|c| !c.is_whitespace()).collect();
  let content = STANDARD.decode(clean).ok().filter(|content| !content.is_empty())?;

  Some((kind, content))
}

/// The markup with each picture of its own replaced by the part carrying it, and those parts. A message's
/// own pictures go out as inline parts rather than as addresses in the markup: a mail client shows a part
/// it was handed, and a great many refuse a picture spelled out inside the markup. The content ID is made
/// from where the picture stands and the message's ID, so building the same draft twice under one ID builds
/// the same message.
pub fn inline_images(html: &str, id: &str) -> (String, Vec<InlineImage>) {
  let images: RefCell<Vec<InlineImage>> = RefCell::new(Vec::new());
  let rewritten = rewrite_str(
    html,
    RewriteStrSettings::default().append_element_content_handler(element!("[src]", |picture| {
      let Some((kind, content)) = picture.get_attribute("src").as_deref().and_then(data_image) else {
        return Ok(());
      };
      let mut images = images.borrow_mut();
      let at = images.len() + 1;
      let cid = format!("image{at}.{id}");

      picture.set_attribute("src", &format!("cid:{cid}"))?;
      images.push(InlineImage {
        filename: format!("image{at}.{}", image_suffix(&kind)),
        cid,
        kind,
        content,
      });

      Ok(())
    })),
  );

  match rewritten {
    Ok(rewritten) => (rewritten, images.into_inner()),
    Err(_) => (html.to_owned(), Vec::new()),
  }
}

/// The way back from [`inline_images`]: every picture the markup names by `cid:` is carried as its own
/// bytes again, as a `data:` address, where `part` finds the inline part of that content id as its type
/// and its bytes. A `cid:` no part answers is left as it stands.
pub fn data_images(html: &str, part: impl Fn(&str) -> Option<(String, Vec<u8>)>) -> String {
  let rewritten = rewrite_str(
    html,
    RewriteStrSettings::default().append_element_content_handler(element!("[src]", |picture| {
      let Some(src) = picture.get_attribute("src") else {
        return Ok(());
      };
      let cid = src
        .get(..4)
        .filter(|scheme| scheme.eq_ignore_ascii_case("cid:"))
        .map(|_| src[4..].trim_start_matches('<').trim_end_matches('>'));
      let Some((kind, content)) = cid.and_then(&part) else {
        return Ok(());
      };

      picture.set_attribute("src", &format!("data:{kind};base64,{}", STANDARD.encode(content)))?;

      Ok(())
    })),
  );

  rewritten.unwrap_or_else(|_| html.to_owned())
}

/// What a message is built for. A draft carries the headers that say what it answers, and its Bcc; a
/// message sent carries neither, and the copy kept in the sent folder keeps its Bcc.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
  Draft,
  Send,
  SentCopy,
}

fn address_of(address: &MailAddress) -> Address<'static> {
  if address.name.is_empty() {
    Address::from(address.address.clone())
  } else {
    Address::from((address.name.clone(), address.address.clone()))
  }
}

fn addresses_of(list: &[MailAddress]) -> Address<'static> {
  Address::from(list.iter().map(address_of).collect::<Vec<_>>())
}

/// A message ID without its angle brackets, as the builder writes one.
fn bare(id: &str) -> String {
  id.trim().trim_start_matches('<').trim_end_matches('>').to_owned()
}

/// A new message ID without its angle brackets: random hex at the domain of `address`, `localhost` where
/// it has none. Every message this service writes is named this way, so no ID carries the machine's name.
pub fn new_message_id(address: &str) -> String {
  let mut random = [0u8; 12];

  rand::rng().fill_bytes(&mut random);

  let domain = address.split_once('@').map_or("localhost", |(_, domain)| domain);
  let hex: String = random.iter().map(|byte| format!("{byte:02x}")).collect();

  format!("{hex}@{domain}")
}

/// The whole message, as the bytes a server takes. `date` is Unix seconds; where `message_id` is none, a new
/// one is made at the sender's domain.
pub fn build_message(
  draft: &Draft,
  attachments: &[ResolvedAttachment],
  purpose: Purpose,
  message_id: Option<&str>,
  date: i64,
) -> std::io::Result<Vec<u8>> {
  let message_id = message_id.map_or_else(|| new_message_id(&draft.from), bare);
  let id_seed: String = message_id
    .chars()
    .filter(|c| !matches!(c, '<' | '>' | '@') && !c.is_whitespace())
    .collect();
  let (html, images) = inline_images(&draft.html, if id_seed.is_empty() { "bmo" } else { &id_seed });
  let alternative = MimePart::new(
    "multipart/alternative",
    vec![
      MimePart::new("text/plain", BodyPart::Text(html_to_text(&html).into())),
      MimePart::new("text/html", BodyPart::Text(html.into())),
    ],
  );
  let content = if images.is_empty() {
    alternative
  } else {
    let mut related = vec![alternative];

    related.extend(images.into_iter().map(|image| {
      MimePart::new(ContentType::new(image.kind), BodyPart::Binary(image.content.into()))
        .header("Content-Disposition", ContentType::new("inline").attribute("filename", image.filename))
        .cid(image.cid)
    }));

    MimePart::new("multipart/related", related)
  };
  let body = if attachments.is_empty() {
    content
  } else {
    let mut mixed = vec![content];

    mixed.extend(attachments.iter().map(|one| {
      MimePart::new(ContentType::new(one.kind.clone()), BodyPart::Binary(one.content.clone().into()))
        .attachment(one.filename.clone())
    }));

    MimePart::new("multipart/mixed", mixed)
  };
  let mut message = MessageBuilder::new()
    .from(Address::from(draft.from.clone()))
    .subject(draft.subject.clone())
    .date(date);

  if !draft.to.is_empty() {
    message = message.to(addresses_of(&draft.to));
  }

  if !draft.cc.is_empty() {
    message = message.cc(addresses_of(&draft.cc));
  }

  if purpose != Purpose::Send && !draft.bcc.is_empty() {
    message = message.bcc(addresses_of(&draft.bcc));
  }

  message = message.message_id(message_id);

  if let Some(id) = &draft.in_reply_to {
    message = message.in_reply_to(bare(id));
  }

  if !draft.references.is_empty() {
    message =
      message.references(MessageId::from(draft.references.iter().map(|id| bare(id)).collect::<Vec<_>>()));
  }

  if draft.receipt {
    message = message.header("Disposition-Notification-To", Address::from(draft.from.clone()));
  }

  if purpose == Purpose::Draft {
    let kind = match draft.kind {
      DraftKind::New => "new",
      DraftKind::Reply => "reply",
      DraftKind::Forward => "forward",
    };

    message = message.header(DRAFT_KIND_HEADER, Text::new(kind));

    if let Some(original) = &draft.original {
      message = message.header(DRAFT_ORIGINAL_HEADER, Text::new(original.clone()));
    }
  }

  message.body(body).write_to_vec()
}
