import { defineAsyncComponent } from "vue";

import MailEditorSkeleton from "../components/MailEditorSkeleton.vue";

// The mail editor, loaded apart from the mail screen (docs/mail.md, 「Writing」). It carries the
// rich-text engine, most of what the screen would otherwise load before it is drawn, and is needed
// only once a message or a signature is being written.
export function loadMailEditor() {
  return import("~/components/MailEditor.vue");
}

// What a composer or a signature section draws in the editor's place. Until the editor has
// arrived it stands on the editor's skeleton, from the first frame, so its place never stands
// empty first, and a load that failed says so there. Every page stands inside the page's suspense,
// which would otherwise take the wait over and draw nothing in the editor's place.
export const AsyncMailEditor = defineAsyncComponent({
  loader: loadMailEditor,
  loadingComponent: MailEditorSkeleton,
  errorComponent: MailEditorSkeleton,
  delay: 0,
  suspensible: false,
});
