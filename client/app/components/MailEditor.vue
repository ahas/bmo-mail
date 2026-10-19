<template>
  <!-- The rich editor the console writes mail with (docs/mail.md, 「Writing」). One component stands
       for every place a message's markup is written — the composer's body and the account's
       signature — so what can be written in one can be written in the other. Its toolbar is the
       console's own toolbar row, so its marks are squares of the row's height like every other
       toolbar here; the marks the component library has no handler for — the text colour, the
       table, the link and the picture — are cells of this screen's own beside them.
       A picture is carried in the markup itself as its own bytes; the message that goes out turns
       each one into an inline part, so a signature's picture arrives with the mail. -->
  <UEditor
    ref="board"
    v-slot="{ editor }"
    v-model="model"
    content-type="html"
    :placeholder="props.placeholder"
    :extensions="extensions"
    :image="IMAGE"
    :editor-props="editorProps"
    :class="props.class"
    :ui="{ base: props.compact ? 'min-h-32 px-3 py-2' : 'min-h-48 px-3 py-2' }"
    @focus="emit('focused')"
  >
    <ToolBar :label="$t('mail.label.format')" class="sticky top-0 z-10 bg-default">
      <UEditorToolbar :editor="editor" :items="TOOLBAR" class="!px-0" />

      <div class="!px-0">
        <UPopover :content="{ align: 'start' }">
          <UTooltip :text="$t('mail.label.text_color')">
            <UButton color="neutral" variant="ghost" size="sm" square icon="i-material-symbols-format-color-text" :aria-label="$t('mail.label.text_color')" />
          </UTooltip>
          <template #content>
            <div class="flex flex-wrap gap-1 p-2">
              <UTooltip :text="$t('common.label.default')">
                <UButton color="neutral" variant="ghost" size="sm" square icon="i-material-symbols-format-color-reset" :aria-label="$t('common.label.default')" @click="editor.chain().focus().unsetColor().run()" />
              </UTooltip>
              <UTooltip v-for="one in TEXT_COLORS" :key="one.value" :text="one.label">
                <UButton color="neutral" variant="ghost" size="sm" square icon="i-material-symbols-circle" :style="{ color: one.value }" :aria-label="one.label" @click="editor.chain().focus().setColor(one.value).run()" />
              </UTooltip>
            </div>
          </template>
        </UPopover>
      </div>

      <div class="!px-0">
        <UDropdownMenu :items="insertItems(editor)" :content="{ align: 'start' }">
          <UTooltip :text="$t('mail.label.insert')">
            <UButton color="neutral" variant="ghost" size="sm" square icon="i-material-symbols-add-circle-outline" :aria-label="$t('mail.label.insert')" />
          </UTooltip>
        </UDropdownMenu>
      </div>

      <div v-if="editor.isActive('table')" class="!px-0">
        <UDropdownMenu :items="tableItems(editor)" :content="{ align: 'start' }">
          <UTooltip :text="$t('mail.label.table_edit')">
            <UButton color="neutral" variant="ghost" size="sm" square icon="i-material-symbols-table-chart-outline" :aria-label="$t('mail.label.table_edit')" />
          </UTooltip>
        </UDropdownMenu>
      </div>
    </ToolBar>
  </UEditor>

  <input ref="picker" type="file" accept="image/*" class="hidden" @change="onPick" />

  <UModal v-model:open="linkOpen" :title="$t('common.label.link')" :ui="{ footer: 'justify-end' }">
    <template #body>
      <UInput v-model="linkHref" class="w-full" placeholder="https://" :aria-label="$t('mail.placeholder.link_address')" autofocus @keydown.enter="applyLink" />
    </template>
    <template #footer>
      <UButton color="neutral" variant="ghost" :label="$t('common.action.cancel')" @click="linkOpen = false" />
      <UButton :label="$t('mail.label.insert')" :disabled="linkHref.trim() === ''" @click="applyLink" />
    </template>
  </UModal>
</template>

<script setup lang="ts">
import { TableKit } from "@tiptap/extension-table";
import { TextAlign } from "@tiptap/extension-text-align";
import { Color, TextStyle } from "@tiptap/extension-text-style";

import { editorToolbar, imageIn, MailImageError, readImageData } from "../domain/mail-editor";

import type { Editor } from "@tiptap/vue-3";
import type { DropdownMenuItem } from "@nuxt/ui";

defineOptions({ inheritAttrs: false });

const props = defineProps<{
  placeholder: string;
  // A signature stands in a section of a settings page and opens shorter than a message body does.
  compact?: boolean;
  class?: string;
}>();

const emit = defineEmits<{
  focused: [];
  failed: [reason: string];
}>();

const model = defineModel<string>({ required: true });
const { t } = useI18n();

// The colours a message's text is written in. A mail is read against backgrounds this console does
// not choose, so each one is dark enough to stand on white and light enough to stand on black.
const TEXT_COLORS = [
  { label: t("mail.label.color_red"), value: "#dc2626" },
  { label: t("mail.label.color_orange"), value: "#ea580c" },
  { label: t("mail.label.color_yellow"), value: "#ca8a04" },
  { label: t("mail.label.color_green"), value: "#16a34a" },
  { label: t("mail.label.color_blue"), value: "#2563eb" },
  { label: t("mail.label.color_purple"), value: "#7c3aed" },
  { label: t("mail.label.color_gray"), value: "#6b7280" },
];

// A table's lines are written into the markup rather than left to a stylesheet: what goes out is
// read by a mail client that has none of this console's styles.
const CELL_STYLE = "border:1px solid #d1d5db;padding:6px 8px;vertical-align:top";

const extensions = [
  TableKit.configure({
    table: { resizable: false, HTMLAttributes: { style: "border-collapse:collapse;width:100%" } },
    tableCell: { HTMLAttributes: { style: CELL_STYLE } },
    tableHeader: { HTMLAttributes: { style: `${CELL_STYLE};background:#f3f4f6;font-weight:600` } },
  }),
  TextStyle,
  Color,
  TextAlign.configure({ types: ["heading", "paragraph"] }),
];

// A picture is carried in the markup as its own bytes, so the editor reads and keeps one written as a
// data address; with the library's own setting it drops such a picture whenever markup is loaded.
const IMAGE = { allowBase64: true };

const TOOLBAR = editorToolbar(t);

// What a menu of this screen's own does. The editor comes from the slot that drew the menu, so
// nothing of it is held between renders.
function insertItems(editor: Editor): DropdownMenuItem[][] {
  return [
    [
      { label: t("common.label.link"), icon: "i-material-symbols-link", onSelect: () => openLink(editor) },
      { label: t("mail.label.picture"), icon: "i-material-symbols-image-outline", onSelect: () => picker.value?.click() },
    ],
    [
      { label: t("mail.label.table"), icon: "i-material-symbols-table-chart-outline", onSelect: () => editor.chain().focus().insertTable({ rows: 3, cols: 3, withHeaderRow: true }).run() },
      { label: t("mail.label.rule"), icon: "i-material-symbols-horizontal-rule", onSelect: () => editor.chain().focus().setHorizontalRule().run() },
    ],
  ];
}

function tableItems(editor: Editor): DropdownMenuItem[][] {
  return [
    [
      { label: t("mail.action.row_above"), icon: "i-material-symbols-add-row-above", onSelect: () => editor.chain().focus().addRowBefore().run() },
      { label: t("mail.action.row_below"), icon: "i-material-symbols-add-row-below", onSelect: () => editor.chain().focus().addRowAfter().run() },
      { label: t("mail.action.column_left"), icon: "i-material-symbols-add-column-left", onSelect: () => editor.chain().focus().addColumnBefore().run() },
      { label: t("mail.action.column_right"), icon: "i-material-symbols-add-column-right", onSelect: () => editor.chain().focus().addColumnAfter().run() },
    ],
    [
      { label: t("mail.action.merge_cells"), icon: "i-material-symbols-cell-merge", onSelect: () => editor.chain().focus().mergeOrSplit().run() },
      { label: t("mail.action.header_row"), icon: "i-material-symbols-table-rows-narrow-outline", onSelect: () => editor.chain().focus().toggleHeaderRow().run() },
    ],
    [
      { label: t("mail.action.delete_row"), icon: "i-material-symbols-delete-outline", onSelect: () => editor.chain().focus().deleteRow().run() },
      { label: t("mail.action.delete_column"), icon: "i-material-symbols-delete-outline", onSelect: () => editor.chain().focus().deleteColumn().run() },
      { label: t("mail.action.delete_table"), icon: "i-material-symbols-delete-forever-outline", onSelect: () => editor.chain().focus().deleteTable().run() },
    ],
  ];
}

// The editor itself, for the paste and the drop: those two are handed to it as it is built and so
// cannot be given the one a slot renders with. The component exposes it as a ref, which Vue unwraps on
// the exposed object, so the editor stands there as it is.
const board = useTemplateRef<{ editor: Editor | undefined }>("board");

function editing(): Editor | null {
  return board.value?.editor ?? null;
}

const linkOpen = ref(false);
const linkHref = ref("");

function openLink(editor: Editor): void {
  if (editor.isActive("link")) {
    editor.chain().focus().unsetLink().run();

    return;
  }

  linkHref.value = "";
  linkOpen.value = true;
}

function applyLink(): void {
  const href = linkHref.value.trim();
  const editor = editing();

  if (href === "" || editor === null) {
    return;
  }

  editor.chain().focus().extendMarkRange("link").setLink({ href }).run();
  linkOpen.value = false;
}

const picker = useTemplateRef<HTMLInputElement>("picker");

async function insertImage(file: File): Promise<void> {
  const editor = editing();

  if (editor === null) {
    return;
  }

  try {
    editor.chain().focus().setImage({ src: await readImageData(t, file) }).run();
  } catch (error) {
    emit("failed", error instanceof MailImageError ? error.message : t("mail.error.picture_insert_failed"));
  }
}

async function onPick(event: Event): Promise<void> {
  const input = event.target as HTMLInputElement;
  const file = input.files?.[0];

  input.value = "";

  if (file !== undefined) {
    await insertImage(file);
  }
}

// A picture pasted or dragged in is taken as a picture rather than as the file name the browser
// would otherwise drop in. Anything else is left to the editor's own handling.
function takeImage(data: DataTransfer | null): boolean {
  const file = imageIn([...(data?.files ?? [])]);

  if (file === null) {
    return false;
  }

  void insertImage(file);

  return true;
}

const editorProps = {
  handlePaste: (_view: unknown, event: ClipboardEvent): boolean => takeImage(event.clipboardData),
  handleDrop: (_view: unknown, event: DragEvent): boolean => takeImage(event.dataTransfer),
};
</script>
