// The parts of showing a mail body that need no DOM (docs/mail.md, 「Reading」): a plain-text body
// as HTML, whether a message paints itself, the colour that pairs with a background a sender chose,
// how far a body too wide for its frame is shrunk, and the document the body's frame holds. The
// cleaning of a sender's HTML is DOMPurify's, in its own module, loaded only where a body is shown.

import { linkParts } from "./mail-links";

// The two text colours the frame draws in: one for a light surface, one for a dark one. They are
// the pair the console's own themes use, so a mail reads like the rest of the console.
export const ON_LIGHT = "#111827";
export const ON_DARK = "#e5e7eb";

// How far a body may be shrunk before shrinking stops buying anything. Past half size the letters
// are too small to read, so the rest of the width is reached by scrolling the frame instead.
export const MIN_BODY_SCALE = 0.5;

// The element the frame's body is wrapped in, which is what the scaling is applied to.
export const FIT_ID = "bmo-fit";

// The height a frame is never shorter than, so an empty body is still a body and not a hairline.
export const MIN_FRAME_HEIGHT = 40;

// The room a frame keeps under a body it is scrolling sideways, for the scrollbar that scroll puts
// there on the platforms that give one room of its own.
export const SCROLLBAR_ALLOWANCE = 16;

function escapeHtml(text: string): string {
  return text.replace(/[&<>"']/g, (char) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[char]!);
}

// A plain-text body as HTML: every character escaped, its lines kept, its web addresses made links
// by the rule the chat reads them by (see shared/links).
export function plainTextHtml(text: string): string {
  return linkParts(text)
    .map((part) => (part.href === null ? escapeHtml(part.text) : `<a href="${escapeHtml(part.href)}" target="_blank" rel="noopener noreferrer">${escapeHtml(part.text)}</a>`))
    .join("");
}

// Whether a message paints itself — a background, a text colour, anything of its own. Such a message
// was written against a surface of its own choosing, so it is shown on one: its own colours and the
// console's theme would otherwise be read together, and grey text on the theme's dark surface under
// the sender's white background is text nobody can read. A message that paints nothing is shown on
// the console's own surface and reads in either theme. A background drawn as a picture counts as
// much as one drawn as a colour: the picture is a surface all the same, and what it looks like is
// not something the markup can be asked.
export function paintsItself(html: string): boolean {
  return /\s(?:bgcolor|background|color)\s*=|(?:^|[\s;{"'])(?:background|background-color|background-image|color)\s*:/i.test(html);
}

// The basic colour names a mail is actually written with, as they are named in markup old enough to
// use them. Anything past these is read from its own digits or left alone.
const NAMED: Record<string, [number, number, number]> = {
  aqua: [0, 255, 255], black: [0, 0, 0], blue: [0, 0, 255], fuchsia: [255, 0, 255], gray: [128, 128, 128],
  grey: [128, 128, 128], green: [0, 128, 0], lime: [0, 255, 0], maroon: [128, 0, 0], navy: [0, 0, 128],
  olive: [128, 128, 0], purple: [128, 0, 128], red: [255, 0, 0], silver: [192, 192, 192],
  teal: [0, 128, 128], white: [255, 255, 255], yellow: [255, 255, 0],
};

// A colour read out of the forms a mail writes one in, or nothing where it is not a colour at all —
// a gradient, a picture, `transparent`, a keyword that only means "whatever stands behind".
function readColor(value: string): [number, number, number] | null {
  const text = value.trim().toLowerCase();
  const hex = /(?:^|[\s:])#([0-9a-f]{3}|[0-9a-f]{6})\b/.exec(text);

  if (hex !== null) {
    const digits = hex[1]!;
    const full = digits.length === 3 ? digits.replace(/./g, (char) => char + char) : digits;

    return [0, 2, 4].map((at) => parseInt(full.slice(at, at + 2), 16)) as [number, number, number];
  }

  const rgb = /\brgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)(?:[\s,/]+([\d.]+))?\s*\)/.exec(text);

  if (rgb !== null) {
    // A colour one can see through is not a surface: what is read against it is whatever is behind.
    if (rgb[4] !== undefined && Number(rgb[4]) < 0.5) {
      return null;
    }

    return [Number(rgb[1]), Number(rgb[2]), Number(rgb[3])] as [number, number, number];
  }

  for (const word of text.split(/[^a-z]+/)) {
    const named = NAMED[word];

    if (named !== undefined) {
      return named;
    }
  }

  return null;
}

// How light a colour reads, as the contrast standards measure it.
function luminance([red, green, blue]: [number, number, number]): number {
  const [r, g, b] = [red, green, blue].map((channel) => {
    const part = channel / 255;

    return part <= 0.03928 ? part / 12.92 : ((part + 0.055) / 1.055) ** 2.4;
  }) as [number, number, number];

  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

// How far apart two colours read, as the contrast standards measure it.
function contrast(one: [number, number, number], other: [number, number, number]): number {
  const [lighter, darker] = [luminance(one), luminance(other)].sort((first, second) => second - first) as [number, number];

  return (lighter + 0.05) / (darker + 0.05);
}

// The text colour that reads against a background the sender chose (ui-design-rules, 「Color and
// contrast」): a region carrying its own background carries its own text colour, so the colour set
// for the surface around it never reaches inside. Nothing comes back where the background is not a
// colour that can be read, since there is then nothing to pair against.
export function pairedColor(background: string): string | null {
  const colour = readColor(background);

  if (colour === null) {
    return null;
  }

  // Whichever of the two reads further from it, which for a background halfway between them is not
  // the one a plain light-or-dark reading of the background would have chosen.
  return contrast(colour, [17, 24, 39]) >= contrast(colour, [229, 231, 235]) ? ON_LIGHT : ON_DARK;
}

// How a body wider than its frame is brought inside it (docs/mail.md, 「Reading」): the body is laid
// out at the width it actually wants and the whole of it is scaled down to the frame's width, so a
// layout table whose cells will not narrow keeps the proportions its sender drew instead of being
// cut off at the frame's edge. Shrinking stops at half size; past that the body keeps half size and
// the frame scrolls sideways over the rest.
export interface BodyFit {
  // What to lay the body out at, as a CSS width; empty where it already fits.
  width: string;
  // The transform that brings that width inside the frame; empty where it already fits.
  transform: string;
  scale: number;
}

export function bodyFit(frame: number, natural: number): BodyFit {
  if (frame <= 0 || natural <= frame) {
    return { width: "", transform: "", scale: 1 };
  }

  const scale = Math.max(MIN_BODY_SCALE, frame / natural);

  return { width: `${Math.round(frame / scale)}px`, transform: `scale(${scale})`, scale };
}

// The whole document the body's frame holds: the cleaned body under a content policy that lets
// images in only from the console — and from the web once the reader allowed it — and runs nothing.
// Its content is laid out to the frame's width and wraps inside it; what no wrapping can bring
// inside is shrunk by the console, which measures the frame from outside it, since nothing in here
// is allowed to run.
export function frameDocument(body: string, options: { allowRemote: boolean; origin: string; dark: boolean; painted: boolean }): string {
  const images = ["data:", options.origin, ...(options.allowRemote ? ["https:", "http:"] : [])].join(" ");
  const policy = `default-src 'none'; img-src ${images}; style-src 'unsafe-inline'${options.allowRemote ? " https: http:" : ""}; font-src data:${options.allowRemote ? " https:" : ""}; media-src 'none'`;
  // A painted message stands on the light surface the console draws behind it, whatever the theme.
  const dark = options.dark && !options.painted;

  return [
    "<!doctype html><html><head><meta charset=\"utf-8\">",
    `<meta http-equiv="Content-Security-Policy" content="${policy}">`,
    '<base target="_blank">',
    "<style>",
    // The frame never cuts its content: what is still too wide once the body has been scaled as far
    // as it goes is reached by scrolling this box, which is the body's own and not the screen's.
    // Downwards it never scrolls, because the frame is made as tall as the body is drawn. A shrunk
    // body is still laid out at its full height, so the document keeps a scrollable height larger
    // than the frame; nothing is drawn down there and this is what keeps it out of reach.
    "html{overflow-x:auto;overflow-y:hidden}",
    "html,body{margin:0;padding:0;background:transparent}",
    `body{font-family:'Apple SD Gothic Neo',Pretendard,system-ui,sans-serif;font-size:16px;line-height:1.6;color:${dark ? ON_DARK : ON_LIGHT};overflow-wrap:anywhere}`,
    // A box given a width in pixels is held to the frame instead, so its text wraps rather than
    // making the whole message shrink for the sake of one box. The box being scaled is the one
    // exception: it is laid out at the width the body wants, which is what the scaling then undoes.
    "body *{max-width:100%}",
    // Laying the box out as its own flow keeps the margins of what it holds inside it, so the
    // height measured off it is the height the frame has to be.
    `#${FIT_ID}{transform-origin:0 0;max-width:none;display:flow-root}`,
    "img{max-width:100%;height:auto}",
    // A held image keeps its place and draws nothing: no broken-image mark, no alternative text.
    "img[data-held-src]{visibility:hidden}",
    "table{max-width:100%}",
    "td,th{max-width:100%;overflow-wrap:anywhere;word-break:break-word}",
    "pre{white-space:pre-wrap;overflow-wrap:anywhere}",
    `a{color:${dark ? "#93c5fd" : "#2563eb"}}`,
    `blockquote{margin:8px 0 8px 4px;padding-left:12px;border-left:2px solid ${dark ? "#4b5563" : "#d1d5db"}}`,
    "</style></head><body>",
    `<div id="${FIT_ID}">`,
    body,
    "</div></body></html>",
  ].join("");
}
