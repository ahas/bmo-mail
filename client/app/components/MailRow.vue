<template>
  <!-- One conversation in a list (docs/mail.md, 「The screen」). On a desktop it stands on one line,
       in the columns the list's header names: the leading cell with the checkbox, the star, the
       unread mark and the sender's avatar, then the sender, then the subject and snippet, then the
       attachment mark and the date. A row's words are cut at the width their cell has: the subject
       is what the row is read by, and the whole of it, with the message under it, is one press
       away, so nothing here is set moving to be finished on the row. The sender and the date take
       the widths the list hands down, so every row's cells stand on one line down the list; the
       subject takes whatever is left, and it is the only cell that grows. On a phone it is two lines — the
       senders and the date, then the labels, the subject and snippet with the star — and the list
       reads a long press on it as the start of choosing. Unread rows are bold and carry the unread
       mark. -->
  <div
    role="row"
    :aria-selected="props.selected"
    :data-row="props.conversation.key"
    class="group relative flex cursor-pointer items-center gap-2 border-b border-default px-3 transition-colors select-none hover:bg-elevated active:bg-accented"
    :class="[props.selected ? 'bg-primary/10' : props.focused ? 'bg-elevated' : '', props.compact ? 'py-2' : 'h-10', props.conversation.unread ? 'text-highlighted' : 'text-muted']"
    @click="onClick"
    @contextmenu="onContextMenu"
  >
    <template v-if="!props.compact">
      <div class="flex shrink-0 items-center gap-2" :style="{ width: `${MAIL_ROW_LEAD_PX}px` }">
        <UCheckbox
          :model-value="props.selected"
          :aria-label="$t('mail.aria.choose_named', { subject: subjectText($t, props.conversation.subject) })"
          @click.stop
          @update:model-value="emit('toggle')"
        />

        <UTooltip :text="props.conversation.flagged ? $t('mail.action.unstar') : $t('mail.action.star')">
          <UButton
            color="neutral"
            variant="ghost"
            size="sm"
            square
            :icon="props.conversation.flagged ? 'i-material-symbols-star' : 'i-material-symbols-star-outline'"
            :class="props.conversation.flagged ? 'text-warning' : ''"
            :aria-label="props.conversation.flagged ? $t('mail.action.unstar') : $t('mail.action.star')"
            @click.stop="emit('star')"
          />
        </UTooltip>

        <UIcon
          :name="props.conversation.unread ? 'i-material-symbols-circle' : 'i-material-symbols-circle-outline'"
          class="size-2 shrink-0"
          :class="props.conversation.unread ? 'text-primary' : 'text-transparent'"
          :aria-label="props.conversation.unread ? $t('mail.label.unread') : undefined"
        />

        <MailAvatar :address="senderAddress" :name="senders" size="row" />
      </div>

      <p class="shrink-0 truncate text-sm" :style="{ width: 'var(--mail-from)' }" :class="{ 'font-semibold': props.conversation.unread }">{{ senders }}</p>

      <div class="flex min-w-0 flex-1 items-center gap-2">
        <span v-for="label in rowLabels" :key="label.id" class="shrink-0 rounded-sm px-1.5 text-sm leading-6 text-inverted" :style="{ backgroundColor: label.colour }">{{ label.name }}</span>
        <p class="min-w-0 flex-1 truncate text-sm">
          <span :class="props.conversation.unread ? 'font-semibold' : 'text-default'">{{ subjectText($t, props.conversation.subject) }}</span>
          <span v-if="props.conversation.snippet !== ''" class="text-muted"> — {{ props.conversation.snippet }}</span>
        </p>
      </div>

      <UIcon v-if="props.conversation.hasAttachments" name="i-material-symbols-attach-file" class="size-4 shrink-0 text-muted" :aria-label="$t('mail.aria.has_attachment')" />
      <span v-else class="size-4 shrink-0" aria-hidden="true" />

      <UTooltip :text="mailDateLong(locale, props.conversation.at)">
        <span class="shrink-0 text-end text-sm tabular-nums" :style="{ width: 'var(--mail-date)' }" :class="{ 'font-semibold': props.conversation.unread }">{{ mailDate(locale, props.conversation.at) }}</span>
      </UTooltip>
    </template>

    <template v-else>
      <UCheckbox
        v-if="!props.narrow || props.choosing"
        :model-value="props.selected"
        :aria-label="$t('mail.aria.choose_named', { subject: subjectText($t, props.conversation.subject) })"
        @click.stop
        @update:model-value="emit('toggle')"
      />

      <MailAvatar :address="senderAddress" :name="senders" size="row" />

      <div class="flex min-w-0 flex-1 flex-col gap-1">
        <div class="flex min-w-0 items-center gap-2">
          <UIcon v-if="props.conversation.unread" name="i-material-symbols-circle" class="size-2 shrink-0 text-primary" :aria-label="$t('mail.label.unread')" />
          <p class="min-w-0 flex-1 truncate text-base" :class="{ 'font-semibold': props.conversation.unread }">{{ senders }}</p>
          <UIcon v-if="props.conversation.hasAttachments" name="i-material-symbols-attach-file" class="size-4 shrink-0 text-muted" :aria-label="$t('mail.aria.has_attachment')" />
          <span class="shrink-0 text-sm tabular-nums" :class="{ 'font-semibold': props.conversation.unread }">{{ mailDate(locale, props.conversation.at) }}</span>
        </div>
        <div class="flex min-w-0 items-center gap-2">
          <span v-for="label in rowLabels" :key="label.id" class="max-w-24 shrink-0 truncate rounded-sm px-1.5 text-sm leading-6 text-inverted" :style="{ backgroundColor: label.colour }">{{ label.name }}</span>
          <p class="min-w-0 flex-1 truncate text-sm">
            <span :class="props.conversation.unread ? 'font-semibold' : 'text-default'">{{ subjectText($t, props.conversation.subject) }}</span>
            <span v-if="props.conversation.snippet !== ''" class="text-muted"> — {{ props.conversation.snippet }}</span>
          </p>
          <UTooltip :text="props.conversation.flagged ? $t('mail.action.unstar') : $t('mail.action.star')">
            <UButton
              color="neutral"
              variant="ghost"
              size="sm"
              square
              :icon="props.conversation.flagged ? 'i-material-symbols-star' : 'i-material-symbols-star-outline'"
              :class="props.conversation.flagged ? 'text-warning' : ''"
              :aria-label="props.conversation.flagged ? $t('mail.action.unstar') : $t('mail.action.star')"
              @click.stop="emit('star')"
              @touchstart.stop
            />
          </UTooltip>
        </div>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { mailDate, mailDateLong, MAIL_ROW_LEAD_PX, sendersLine, subjectText } from "../domain/mail";

import type { MailConversation, MailLabel } from "../../shared/mail";

const props = defineProps<{
  conversation: MailConversation;
  labels: MailLabel[];
  account: string;
  selected: boolean;
  focused: boolean;
  narrow: boolean;
  // Whether the row stands on two lines: on a phone, and in a list too narrow for one.
  compact: boolean;
  // Whether a phone is choosing rows: a tap then chooses instead of opening.
  choosing: boolean;
}>();

const emit = defineEmits<{
  open: [];
  toggle: [];
  star: [];
}>();

const { t, locale } = useI18n();
const senders = computed(() => sendersLine(t, props.conversation, props.account));
// The avatar stands for whoever the row leads with, which is the newest message's sender.
const senderAddress = computed(() => props.conversation.from[0]?.address ?? "");
const rowLabels = computed(() => props.labels.filter((label) => props.conversation.labelIds.includes(label.id)));

// A long press on a phone raises the platform's own menu too; the hold the list reads off the same
// press is what answers it.
function onContextMenu(event: MouseEvent): void {
  if (props.narrow) {
    event.preventDefault();
  }
}

function onClick(): void {
  if (props.narrow && props.choosing) {
    emit("toggle");
  } else {
    emit("open");
  }
}
</script>
