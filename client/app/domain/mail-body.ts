import createDOMPurify from "dompurify";

import { pairedColor } from "./mail-frame";

// A mail's body made safe to show (docs/mail.md, 「Reading」). The HTML a sender wrote is cleaned
// with DOMPurify under a profile of its own: nothing runs, nothing posts, nothing loads from the web
// until the reader says so for that message, and the images the message carries itself are pointed
// at the console's own route. What is left is shown in a frame with no scripts and a content
// policy of its own, so what the sender styled stays inside the frame. Only a browser can run this;
// nothing on the server reaches it.
//
// The same walk over the elements is also where a cell that paints itself is given the text colour
// that reads against its own background, since this is the one place every element of a body passes
// through with its style still its own.

export interface CleanBody {
  html: string;
  // Whether anything was held back from the web, which is what puts up the bar that shows it.
  blocked: boolean;
}

export interface CleanOptions {
  // Whether the web may be reached for this message: its remote images and styles shown.
  allowRemote: boolean;
  // Where an image the message carries by `cid:` is fetched from.
  cidUrl: (cid: string) => string;
}

// What a mail body never keeps: anything that runs, posts, frames another page or speaks for the
// document the frame is.
const FORBIDDEN = ["script", "iframe", "frame", "frameset", "object", "embed", "applet", "form", "input", "button", "textarea", "select", "option", "base", "meta", "link", "noscript", "template", "portal"];

const URL_ATTRIBUTES = ["src", "background", "poster", "href", "xlink:href"];
const LOADING_ATTRIBUTES = ["src", "background", "poster"];

// The declarations that give an element a background of its own, and the one that gives it a text
// colour. A shorthand carries the colour among its other parts, so each is read whole.
const BACKGROUND_DECLARATION = /(?:^|[;\s])background(?:-color)?\s*:([^;]*)/i;
const COLOR_DECLARATION = /(?:^|[;\s])color\s*:/i;

// Whether the sender said what colour this element's own letters are — in its style, or in the
// `color` attribute the markup a mail is written in still uses.
function declaresColor(element: Element): boolean {
  return element.hasAttribute("color") || COLOR_DECLARATION.test(element.getAttribute("style") ?? "");
}

// The background the sender gave this element, from its style first and its `bgcolor` attribute
// after, or nothing where it was given none.
function declaredBackground(element: Element): string | null {
  const declaration = BACKGROUND_DECLARATION.exec(element.getAttribute("style") ?? "");

  if (declaration !== null) {
    return declaration[1]!;
  }

  return element.getAttribute("bgcolor");
}

// A cell that carries its own background carries its own text colour with it (docs/mail.md,
// 「Reading」). Left alone, its letters take the colour the frame sets for the surface around it,
// which pairs with that surface and not with this one — the console's pale grey on a sender's white
// panel, or near-black on a navy header. A cell whose colour the sender did set is left as written.
function pairTextColor(element: Element): void {
  if (declaresColor(element)) {
    return;
  }

  const background = declaredBackground(element);
  const paired = background === null ? null : pairedColor(background);

  if (paired !== null) {
    const written = (element.getAttribute("style") ?? "").trim().replace(/;$/, "");

    element.setAttribute("style", written === "" ? `color:${paired}` : `${written};color:${paired}`);
  }
}

function isRemote(value: string): boolean {
  return /^\s*(https?:)?\/\//i.test(value);
}

// A CSS text with every remote `url(…)` taken out, and whether one was.
function holdRemoteCss(css: string, allowRemote: boolean): { css: string; blocked: boolean } {
  let blocked = false;
  const kept = css
    .replace(/@import[^;]*;?/gi, (rule: string) => {
      if (allowRemote) {
        return rule;
      }

      blocked = true;

      return "";
    })
    .replace(/url\(\s*(['"]?)(.*?)\1\s*\)/gi, (whole: string, _quote: string, target: string) => {
      if (isRemote(target) && !allowRemote) {
        blocked = true;

        return "none";
      }

      return whole;
    });

  return { css: kept, blocked };
}

export function cleanMailHtml(html: string, options: CleanOptions): CleanBody {
  const purify = createDOMPurify(window);
  let blocked = false;

  purify.addHook("uponSanitizeElement", (node, data) => {
    if (data.tagName === "style" && node.textContent !== null) {
      const held = holdRemoteCss(node.textContent, options.allowRemote);

      blocked ||= held.blocked;
      node.textContent = held.css;
    }
  });

  purify.addHook("afterSanitizeAttributes", (node) => {
    const element = node as Element;

    for (const name of LOADING_ATTRIBUTES) {
      const value = element.getAttribute(name);

      if (value === null) {
        continue;
      }

      if (/^\s*cid:/i.test(value)) {
        element.setAttribute(name, options.cidUrl(value.trim().slice(4).replace(/^<|>$/g, "")));
      } else if (isRemote(value) && !options.allowRemote) {
        blocked = true;
        element.removeAttribute(name);
        element.setAttribute(`data-held-${name}`, value);
      }
    }

    if (element.hasAttribute("srcset")) {
      if (!options.allowRemote && isRemote(element.getAttribute("srcset") ?? "")) {
        blocked = true;
      }

      element.removeAttribute("srcset");
    }

    const style = element.getAttribute("style");

    if (style !== null) {
      const held = holdRemoteCss(style, options.allowRemote);

      blocked ||= held.blocked;
      element.setAttribute("style", held.css);
    }

    if (element.tagName === "A") {
      element.setAttribute("target", "_blank");
      element.setAttribute("rel", "noopener noreferrer");
    }

    for (const name of URL_ATTRIBUTES) {
      const value = element.getAttribute(name);

      if (value !== null && /^\s*(javascript|vbscript|data:text\/html)/i.test(value)) {
        element.removeAttribute(name);
      }
    }

    // Last, so the background it reads is the one that will actually be drawn: a remote one held
    // back is no surface, and nothing is paired against it.
    pairTextColor(element);
  });

  const cleaned = purify.sanitize(html, {
    FORBID_TAGS: FORBIDDEN,
    ADD_TAGS: ["style"],
    FORCE_BODY: true,
    ALLOW_DATA_ATTR: false,
    ADD_ATTR: ["target"],
  });

  return { html: String(cleaned), blocked };
}
