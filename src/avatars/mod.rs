//! The sender's own logo, found where the sender publishes it (`docs/mail.md`, 「Sender avatars」). The
//! service looks it up and fetches it; the browser asks the service and never the sender, so nothing a
//! message carries tells its sender that it was opened. What was found — and that nothing was — is kept in
//! the store, so a list of fifty rows is not fifty lookups.

pub mod domains;

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use futures::FutureExt;
use futures::future::{BoxFuture, Shared};
use hickory_resolver::TokioResolver;
use hickory_resolver::proto::rr::RData;
use regex::Regex;
use reqwest::{Url, header};
use scraper::{Html, Selector};
use sqlx::SqlitePool;

use crate::avatars::domains::registrable_domain;

/// The most of a logo the service keeps. A BIMI mark is a few kilobytes of SVG; a site icon is smaller
/// still, and anything past this is not an icon.
const MOST_BYTES: usize = 64 * 1024;
/// The most of a page read while looking for the icon its head names.
const MOST_PAGE_BYTES: usize = 256 * 1024;
/// How long one fetch may take.
const WAIT: Duration = Duration::from_secs(5);
/// How many redirects are followed, each of them checked before it is taken.
const MOST_HOPS: usize = 3;
/// How long a logo found holds before it is looked for again.
const FOUND_MS: i64 = 7 * 24 * 60 * 60 * 1000;
/// How long a domain that had none is left alone.
const MISSING_MS: i64 = 24 * 60 * 60 * 1000;

/// The types a logo may come back as. An icon of any other type is not shown.
const IMAGE_TYPES: [&str; 7] = [
  "image/svg+xml",
  "image/png",
  "image/jpeg",
  "image/webp",
  "image/gif",
  "image/x-icon",
  "image/vnd.microsoft.icon",
];

/// A logo: its type and its bytes.
#[derive(Clone, Debug)]
pub struct MailAvatar {
  pub kind: String,
  pub bytes: Vec<u8>,
}

/// A sender's domain as a lookup takes it: the part after `@`, lower case, and nothing that is not a domain
/// name.
pub fn avatar_domain(value: &str) -> Option<String> {
  static NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[a-z0-9]([a-z0-9-]*[a-z0-9])?(\.[a-z0-9]([a-z0-9-]*[a-z0-9])?)+$")
      .expect("the name pattern reads")
  });
  let domain = value.rsplit_once('@').map_or(value, |(_, domain)| domain).trim().to_lowercase();
  let domain = domain.strip_suffix('.').unwrap_or(&domain).to_owned();

  (NAME.is_match(&domain) && domain.len() <= 253).then_some(domain)
}

/// Whether an address is one of this machine's own networks rather than the open web: the loopback, the
/// private ranges, the link-local ones and the carrier range, in either family.
pub fn own_network(address: IpAddr) -> bool {
  match address {
    IpAddr::V6(v6) => {
      if let Some(v4) = v6.to_ipv4_mapped() {
        return own_network(IpAddr::V4(v4));
      }

      let first = v6.segments()[0];

      v6.is_unspecified() || v6.is_loopback() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
    }
    IpAddr::V4(v4) => {
      let [a, b, ..] = v4.octets();

      a == 0
        || v4.is_private()
        || v4.is_loopback()
        || v4.is_link_local()
        || (a == 100 && (64..=127).contains(&b))
    }
  }
}

/// Whether a fetch may go there at all. The address comes out of a record the sender writes, so it is a
/// stranger's word: it may only be an ordinary HTTPS address on the open web, never a door inside this
/// house. Every hop of a redirect is asked this again, since the first answer says nothing about where it
/// points.
async fn reachable(url: &Url) -> bool {
  if url.scheme() != "https" || url.port().is_some_and(|port| port != 443) {
    return false;
  }

  let Some(host) = url.host_str() else {
    return false;
  };

  match tokio::net::lookup_host((host.trim_start_matches('[').trim_end_matches(']'), 443)).await {
    Ok(found) => {
      let found: Vec<_> = found.collect();

      !found.is_empty() && found.iter().all(|one| !own_network(one.ip()))
    }
    Err(_) => false,
  }
}

/// The body of an answer, read no further than the cap. A server that keeps sending is cut off rather than
/// read to its end.
async fn read_capped(mut response: reqwest::Response, cap: usize) -> Option<Vec<u8>> {
  let mut bytes = Vec::new();

  while let Some(chunk) = response.chunk().await.ok()? {
    bytes.extend_from_slice(&chunk);

    if bytes.len() > cap {
      return None;
    }
  }

  Some(bytes)
}

/// Whether an SVG is a picture and nothing more: no script, no event attribute, no document of its own, and
/// nothing it would go and fetch. A logo that is anything more is dropped where it lies rather than
/// cleaned: a mark worth showing never needed cleaning.
pub fn svg_is_safe(text: &str) -> bool {
  let lowered = text.to_lowercase();

  if lowered.contains("javascript:") || lowered.contains("data:text/html") {
    return false;
  }

  let document = Html::parse_document(text);
  let fetching = |value: &str| {
    let value = value.trim();
    let scheme = value.split_once(':').map(|(scheme, _)| scheme);

    scheme.is_some_and(|scheme| !scheme.is_empty() && scheme.chars().all(|c| c.is_ascii_alphabetic()))
  };

  document.tree.nodes().filter_map(|node| node.value().as_element()).all(|element| {
    let name = element.name().to_lowercase();
    let href = element
      .attrs()
      .find(|(attribute, _)| attribute.eq_ignore_ascii_case("href"))
      .map(|(_, value)| value);
    let acts = element.attrs().any(|(attribute, _)| attribute.to_lowercase().starts_with("on"));
    let foreign = matches!(name.as_str(), "script" | "foreignobject" | "iframe" | "embed" | "object");
    let fetches = match name.as_str() {
      "use" => href.is_some_and(|href| !href.trim().starts_with('#')),
      "image" => href.is_some_and(fetching),
      _ => false,
    };

    !acts && !foreign && !fetches
  })
}

/// The icons a site names in its own head, resolved against the page they were named on. The largest
/// `sizes` comes first, since a site that offers several offers the small one first.
pub fn icon_links(html: &str, base: &Url) -> Vec<Url> {
  static LINKS: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("link[rel][href]").expect("the link selector reads"));
  static SIZE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\d+)x\d+").expect("the size pattern reads"));
  let document = Html::parse_document(html);
  let mut found: Vec<(Url, u64)> = Vec::new();

  for link in document.select(&LINKS) {
    let rel = link.value().attr("rel").unwrap_or("").to_lowercase();
    let words: Vec<&str> = rel.split_whitespace().collect();
    let names_icon =
      words.contains(&"icon") || words.contains(&"apple-touch-icon") || rel.trim() == "shortcut icon";

    if !names_icon {
      continue;
    }

    let Some(url) = link.value().attr("href").and_then(|href| base.join(href).ok()) else {
      continue;
    };
    let size = link
      .value()
      .attr("sizes")
      .and_then(|sizes| SIZE.captures(sizes))
      .and_then(|found| found[1].parse().ok())
      .unwrap_or(0);

    found.push((url, size));
  }

  found.sort_by_key(|(_, size)| std::cmp::Reverse(*size));
  found.into_iter().map(|(url, _)| url).collect()
}

/// The BIMI record's tags the service reads: the version, and where the logo is. A BIMI record is a list of
/// `tag=value` pairs that no maintained crate reads, so the two tags are read here.
static BIMI_VERSION: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"(?i)^\s*v\s*=\s*BIMI1\s*;").expect("the version pattern reads"));
static BIMI_LOCATION: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"(?i)(?:^|;)\s*l\s*=\s*([^;]+)").expect("the location pattern reads"));

struct Inner {
  asking: Mutex<HashMap<String, Shared<BoxFuture<'static, Option<MailAvatar>>>>>,
  client: Option<reqwest::Client>,
  resolver: Option<TokioResolver>,
}

/// The lookups of sender logos: the one each domain has going, and what they fetch with.
#[derive(Clone)]
pub struct Avatars {
  inner: Arc<Inner>,
}

impl std::fmt::Debug for Avatars {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter.debug_struct("Avatars").finish_non_exhaustive()
  }
}

impl Default for Avatars {
  fn default() -> Self {
    let client = reqwest::Client::builder()
      .redirect(reqwest::redirect::Policy::none())
      .timeout(WAIT)
      .build()
      .ok();
    let resolver = TokioResolver::builder_tokio().ok().and_then(|builder| builder.build().ok());

    Self {
      inner: Arc::new(Inner {
        asking: Mutex::new(HashMap::new()),
        client,
        resolver,
      }),
    }
  }
}

impl Avatars {
  /// One answer from the open web: the address checked, then each redirect checked before it is taken.
  async fn get(&self, url: Url) -> Option<reqwest::Response> {
    let client = self.inner.client.as_ref()?;
    let mut at = url;

    for _ in 0..=MOST_HOPS {
      if !reachable(&at).await {
        return None;
      }

      let response = client
        .get(at.clone())
        .header(header::ACCEPT, "image/*,text/html;q=0.5")
        .send()
        .await
        .ok()?;

      if !response.status().is_redirection() {
        return response.status().is_success().then_some(response);
      }

      let next = response.headers().get(header::LOCATION)?.to_str().ok()?;

      at = at.join(next).ok()?;
    }

    None
  }

  /// A picture fetched and checked: a type a browser draws, a size an icon could have, and no SVG that does
  /// anything but draw.
  async fn picture(&self, url: Url) -> Option<MailAvatar> {
    let response = self.get(url).await?;
    let kind = response
      .headers()
      .get(header::CONTENT_TYPE)
      .and_then(|value| value.to_str().ok())
      .unwrap_or("")
      .split(';')
      .next()
      .unwrap_or("")
      .trim()
      .to_lowercase();

    if !IMAGE_TYPES.contains(&kind.as_str()) {
      return None;
    }

    let bytes = read_capped(response, MOST_BYTES).await.filter(|bytes| !bytes.is_empty())?;

    if kind == "image/svg+xml" && !svg_is_safe(&String::from_utf8_lossy(&bytes)) {
      return None;
    }

    Some(MailAvatar { kind, bytes })
  }

  /// The SVG a domain's BIMI record points at. A record that names no logo, or one whose logo is not an SVG
  /// a browser may draw, is no logo at all.
  async fn bimi_logo(&self, domain: &str) -> Option<MailAvatar> {
    let resolver = self.inner.resolver.as_ref()?;
    let found = resolver.txt_lookup(format!("default._bimi.{domain}.")).await.ok()?;

    for record in found.answers() {
      let RData::TXT(txt) = &record.data else {
        continue;
      };
      let text: String =
        txt.txt_data.iter().map(|part| String::from_utf8_lossy(part).into_owned()).collect();

      if !BIMI_VERSION.is_match(&text) {
        continue;
      }

      let Some(location) = BIMI_LOCATION.captures(&text).map(|found| found[1].trim().to_owned()) else {
        continue;
      };
      let Ok(url) = Url::parse(&location) else {
        continue;
      };

      if let Some(found) = self.picture(url).await.filter(|found| found.kind == "image/svg+xml") {
        return Some(found);
      }
    }

    None
  }

  /// The icon a domain's own site names in its head, and failing that the icon at the usual place.
  async fn site_icon(&self, domain: &str) -> Option<MailAvatar> {
    let home = Url::parse(&format!("https://{domain}/")).ok()?;

    if let Some(page) = self.get(home.clone()).await {
      let html = page
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|kind| kind.to_lowercase().contains("text/html"));

      if html && let Some(page) = read_capped(page, MOST_PAGE_BYTES).await {
        for url in icon_links(&String::from_utf8_lossy(&page), &home) {
          if let Some(found) = self.picture(url).await {
            return Some(found);
          }
        }
      }
    }

    self.picture(home.join("favicon.ico").ok()?).await
  }

  /// What a domain publishes as its own mark: the SVG its BIMI record names, and failing that the icon its
  /// site names.
  async fn published(&self, domain: &str) -> Option<MailAvatar> {
    match self.bimi_logo(domain).await {
      Some(found) => Some(found),
      None => self.site_icon(domain).await,
    }
  }

  async fn look_up(&self, pool: &SqlitePool, domain: &str) -> Option<MailAvatar> {
    let main = registrable_domain(domain);
    // A name a machine sends under carries no site of its own, so what it publishes nothing for is asked of
    // the domain it sits under — as a sender domain in its own right, which is what holds that answer for
    // every other sender sharing it. What comes back is kept under this domain as well, so the next list of
    // this sender is no lookup at all.
    let found = match self.published(domain).await {
      Some(found) => Some(found),
      None if main != domain => self.sender_avatar(pool.clone(), main).await,
      None => None,
    };

    let _ = keep(pool, domain, found.as_ref()).await;

    found
  }

  /// What the store kept of a domain where it was looked up recently, and a fresh lookup otherwise.
  async fn kept_or_looked_up(&self, pool: &SqlitePool, domain: &str) -> Option<MailAvatar> {
    if let Ok(Some((kind, bytes, at))) = held(pool, domain).await {
      let fresh =
        chrono::Utc::now().timestamp_millis() - at < if bytes.is_none() { MISSING_MS } else { FOUND_MS };

      if fresh {
        return bytes.map(|bytes| MailAvatar { kind, bytes });
      }
    }

    self.look_up(pool, domain).await
  }

  /// One sender domain's logo, from the store where it was looked up recently and from the domain itself
  /// where it was not. Nothing fails: a domain that answers nothing has no logo, and the screen draws the
  /// initial instead. The one lookup a domain has going is found and taken in the same step, so fifty rows
  /// of one sender ask once.
  pub fn sender_avatar(&self, pool: SqlitePool, domain: String) -> BoxFuture<'static, Option<MailAvatar>> {
    let mut asking = self.inner.asking.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

    if let Some(already) = asking.get(&domain) {
      return already.clone().boxed();
    }

    let this = self.clone();
    let named = domain.clone();
    let work = async move {
      let found = this.kept_or_looked_up(&pool, &named).await;

      this.inner.asking.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(&named);

      found
    }
    .boxed()
    .shared();

    asking.insert(domain, work.clone());

    work.boxed()
  }
}

async fn held(
  pool: &SqlitePool,
  domain: &str,
) -> Result<Option<(String, Option<Vec<u8>>, i64)>, sqlx::Error> {
  sqlx::query_as("SELECT type, bytes, looked_up_at FROM avatars WHERE domain = ?")
    .bind(domain)
    .fetch_optional(pool)
    .await
}

async fn keep(pool: &SqlitePool, domain: &str, found: Option<&MailAvatar>) -> Result<(), sqlx::Error> {
  sqlx::query(
    "INSERT INTO avatars (domain, type, bytes, looked_up_at) VALUES (?, ?, ?, ?)
     ON CONFLICT(domain) DO UPDATE SET type = excluded.type, bytes = excluded.bytes, looked_up_at = excluded.looked_up_at",
  )
  .bind(domain)
  .bind(found.map_or("", |found| found.kind.as_str()))
  .bind(found.map(|found| found.bytes.clone()))
  .bind(chrono::Utc::now().timestamp_millis())
  .execute(pool)
  .await
  .map(drop)
}
