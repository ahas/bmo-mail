import { webAddresses } from "bmo-text/web-address";

// The web addresses a plain-text body's words carry, read by the one rule every place that finds them
// shares (bmo-text's web-address): a bare address ends where the sentence takes over.

export interface LinkPart {
  text: string;
  // The address a part opens, or null for words.
  href: string | null;
}

// The words split into plain runs and addresses, in order, so every address can be pressed.
export function linkParts(text: string): LinkPart[] {
  const parts: LinkPart[] = [];
  let at = 0;

  for (const address of webAddresses(text)) {
    if (address.start > at) {
      parts.push({ text: text.slice(at, address.start), href: null });
    }

    parts.push({ text: text.slice(address.start, address.end), href: address.url });
    at = address.end;
  }

  if (at < text.length) {
    parts.push({ text: text.slice(at), href: null });
  }

  return parts;
}
