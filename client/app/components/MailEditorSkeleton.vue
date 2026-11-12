<template>
  <!-- The place the mail editor keeps while its own code arrives (docs/mail.md, 「Writing」): the
       editor is loaded apart from the mail screen, so the first composer or signature opened before
       it has arrived stands on this. The toolbar row is the editor's own, drawn at once, with one
       bar where each of its marks will stand, grouped as the editor groups them and gathered as the
       editor's row gathers by the room it has. The writing
       surface keeps its full height and its own padding: an empty body shows the placeholder the
       editor will show, and a body already written in — a signature, a quoted message — a line for
       each of its first blocks. A load that failed says so in the surface's place. -->
  <p v-if="props.error !== undefined" role="alert" class="px-3 py-2 text-sm text-error">{{ $t("mail.error.editor_failed") }}</p>
  <ContentSkeleton v-else :label="$t('mail.status.editor_loading')" :class="props.class" listClass="flex flex-1 flex-col">
    <ToolBar ref="bar" :label="$t('mail.label.format')" class="sticky top-0 z-10 bg-default">
      <div class="flex items-stretch !gap-1.5 !px-1">
        <template v-for="(marks, group) in groups" :key="group">
          <div v-if="group > 0" class="w-px self-stretch bg-border" />
          <div class="flex items-center gap-0.5">
            <div v-for="n in marks" :key="n" :class="CELL"><SkeletonBar class="size-4 rounded-sm" /></div>
          </div>
        </template>
      </div>
      <div v-for="n in ownCells" :key="`own-${n}`" class="!px-1">
        <div :class="CELL"><SkeletonBar class="size-4 rounded-sm" /></div>
      </div>
    </ToolBar>
    <div class="flex-1 px-3 py-2 sm:px-8" :class="props.compact ? 'min-h-32' : 'min-h-48'">
      <p v-if="blocks === 0" class="leading-7 text-dimmed">{{ props.placeholder }}</p>
      <div v-else class="flex flex-col gap-5">
        <SkeletonLine v-for="n in blocks" :key="n" size="lg" :width="n === blocks ? 'w-1/2' : 'w-full'" />
      </div>
    </div>
  </ContentSkeleton>
</template>

<script setup lang="ts">
import { barGathers, GATHERED_ACTIONS, useBarRoom } from "bmo-ui/app/domain/actionbar";

import { editorToolbar } from "../domain/mail-editor";

defineOptions({ inheritAttrs: false });

// What the editor itself is handed, so its place takes the same height, frame and writing; what
// else the editor is handed has no bearing on its place. A failed load hands only the error.
const props = defineProps<{
  modelValue?: string;
  placeholder?: string;
  compact?: boolean;
  class?: string;
  error?: unknown;
}>();

const { t } = useI18n();

// The most lines the surface draws for a body already written in: as many as its least height holds
// at the editor's own line and block spacing, so the place never stands taller than the editor.
const mostBlocks = computed(() => (props.compact ? 2 : 4));

// How many of the body's blocks the surface draws a line for, and none for a body with nothing
// written in it, which the editor shows as its placeholder.
const blocks = computed(() => {
  const body = new DOMParser().parseFromString(props.modelValue ?? "", "text/html").body;

  if (body.textContent?.trim() === "" && body.querySelector("img, table, hr") === null) {
    return 0;
  }

  return Math.min(Math.max(body.children.length, 1), mostBlocks.value);
});

// The marks of the editor's toolbar, group by group, and the cells of its own beside them — the
// text colour and the insert menu; the table menu stands only while the cursor is in a table.
const TOOLBAR_GROUPS = editorToolbar(t).map((group) => group.length);
const OWN_CELLS = 2;

// Short of room the editor's row stands the marks reached for first as one group and one more menu,
// so its place stands the same.
const bar = useTemplateRef<{ $el: HTMLElement }>("bar");
const room = useBarRoom(() => bar.value?.$el);
const gathered = computed(() => barGathers(room.value));
const groups = computed(() => (gathered.value ? [GATHERED_ACTIONS] : TOOLBAR_GROUPS));
const ownCells = computed(() => (gathered.value ? 1 : OWN_CELLS));

// One mark's place: the component library's small square icon button, centred in the row, and the
// cells around the marks keep the small room from the row's edge a cell of buttons keeps.
const CELL = "flex size-7 items-center justify-center";
</script>
