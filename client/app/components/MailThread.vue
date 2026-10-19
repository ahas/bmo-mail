<template>
  <!-- One conversation (docs/mail.md, 「The screen」): its subject on a row of its own; under it the
       labels on the left and reply, reply-all and forward on the right; then the messages, the newest open and
       the older ones folded to a line each. On a phone it stands in the list's place, and the back
       button returns to the list where it was. -->
  <section class="flex min-h-0 flex-1 flex-col" :aria-label="$t('mail.aria.thread')">
    <p v-if="props.failure !== ''" role="alert" class="p-4 text-sm text-error">{{ props.failure }}</p>

    <div v-else-if="props.messages === null" class="p-4">
      <ContentSkeleton :rows="6" listClass="flex flex-col gap-3" :label="$t('mail.status.loading')">
        <div class="flex flex-col gap-2 rounded-lg border border-default px-4 py-3">
          <div class="flex h-6 items-center gap-2">
            <SkeletonBar class="size-6 shrink-0 rounded-full" />
            <SkeletonBar class="h-4 w-32" />
            <SkeletonBar class="ms-auto h-4 w-20" />
          </div>
          <SkeletonLine width="w-3/4" />
        </div>
      </ContentSkeleton>
    </div>

    <p v-else-if="props.messages.length === 0" class="p-6 text-center text-sm text-muted">{{ $t("mail.message.choose_mail") }}</p>

    <template v-else>
      <div class="flex shrink-0 flex-col gap-2 border-b border-default px-4 py-3">
        <div class="flex items-center gap-2">
          <UTooltip v-if="props.narrow" :text="$t('mail.action.back_to_list')">
            <UButton color="neutral" variant="ghost" square icon="i-material-symbols-arrow-back" :aria-label="$t('mail.action.back_to_list')" @click="emit('back')" />
          </UTooltip>
          <h2 class="min-w-0 flex-1 text-xl font-semibold break-words text-highlighted">{{ subjectText($t, subject) }}</h2>
        </div>
        <div class="flex items-center gap-2">
          <div class="flex min-w-0 flex-1 flex-wrap gap-1">
            <span v-for="label in threadLabels" :key="label.id" class="rounded-sm px-1.5 text-sm leading-6 text-inverted" :style="{ backgroundColor: label.color }">{{ label.name }}</span>
          </div>
          <div class="flex shrink-0 items-center gap-1">
            <UTooltip :text="$t('mail.action.reply')">
              <UButton color="neutral" variant="ghost" square icon="i-material-symbols-reply" :aria-label="$t('mail.action.reply')" @click="emit('write', 'reply', newest)" />
            </UTooltip>
            <UTooltip :text="$t('mail.action.reply_all')">
              <UButton color="neutral" variant="ghost" square icon="i-material-symbols-reply-all" :aria-label="$t('mail.action.reply_all')" @click="emit('write', 'reply-all', newest)" />
            </UTooltip>
            <UTooltip :text="$t('mail.action.forward')">
              <UButton color="neutral" variant="ghost" square icon="i-material-symbols-forward" :aria-label="$t('mail.action.forward')" @click="emit('write', 'forward', newest)" />
            </UTooltip>
          </div>
        </div>
      </div>

      <div class="min-h-0 flex-1 overflow-y-auto overscroll-contain">
        <MailMessageCard
          v-for="message in props.messages"
          :key="`${message.folder}:${message.uid}`"
          :message="message"
          :account="props.account"
          :address="props.address"
          :show-images="props.showImages"
          :open="opened.has(`${message.folder}:${message.uid}`)"
          @toggle="toggle(message)"
          @receipt="(send, done) => emit('receipt', message, send, done)"
          @write="(kind) => emit('write', kind, message)"
        />
      </div>
    </template>
  </section>
</template>

<script setup lang="ts">
import { subjectText } from "../domain/mail";

import type { MailLabel, MailMessage } from "../../shared/mail";

const props = defineProps<{
  messages: MailMessage[] | null;
  labels: MailLabel[];
  account: string;
  address: string;
  narrow: boolean;
  failure: string;
  // Whether the account shows a message's outside pictures as it opens.
  showImages: boolean;
}>();

const emit = defineEmits<{
  back: [];
  write: [kind: "reply" | "reply-all" | "forward", message: MailMessage];
  receipt: [message: MailMessage, send: boolean, done: () => void];
}>();

const newest = computed(() => props.messages![props.messages!.length - 1]!);
const subject = computed(() => props.messages?.[0]?.subject ?? "");
const threadLabels = computed(() => {
  const ids = new Set((props.messages ?? []).flatMap((message) => message.labels));

  return props.labels.filter((label) => ids.has(label.id));
});

const opened = ref(new Set<string>());

// A thread opens with its newest message open, and with every unread one, as Gmail opens it.
watch(
  () => props.messages,
  (messages) => {
    const last = messages?.[messages.length - 1];

    opened.value = new Set([...(messages ?? []).filter((message) => !message.seen), ...(last === undefined ? [] : [last])].map((message) => `${message.folder}:${message.uid}`));
  },
  { immediate: true },
);

function toggle(message: MailMessage): void {
  const key = `${message.folder}:${message.uid}`;
  const next = new Set(opened.value);

  if (next.has(key)) next.delete(key);
  else next.add(key);

  opened.value = next;
}
</script>
