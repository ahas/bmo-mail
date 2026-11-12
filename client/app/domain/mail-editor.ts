// The mail editor's toolbar, and what it does with a picture (docs/mail.md, 「Writing」). A picture
// put into a message or a signature is carried in the markup as its own bytes, because a signature
// has no message to hang an attachment on and a draft's picture must survive being saved and
// reopened. The message that goes out turns each one into an inline part, so nothing leaves here as
// a data address.

import type { EditorToolbarItem } from "@nuxt/ui";
import type { Translate } from "bmo-ui/shared/wording";

// How large a picture may be. The message is capped by the submission server's own size, and a
// picture written into the markup grows by a third on its way there; this is the line past which
// one picture would eat that whole allowance.
export const IMAGE_BYTES = 2 * 1024 * 1024;

export class MailImageError extends Error {}

// Whether a file is a picture this editor takes. A browser hands over its own reading of the type,
// and anything it does not call a picture is left to the attachment list instead.
export function isImageFile(file: { type: string }): boolean {
  return file.type.startsWith("image/");
}

// The picture as the markup carries it. The browser reads the file; nothing is asked of the server,
// so a signature is written with no draft behind it.
export async function readImageData(t: Translate, file: File): Promise<string> {
  if (!isImageFile(file)) {
    throw new MailImageError(t("mail.error.image_only"));
  }

  if (file.size > IMAGE_BYTES) {
    throw new MailImageError(t("mail.error.image_too_large", { mb: Math.floor(IMAGE_BYTES / 1024 / 1024) }));
  }

  const read = await new Promise<string>((done, fail) => {
    const reader = new FileReader();

    reader.onerror = () => fail(new MailImageError(t("mail.error.image_unreadable")));
    reader.onload = () => done(String(reader.result ?? ""));
    reader.readAsDataURL(file);
  });

  if (!read.startsWith("data:image/")) {
    throw new MailImageError(t("mail.error.image_unreadable"));
  }

  return read;
}

// The first picture among what a paste or a drop carried, and nothing where it carried none.
export function imageIn(files: readonly File[]): File | null {
  return files.find(isImageFile) ?? null;
}

// The marks of the editor's toolbar, group by group: what the component library's own toolbar
// draws, ahead of the cells the editor adds beside them. The place the editor keeps while its code
// arrives draws one bar for each of these, so it is read from here by both.
export function editorToolbar(t: Translate): EditorToolbarItem[][] {
  return [
    [
      { kind: "undo", icon: "i-material-symbols-undo", tooltip: { text: t("mail.action.undo") }, "aria-label": t("mail.action.undo") },
      { kind: "redo", icon: "i-material-symbols-redo", tooltip: { text: t("mail.action.redo") }, "aria-label": t("mail.action.redo") },
    ],
    [
      {
        icon: "i-material-symbols-format-h1",
        tooltip: { text: t("mail.label.heading") },
        "aria-label": t("mail.label.heading"),
        items: [
          { kind: "paragraph", label: t("mail.label.paragraph"), icon: "i-material-symbols-format-paragraph" },
          { kind: "heading", level: 1, label: t("mail.label.heading_1"), icon: "i-material-symbols-format-h1" },
          { kind: "heading", level: 2, label: t("mail.label.heading_2"), icon: "i-material-symbols-format-h2" },
          { kind: "heading", level: 3, label: t("mail.label.heading_3"), icon: "i-material-symbols-format-h3" },
        ],
      },
    ],
    [
      { kind: "mark", mark: "bold", icon: "i-material-symbols-format-bold", tooltip: { text: t("mail.label.bold") }, "aria-label": t("mail.label.bold") },
      { kind: "mark", mark: "italic", icon: "i-material-symbols-format-italic", tooltip: { text: t("mail.label.italic") }, "aria-label": t("mail.label.italic") },
      { kind: "mark", mark: "underline", icon: "i-material-symbols-format-underlined", tooltip: { text: t("mail.label.underline") }, "aria-label": t("mail.label.underline") },
      { kind: "mark", mark: "strike", icon: "i-material-symbols-format-strikethrough", tooltip: { text: t("mail.label.strike") }, "aria-label": t("mail.label.strike") },
    ],
    [
      {
        icon: "i-material-symbols-format-align-left",
        tooltip: { text: t("mail.label.align") },
        "aria-label": t("mail.label.align"),
        items: [
          { kind: "textAlign", align: "left", label: t("mail.label.align_left"), icon: "i-material-symbols-format-align-left" },
          { kind: "textAlign", align: "center", label: t("mail.label.align_center"), icon: "i-material-symbols-format-align-center" },
          { kind: "textAlign", align: "right", label: t("mail.label.align_right"), icon: "i-material-symbols-format-align-right" },
          { kind: "textAlign", align: "justify", label: t("mail.label.align_justify"), icon: "i-material-symbols-format-align-justify" },
        ],
      },
    ],
    [
      {
        icon: "i-material-symbols-format-list-bulleted",
        tooltip: { text: t("mail.label.list") },
        "aria-label": t("mail.label.list"),
        items: [
          { kind: "bulletList", label: t("mail.label.bullet_list"), icon: "i-material-symbols-format-list-bulleted" },
          { kind: "orderedList", label: t("mail.label.ordered_list"), icon: "i-material-symbols-format-list-numbered" },
          { kind: "blockquote", label: t("mail.label.quote"), icon: "i-material-symbols-format-quote" },
          { kind: "codeBlock", label: t("mail.label.code_block"), icon: "i-material-symbols-code" },
        ],
      },
    ],
    [{ kind: "clearFormatting", icon: "i-material-symbols-format-clear", tooltip: { text: t("mail.action.clear_format") }, "aria-label": t("mail.action.clear_format") }],
  ];
}
