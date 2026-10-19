//! Where a name somebody registered ends and a registry's own tail begins (`docs/mail.md`, 「Sender
//! avatars」). A machine sends under a name of its own — `mail.example.com` — and the mark worth showing
//! beside it is the one `example.com` publishes, so whatever climbs from the one to the other has to know
//! which of the labels were ever anybody's to register. The public suffix list says so.

/// The domain somebody registered, out of the name a sender writes from. A name that is no longer than
/// that, or one the list does not know, is answered unchanged — which is how a caller asks whether there is
/// anywhere to climb to at all.
pub fn registrable_domain(domain: &str) -> String {
  psl::domain_str(domain).unwrap_or(domain).to_owned()
}
