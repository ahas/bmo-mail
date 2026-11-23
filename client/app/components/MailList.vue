<template>
  <!-- A list of conversations (docs/mail.md, 「The screen」): the search box, the toolbar, the column
       header and the rows. The toolbar's groups stand divided: choosing, refresh, mark-all-read and
       the unread-only filter; with something chosen, its state, its place and its labels; and the
       range with previous and next at the right end. Where a row stands on one line the header names
       its columns and has its edges dragged to set the widths; where a row stacks instead there is
       no header. The list stands newest first. On a phone the search shares its row with the folder button,
       and choosing turns the row into the select-all checkbox, the count, the move-to-Trash and
       mark-read buttons, the move and label selects and one menu with the rest; a finger held on a row
       starts choosing and dragging on from it chooses every row it passes. The select-all checkbox is the same one the
       wide toolbar carries, so the choice is
       taken whole at either width.

       The toolbar gathers by the room it has rather than by the window: this list stands in a panel
       beside a thread and is often narrower than a phone on a wide screen, and a bar that asked the
       window would stand its whole row of actions there and run off its own edge. Short of that
       room it keeps what is reached for first — the choosing control, and with something chosen
       move-to-Trash, mark-read and the move and label selects — and folds the rest into one menu.

       On an account whose kind moves no mail between folders, nothing that would is drawn at any
       width: no trash or delete forever, no spam or not spam, and no move select. -->
  <div class="relative flex min-h-0 flex-1 flex-col" :style="{ '--mail-from': `${fromWidth}px`, '--mail-date': `${dateWidth}px` }">
    <div class="flex shrink-0 items-center gap-2 border-b border-default p-3">
      <UButton
        v-if="props.narrow"
        color="neutral"
        variant="outline"
        icon="i-material-symbols-menu"
        :label="props.title"
        class="max-w-[40%] shrink-0"
        :ui="{ label: 'truncate' }"
        @click="emit('nav')"
      />
      <UInput
        ref="searchBox"
        v-model="typed"
        class="min-w-0 flex-1"
        icon="i-material-symbols-search"
        :placeholder="$t('mail.placeholder.search')"
        :aria-label="$t('mail.placeholder.search')"
        :ui="{ trailing: 'pe-1' }"
        @keydown.enter="submitSearch"
      >
        <template v-if="typed !== ''" #trailing>
          <UTooltip :text="$t('mail.action.clear_search')">
            <UButton size="sm" color="neutral" variant="link" icon="i-material-symbols-close" :aria-label="$t('mail.action.clear_search')" @click="clearSearch" />
          </UTooltip>
        </template>
      </UInput>
    </div>

    <ToolBar ref="bar" :label="$t('mail.aria.list_tools')" class="shrink-0">
      <template v-if="gathered && props.choosing">
        <!-- The select-all checkbox is the first thing in the row, so it stands on the same vertical
             line as the marks down the rows it speaks for; the way out of the choosing stands at
             the far end, where a closing belongs. -->
        <div>
          <ChooseAll :keys="shownKeys" :selected="props.selected" @choose="(all) => emit('select', all ? 'all' : 'none')" />
          <RollingSentence class="text-sm" :keypath="'mail.label.count'" :figures="{ n: props.selected.size }" />
        </div>
        <div class="ms-auto">
          <UTooltip v-if="props.moves" :text="trashLabel">
            <UButton color="neutral" variant="ghost" size="sm" square :icon="trashIcon" :aria-label="trashLabel" :disabled="props.selected.size === 0" @click="emit('act', inTrashOrJunk ? 'delete' : 'trash')" />
          </UTooltip>
          <UTooltip :text="state.anyUnread ? $t('mail.action.mark_read') : $t('mail.action.mark_unread')">
            <UButton
              color="neutral"
              variant="ghost"
              size="sm"
              square
              :icon="state.anyUnread ? 'i-material-symbols-mark-email-read-outline' : 'i-material-symbols-mark-email-unread-outline'"
              :aria-label="state.anyUnread ? $t('mail.action.mark_read') : $t('mail.action.mark_unread')"
              :disabled="props.selected.size === 0"
              @click="emit('act', state.anyUnread ? 'read' : 'unread')"
            />
          </UTooltip>
          <MailToolChoice v-if="props.moves" v-bind="moveChoice" :disabled="props.selected.size === 0" @update:model-value="moveTo" />
          <MailLabelChoice :disabled="props.selected.size === 0" :labels="props.labels" :all="labelState.all" :some="labelState.some" @toggle="(id, on) => emit('label', id, on)" />
          <UDropdownMenu :items="narrowMenu" :content="{ align: 'end' }">
            <UTooltip :text="$t('common.action.more')">
              <UButton color="neutral" variant="ghost" size="sm" square icon="i-material-symbols-more-vert" :aria-label="$t('common.action.more')" :disabled="props.selected.size === 0" />
            </UTooltip>
          </UDropdownMenu>
        </div>
        <div>
          <UTooltip :text="$t('mail.action.end_choosing')">
            <UButton color="neutral" variant="ghost" size="sm" square icon="i-material-symbols-close" :aria-label="$t('mail.action.end_choosing')" @click="emit('stop-choosing')" />
          </UTooltip>
        </div>
      </template>

      <template v-else>
        <div>
          <!-- The choosing control never folds into a menu, so it stands here at every width; the
               phone has no place for it while nothing is being chosen, where a held finger is what
               starts the choosing instead. -->
          <template v-if="!props.narrow">
            <ChooseAll :keys="shownKeys" :selected="props.selected" @choose="(all) => emit('select', all ? 'all' : 'none')" />
            <UDropdownMenu :items="selectMenu" :content="{ align: 'start' }">
              <UTooltip :text="$t('mail.action.choose')">
                <UButton color="neutral" variant="ghost" size="sm" square icon="i-material-symbols-arrow-drop-down" :aria-label="$t('mail.action.choose')" />
              </UTooltip>
            </UDropdownMenu>
          </template>

          <template v-if="props.selected.size === 0">
            <template v-if="!gathered">
              <UTooltip :text="$t('mail.action.refresh')">
                <UButton color="neutral" variant="ghost" size="sm" square icon="i-material-symbols-refresh" :aria-label="$t('mail.action.refresh')" :loading="props.loading" @click="emit('refresh')" />
              </UTooltip>
              <UTooltip v-if="props.readAll" :text="$t('mail.action.read_all')">
                <UButton color="neutral" variant="ghost" size="sm" square icon="i-material-symbols-mark-email-read-outline" :aria-label="$t('mail.action.read_all')" :loading="props.markingRead" @click="emit('read-all')" />
              </UTooltip>
              <UTooltip :text="unreadOnlyLabel">
                <UButton
                  :color="props.unreadOnly ? 'primary' : 'neutral'"
                  :variant="props.unreadOnly ? 'solid' : 'ghost'"
                  size="sm"
                  square
                  :icon="props.unreadOnly ? 'i-material-symbols-filter-alt' : 'i-material-symbols-filter-alt-outline'"
                  :aria-label="unreadOnlyLabel"
                  :aria-pressed="props.unreadOnly"
                  @click="emit('unread-only', !props.unreadOnly)"
                />
              </UTooltip>
            </template>

            <UDropdownMenu v-else :items="listMenu" :content="{ align: 'start' }">
              <UTooltip :text="$t('common.action.more')">
                <UButton color="neutral" variant="ghost" size="sm" square icon="i-material-symbols-more-vert" :aria-label="$t('common.action.more')" :loading="props.loading || props.markingRead" />
              </UTooltip>
            </UDropdownMenu>
          </template>
        </div>

        <template v-if="!props.narrow && props.selected.size > 0 && !gathered">
          <div>
            <UTooltip :text="state.anyUnread ? $t('mail.action.mark_read') : $t('mail.action.mark_unread')">
              <UButton
                color="neutral"
                variant="ghost"
                size="sm"
                square
                :icon="state.anyUnread ? 'i-material-symbols-mark-email-read-outline' : 'i-material-symbols-mark-email-unread-outline'"
                :aria-label="state.anyUnread ? $t('mail.action.mark_read') : $t('mail.action.mark_unread')"
                @click="emit('act', state.anyUnread ? 'read' : 'unread')"
              />
            </UTooltip>
            <UTooltip :text="state.allStarred ? $t('mail.action.unstar') : $t('mail.action.star')">
              <UButton
                color="neutral"
                variant="ghost"
                size="sm"
                square
                :icon="state.allStarred ? 'i-material-symbols-star' : 'i-material-symbols-star-outline'"
                :aria-label="state.allStarred ? $t('mail.action.unstar') : $t('mail.action.star')"
                @click="emit('act', state.allStarred ? 'unstar' : 'star')"
              />
            </UTooltip>
          </div>
          <div v-if="props.moves">
            <UTooltip :text="trashLabel">
              <UButton color="neutral" variant="ghost" size="sm" square :icon="trashIcon" :aria-label="trashLabel" @click="emit('act', inTrashOrJunk ? 'delete' : 'trash')" />
            </UTooltip>
            <UTooltip :text="props.role === 'junk' ? $t('mail.action.not_spam') : $t('mail.action.report_spam')">
              <UButton
                color="neutral"
                variant="ghost"
                size="sm"
                square
                :icon="props.role === 'junk' ? 'i-material-symbols-inbox-outline' : 'i-material-symbols-report-outline'"
                :aria-label="props.role === 'junk' ? $t('mail.action.not_spam') : $t('mail.action.report_spam')"
                @click="emit('act', props.role === 'junk' ? 'not-spam' : 'spam')"
              />
            </UTooltip>
            <MailToolChoice v-bind="moveChoice" @update:model-value="moveTo" />
          </div>
          <div>
            <MailLabelChoice :labels="props.labels" :all="labelState.all" :some="labelState.some" @toggle="(id, on) => emit('label', id, on)" />
          </div>
        </template>

        <!-- The same actions on a bar short of room: move-to-Trash and mark-read, which is what a
             hand reaches for first, and the rest in the one menu a phone's choosing row already
             carries. -->
        <div v-else-if="!props.narrow && props.selected.size > 0">
          <UTooltip v-if="props.moves" :text="trashLabel">
            <UButton color="neutral" variant="ghost" size="sm" square :icon="trashIcon" :aria-label="trashLabel" @click="emit('act', inTrashOrJunk ? 'delete' : 'trash')" />
          </UTooltip>
          <UTooltip :text="state.anyUnread ? $t('mail.action.mark_read') : $t('mail.action.mark_unread')">
            <UButton
              color="neutral"
              variant="ghost"
              size="sm"
              square
              :icon="state.anyUnread ? 'i-material-symbols-mark-email-read-outline' : 'i-material-symbols-mark-email-unread-outline'"
              :aria-label="state.anyUnread ? $t('mail.action.mark_read') : $t('mail.action.mark_unread')"
              @click="emit('act', state.anyUnread ? 'read' : 'unread')"
            />
          </UTooltip>
          <MailToolChoice v-if="props.moves" v-bind="moveChoice" @update:model-value="moveTo" />
          <MailLabelChoice :labels="props.labels" :all="labelState.all" :some="labelState.some" @toggle="(id, on) => emit('label', id, on)" />
          <UDropdownMenu :items="narrowMenu" :content="{ align: 'start' }">
            <UTooltip :text="$t('common.action.more')">
              <UButton color="neutral" variant="ghost" size="sm" square icon="i-material-symbols-more-vert" :aria-label="$t('common.action.more')" />
            </UTooltip>
          </UDropdownMenu>
        </div>

        <div v-if="rangeShown" class="ms-auto">
          <!-- A count is a claim, so before the list has come back its place holds a figure's bar
               rather than a nought. -->
          <SkeletonBar v-if="props.conversations === null" class="inline-block h-4 w-16 align-middle" />
          <span v-else class="text-sm tabular-nums text-muted">{{ range }}</span>
          <UTooltip :text="$t('mail.action.previous')">
            <UButton color="neutral" variant="ghost" size="sm" square icon="i-material-symbols-chevron-left" :aria-label="$t('mail.action.previous')" :disabled="props.page === 1" @click="emit('page', -1)" />
          </UTooltip>
          <UTooltip :text="$t('mail.action.next')">
            <UButton color="neutral" variant="ghost" size="sm" square icon="i-material-symbols-chevron-right" :aria-label="$t('mail.action.next')" :disabled="!props.more" @click="emit('page', 1)" />
          </UTooltip>
        </div>

        <!-- The screen's one create button, a cell of its own at the far right. The screen hands it
             in, since what it makes and what a press does are the screen's; it is told the class
             that puts its cell at the right end where the range has stepped aside. -->
        <slot name="create" :cell-class="rangeShown ? '' : 'ms-auto'" />
      </template>
    </ToolBar>

    <p v-if="props.failure !== ''" role="alert" class="p-4 text-sm text-error">{{ props.failure }}</p>

    <div
      ref="rows"
      class="min-h-0 flex-1 overflow-y-auto overscroll-contain pb-(--bmo-create-room)"
      role="grid"
      :aria-label="$t('mail.aria.list')"
    >
      <!-- The header sticks to the top of the list it names, so a column's name stands over its
           column however far the list is read. It is here only while a row is one line of cells:
           over rows that stack, a column name would name nothing. -->
      <div v-if="!compactRows" role="row" class="sticky top-0 z-20 flex h-8 items-center gap-2 border-b border-default bg-default px-3 text-sm text-muted">
        <span class="shrink-0" :style="{ width: `${MAIL_ROW_LEAD_PX}px` }" aria-hidden="true" />

        <div role="columnheader" class="relative shrink-0" :style="{ width: 'var(--mail-from)' }">
          <span class="flex h-8 w-full min-w-0 items-center px-1"><span class="truncate">{{ $t("mail.label.column_from") }}</span></span>
          <span
            class="group/handle absolute inset-y-0 -end-2 flex w-4 cursor-col-resize touch-none items-center justify-center"
            role="separator"
            aria-orientation="vertical"
            :aria-label="$t('mail.aria.resize_from')"
            @pointerdown.prevent.stop="startResize('from', $event)"
            @pointermove="onResize"
            @pointerup="endResize"
            @pointercancel="endResize"
            @dblclick.stop="setWidth('from', null)"
          >
            <span class="h-4 w-px transition-colors group-hover/handle:bg-primary" :class="dragging?.column === 'from' ? 'bg-primary' : 'bg-[var(--ui-border)]'" />
          </span>
        </div>

        <div role="columnheader" class="min-w-0 flex-1">
          <span class="flex h-8 w-full min-w-0 items-center px-1"><span class="truncate">{{ $t("mail.label.subject") }}</span></span>
        </div>

        <span class="size-4 shrink-0" aria-hidden="true" />

        <div role="columnheader" aria-sort="descending" class="relative shrink-0" :style="{ width: 'var(--mail-date)' }">
          <span class="flex h-8 w-full min-w-0 items-center justify-end px-1"><span class="truncate">{{ $t("mail.label.column_date") }}</span></span>
          <span
            class="group/handle absolute inset-y-0 -start-2 flex w-4 cursor-col-resize touch-none items-center justify-center"
            role="separator"
            aria-orientation="vertical"
            :aria-label="$t('mail.aria.resize_date')"
            @pointerdown.prevent.stop="startResize('date', $event)"
            @pointermove="onResize"
            @pointerup="endResize"
            @pointercancel="endResize"
            @dblclick.stop="setWidth('date', null)"
          >
            <span class="h-4 w-px transition-colors group-hover/handle:bg-primary" :class="dragging?.column === 'date' ? 'bg-primary' : 'bg-[var(--ui-border)]'" />
          </span>
        </div>
      </div>

      <ContentSkeleton v-if="props.conversations === null" :rows="12" :label="$t('mail.status.loading')">
        <div class="flex h-10 items-center gap-2 border-b border-default px-3">
          <span class="shrink-0" :style="{ width: `${MAIL_ROW_LEAD_PX}px` }"><SkeletonBar class="h-4 w-full" /></span>
          <SkeletonBar class="h-4 shrink-0" :style="{ width: 'var(--mail-from)' }" />
          <SkeletonBar class="h-4 min-w-0 flex-1" />
          <SkeletonBar class="h-4 shrink-0" :style="{ width: 'var(--mail-date)' }" />
        </div>
      </ContentSkeleton>
      <p v-else-if="props.conversations.length === 0" class="p-6 text-center text-sm text-muted">{{ emptyText }}</p>
      <MailRow
        v-for="(conversation, index) in props.conversations ?? []"
        v-else
        :key="conversation.key"
        :conversation="conversation"
        :labels="props.labels"
        :account="props.address"
        :selected="props.selected.has(conversation.key)"
        :focused="index === props.focused"
        :narrow="props.narrow"
        :compact="compactRows"
        :choosing="props.choosing"
        :draggable="!props.narrow"
        @dragstart="carry($event, conversation)"
        @open="emit('open', conversation)"
        @toggle="emit('toggle', conversation.key)"
        @star="emit('star', conversation)"
      />
    </div>

    <!-- What the sender and the date columns are wide enough for, measured off the rows this page
         holds rather than guessed: a column that nobody has dragged takes the width its own
         contents ask for, capped, so the subject keeps everything else. It is measured while the
         rows stack too, since whether they fit on one line is read off these widths. -->
    <div v-if="!props.narrow" aria-hidden="true" class="pointer-events-none invisible absolute start-0 top-0 -z-10 flex gap-8">
      <div ref="measureFrom" class="flex flex-col items-start">
        <span v-for="one in shown" :key="one.key" class="text-sm whitespace-nowrap" :class="{ 'font-semibold': one.unread }">{{ sendersLine($t, one, props.address) }}</span>
      </div>
      <div ref="measureDate" class="flex flex-col items-start">
        <span v-for="one in shown" :key="one.key" class="text-sm tabular-nums whitespace-nowrap" :class="{ 'font-semibold': one.unread }">{{ mailDate(locale, one.at) }}</span>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { barGathers, useBarRoom } from "bmo-ui/app/domain/actionbar";
import { useDragSelect } from "bmo-ui/app/domain/dragselect";
import { clampColumn, labelsState, mailDate, MAIL_COLUMN_BOUNDS, MAIL_DRAG, MAIL_ROW_LEAD_PX, rowFit, rangeText, SELECT_KINDS, selectionState, sendersLine } from "../domain/mail";
import { useWidth } from "bmo-ui/app/domain/screen";
import { useMailColumns } from "../domain/mail-columns";

import type { MoveTarget, SelectKind } from "../domain/mail";
import type { FolderRole, MailConversation, MailLabel } from "../../shared/mail";
import type { DropdownMenuItem } from "@nuxt/ui";

export type MailListAction = "read" | "unread" | "star" | "unstar" | "trash" | "delete" | "spam" | "not-spam";

const { t, locale } = useI18n();
const props = defineProps<{
  conversations: MailConversation[] | null;
  // How many messages the folder holds, where the view is one folder and holds back nothing.
  total: number | null;
  // How many messages the pages before this one held, and this page's number from 1.
  offset: number;
  page: number;
  // Whether older messages stand past this page.
  more: boolean;
  labels: MailLabel[];
  moveTargets: MoveTarget[];
  // Whether the account's mailbox moves mail between folders; where it does not, the list draws nothing
  // that would: trash, delete forever, spam and not spam, and the move select.
  moves: boolean;
  view: string;
  title: string;
  // The folder role the list's actions work from, when it is one folder's list.
  role: FolderRole | null;
  address: string;
  query: string;
  // Whether the list holds back what was read.
  unreadOnly: boolean;
  // Whether the view is one folder, which mark-all-read marks whole.
  readAll: boolean;
  selected: Set<string>;
  focused: number;
  loading: boolean;
  failure: string;
  narrow: boolean;
  choosing: boolean;
  // Whether the whole view is being marked read, so the button says so while it runs.
  markingRead: boolean;
}>();

const emit = defineEmits<{
  open: [conversation: MailConversation];
  toggle: [key: string];
  hold: [key: string];
  // What a drag across the rows chose, whole.
  choose: [keys: Set<string>];
  star: [conversation: MailConversation];
  select: [kind: SelectKind];
  act: [action: MailListAction];
  move: [target: MoveTarget];
  label: [id: string, on: boolean];
  refresh: [];
  "read-all": [];
  // A page forward, older, or back.
  page: [step: 1 | -1];
  search: [query: string];
  "unread-only": [on: boolean];
  nav: [];
  "stop-choosing": [];
}>();

defineSlots<{
  // The screen's create button, standing last in the toolbar; its cell takes the class handed here.
  create?: (scope: { cellClass: string }) => unknown;
}>();

const typed = ref(props.query);

// The toolbar gathers by the room it has and not by the window's width: this list often stands in
// a panel narrower than a phone beside an open thread, and a bar that read the window would stand
// its whole row there. The phone gathers whatever it measures, since its row has other work to do.
const barRef = useTemplateRef<{ $el: HTMLElement }>("bar");
const barRoom = useBarRoom(() => barRef.value?.$el);
const gathered = computed(() => props.narrow || barGathers(barRoom.value));

// A row stands on one line where the list is wide enough for its cells, and on two where it is not
// — beside the thread on a desktop the list is often narrower than a phone. How wide is enough is
// read off the cells themselves (`rowFit`), further down where the column widths are known.
const rowsRef = useTemplateRef<HTMLElement>("rows");
const rowsWidth = useWidth(() => rowsRef.value, Number.POSITIVE_INFINITY);
const searchBox = useTemplateRef<{ inputRef?: HTMLInputElement }>("searchBox");

// A held finger starts choosing and goes on to choose every row it passes, the same gesture the
// console's other lists of many are dragged through. Only a phone chooses this way: a wide list
// has its checkboxes and its select menu in view the whole time.
const chosen = computed<Set<string>>({
  get: () => props.selected,
  set: (next) => emit("choose", next)
});

useDragSelect({
  surface: () => rowsRef.value,
  scroller: () => rowsRef.value,
  rowMark: "data-row",
  keys: () => (props.conversations ?? []).map((one) => one.key),
  selected: chosen,
  press: () => (props.narrow ? "hold" : "none"),
  hold: (key) => emit("hold", key)
});

watch(
  () => props.query,
  (query) => {
    typed.value = query;
  },
);

function submitSearch(): void {
  emit("search", typed.value.trim());
}

function clearSearch(): void {
  typed.value = "";
  emit("search", "");
}

// A wide list's row carried to the mail nav takes what is chosen with it, or itself alone where it is not chosen.
function carry(event: DragEvent, conversation: MailConversation): void {
  if (!props.selected.has(conversation.key)) emit("choose", new Set([conversation.key]));
  event.dataTransfer?.setData(MAIL_DRAG, conversation.key);
  if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
}

defineExpose({
  focusSearch: () => searchBox.value?.inputRef?.focus(),
});

const shown = computed(() => props.conversations ?? []);
const shownKeys = computed(() => shown.value.map((one) => one.key));
const fit = computed(() => rowFit(rowsWidth.value, widthOf("from"), widthOf("date")));
const compactRows = computed(() => props.narrow || !fit.value.oneLine);
// The range steps aside while anything is chosen in a list narrower than its cells need.
const rangeShown = computed(() => props.narrow || props.selected.size === 0 || !compactRows.value);
const state = computed(() => selectionState(props.selected, shown.value));
const labelState = computed(() => labelsState(props.selected, shown.value));
const inTrashOrJunk = computed(() => props.role === "trash" || props.role === "junk");
const trashLabel = computed(() => (inTrashOrJunk.value ? t("mail.action.delete_forever") : t("mail.action.trash")));
const trashIcon = computed(() => (inTrashOrJunk.value ? "i-material-symbols-delete-forever-outline" : "i-material-symbols-delete-outline"));
const range = computed(() => rangeText(props.offset, shown.value.reduce((sum, one) => sum + one.count, 0), props.total));
const unreadOnlyLabel = computed(() => (props.unreadOnly ? t("mail.action.show_all") : t("mail.action.unread_only")));
const emptyText = computed(() => {
  if (props.view === "search") return t("mail.status.none_found");

  return props.unreadOnly ? t("mail.status.none_unread") : t("mail.status.none");
});

// The sender and the date stand in columns of the width they were dragged to, and where they were
// never dragged, of the width this page's own rows ask for.
const { widths, setWidth } = useMailColumns();
const natural = ref({ from: MAIL_COLUMN_BOUNDS.from.fallback, date: MAIL_COLUMN_BOUNDS.date.fallback });
const dragging = ref<{ column: "from" | "date"; from: number; width: number } | null>(null);
const live = ref<{ column: "from" | "date"; width: number } | null>(null);
const measureFrom = useTemplateRef<HTMLElement>("measureFrom");
const measureDate = useTemplateRef<HTMLElement>("measureDate");

function widthOf(column: "from" | "date"): number {
  if (live.value?.column === column) return live.value.width;

  return widths.value[column] ?? natural.value[column];
}

// The sender stands at its own width where the row has room for it, and gives way before the
// subject does where it has not.
const fromWidth = computed(() => fit.value.from);
const dateWidth = computed(() => widthOf("date"));

// A line measured to the pixel is a line the marquee reads as one character too long, so each
// column is given the little the measurement rounds away.
const SLACK_PX = 2;

function measure(): void {
  // A list still being read has no rows to measure, and measuring none would put every column at
  // its floor and then widen it again the moment the rows arrive.
  if (measureFrom.value === null || measureDate.value === null || shown.value.length === 0) return;

  natural.value = {
    from: clampColumn("from", measureFrom.value.offsetWidth + SLACK_PX),
    date: clampColumn("date", measureDate.value.offsetWidth + SLACK_PX),
  };
}

watch([shown, () => props.narrow], () => void nextTick(measure));
onMounted(() => void nextTick(measure));

function startResize(column: "from" | "date", event: PointerEvent): void {
  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  dragging.value = { column, from: event.clientX, width: widthOf(column) };
  live.value = { column, width: widthOf(column) };
}

// The sender's edge is its trailing one and the date's is its leading one, so the two are dragged
// in opposite directions to make the same column wider.
function onResize(event: PointerEvent): void {
  const drag = dragging.value;

  if (drag === null) return;

  const moved = drag.column === "from" ? event.clientX - drag.from : drag.from - event.clientX;
  live.value = { column: drag.column, width: clampColumn(drag.column, drag.width + moved) };
}

function endResize(event: PointerEvent): void {
  const drag = dragging.value;

  if (drag === null) return;

  (event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId);
  setWidth(drag.column, live.value?.width ?? drag.width);
  dragging.value = null;
  live.value = null;
}

const selectMenu = computed<DropdownMenuItem[]>(() => SELECT_KINDS.map((one) => ({ label: t(one.label), onSelect: () => emit("select", one.kind) })));

// What the list's own actions become on a bar short of room. The unread-only filter keeps its
// pressed state as the menu's own check.
const listMenu = computed<DropdownMenuItem[][]>(() => [
  [
    { label: t("mail.action.refresh"), icon: "i-material-symbols-refresh", onSelect: () => emit("refresh") },
    ...(props.readAll ? [{ label: t("mail.action.read_all"), icon: "i-material-symbols-mark-email-read-outline", onSelect: () => emit("read-all") }] : []),
    {
      label: t("mail.action.unread_only"),
      icon: "i-material-symbols-filter-alt-outline",
      type: "checkbox" as const,
      checked: props.unreadOnly,
      onSelect: () => emit("unread-only", !props.unreadOnly),
    },
  ],
]);

// Where the chosen mail can be moved: the inbox, the Trash folder, and the mailbox's other folders, as
// the move select's choices, chosen once and then gone, since a move is an act and not a value held.
const moveChoice = computed(() => ({
  icon: "i-material-symbols-drive-file-move-outline",
  label: t("mail.action.move"),
  items: props.moveTargets.map((target) => ({ label: target.label, icon: target.icon, value: target.key })),
  valueKey: "value",
  modelValue: undefined
}));

function moveTo(key: string): void {
  const target = props.moveTargets.find((one) => one.key === key);

  if (target !== undefined) emit("move", target);
}

// What a gathered row folds into its one menu: the selection's state and, where the account moves mail,
// the spam report. Move and labels keep their own selects beside it, since each picks from a list that
// searches once it is long.
const narrowMenu = computed<DropdownMenuItem[][]>(() => [
  [
    { label: state.value.allStarred ? t("mail.action.unstar") : t("mail.action.star"), icon: "i-material-symbols-star-outline", onSelect: () => emit("act", state.value.allStarred ? "unstar" : "star") },
    ...(props.moves
      ? [{ label: props.role === "junk" ? t("mail.action.not_spam") : t("mail.action.report_spam"), icon: "i-material-symbols-report-outline", onSelect: () => emit("act", props.role === "junk" ? "not-spam" : "spam") }]
      : []),
  ],
]);
</script>
