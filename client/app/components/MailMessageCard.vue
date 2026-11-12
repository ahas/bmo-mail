<template>
  <!-- One message of a thread (docs/mail.md, 「The screen」 and 「Read receipts」). Folded, it is one
       line — the sender, the snippet, the date — and pressing it opens it. Open, it shows who wrote
       to whom with reply, reply-all and forward buttons for this message, the read-receipt bar where the message
       asks for one, the body and the attachments. -->
  <article class="border-b border-default">
    <button
      v-if="!props.open"
      type="button"
      class="flex w-full items-center gap-3 px-4 py-3 text-start transition-colors hover:bg-elevated active:bg-accented"
      @click="emit('toggle')"
    >
      <span class="w-40 shrink-0 truncate text-sm font-semibold text-highlighted">{{ sender }}</span>
      <span class="min-w-0 flex-1 truncate text-sm text-muted">{{ props.message.snippet || subjectText($t, props.message.subject) }}</span>
      <span class="shrink-0 text-sm tabular-nums text-muted">{{ mailDate(props.message.at) }}</span>
    </button>

    <div v-else class="flex flex-col gap-3 px-4 py-3">
      <div class="flex items-center gap-1">
        <button type="button" class="flex min-w-0 flex-1 items-center gap-3 text-start" @click="emit('toggle')">
          <MailAvatar :address="senderAddress" :name="sender" size="card" />
          <div class="flex min-w-0 flex-1 flex-col gap-1">
            <p class="flex min-w-0 flex-wrap items-center gap-x-2">
              <span class="text-base font-semibold text-highlighted">{{ sender }}</span>
              <span class="min-w-0 truncate text-sm text-muted">{{ senderAddress }}</span>
            </p>
            <p class="truncate text-sm text-muted">{{ $t("mail.label.to_line", { recipients }) }}</p>
          </div>
          <UTooltip :text="mailDateLong(props.message.at)">
            <span class="shrink-0 text-sm tabular-nums text-muted">{{ mailDate(props.message.at) }}</span>
          </UTooltip>
        </button>
        <div class="flex shrink-0 items-center gap-1">
          <UTooltip v-for="write in writes" :key="write.kind" :text="write.label">
            <UButton color="neutral" variant="ghost" size="sm" square :icon="write.icon" :aria-label="write.label" @click="emit('write', write.kind)" />
          </UTooltip>
        </div>
      </div>

      <div v-if="asksReceipt" class="flex flex-wrap items-center gap-2 rounded-md bg-elevated px-3 py-2 text-sm">
        <UIcon name="i-material-symbols-mark-email-read-outline" class="size-4 shrink-0 text-muted" />
        <span class="min-w-0 flex-1">{{ $t("mail.message.receipt_requested") }}</span>
        <UButton size="sm" color="neutral" variant="ghost" :label="$t('mail.action.receipt_decline')" :loading="answering === 'decline'" :disabled="answering !== null" @click="answer(false)" />
        <UButton size="sm" color="neutral" variant="outline" :label="$t('mail.action.send')" :loading="answering === 'send'" :disabled="answering !== null" @click="answer(true)" />
      </div>

      <MailBody :message="props.message" :account="props.account" :show-images="props.showImages" />

      <ul v-if="attachments.length > 0" class="flex flex-wrap gap-2" :aria-label="$t('mail.label.attachment')">
        <li v-for="part in attachments" :key="part.partId">
          <a
            :href="partUrl(props.account, props.message, part.partId)"
            :download="part.fileName ?? ''"
            class="flex max-w-64 items-center gap-2 rounded-md border border-default px-3 py-2 text-sm transition-colors hover:bg-elevated active:bg-accented"
          >
            <UIcon name="i-material-symbols-attach-file" class="size-4 shrink-0 text-muted" />
            <span class="min-w-0 flex-1 truncate">{{ part.fileName ?? $t("mail.label.attachment") }}</span>
            <span class="shrink-0 tabular-nums text-muted">{{ sizeText(partSize(part)) }}</span>
          </a>
        </li>
      </ul>
    </div>
  </article>
</template>

<script setup lang="ts">
import { mailDate, mailDateLong, personName, sizeText, subjectText } from "../domain/mail";
import { partUrl } from "../domain/mail-api";

import { attachmentsOf, partSize, receiptAnswered } from "../../shared/mail";

import type { MailMessage } from "../../shared/mail";

const props = defineProps<{ message: MailMessage; account: string; address: string; open: boolean; showImages: boolean }>();
const { t } = useI18n();

const emit = defineEmits<{
  toggle: [];
  receipt: [send: boolean, done: () => void];
  write: [kind: "reply" | "reply-all" | "forward"];
}>();

// Reply, reply-all and forward for this one message, beside the thread's own that answer the newest.
// The three answers to this message. A group of three stands as icon buttons, so the screen never shows a
// second more menu beside the list's.
const writes = computed(() => [
  { kind: "reply" as const, label: t("mail.action.reply"), icon: "i-material-symbols-reply" },
  { kind: "reply-all" as const, label: t("mail.action.reply_all"), icon: "i-material-symbols-reply-all" },
  { kind: "forward" as const, label: t("mail.action.forward"), icon: "i-material-symbols-forward" },
]);

const sender = computed(() => {
  const first = props.message.from;

  return first == null ? t("mail.label.no_sender_mark") : personName(t, first, props.address);
});
const senderAddress = computed(() => props.message.from?.address ?? "");
const attachments = computed(() => attachmentsOf(props.message));
const recipients = computed(() => [...props.message.to, ...props.message.cc].map((person) => personName(t, person, props.address)).join(", ") || t("mail.label.none_mark"));

// The bar stands for a message that asks and was not answered, and never for the account's own.
const asksReceipt = computed(
  () => props.message.receiptTo != null && !receiptAnswered(props.message) && props.message.from?.address !== props.address,
);

const answering = ref<"send" | "decline" | null>(null);

function answer(send: boolean): void {
  answering.value = send ? "send" : "decline";
  emit("receipt", send, () => {
    answering.value = null;
  });
}
</script>
