<template>
  <!-- What the composer holds (docs/mail.md, 「Writing」): from, to, cc and bcc with completion, the
       subject, the body in the mail editor, the attachments, and at the foot the attachment,
       read-receipt and discard buttons with the Send button at the right. Each row's label stands in
       a column of its own, wide enough for the longest of them on one line, and the field takes the
       rest; the cc and bcc rows open from the to row and fold away again when the body is written in
       with nothing in them. -->
  <div class="flex min-h-0 flex-1 flex-col">
    <div class="flex min-h-10 items-center gap-2 border-b border-default px-3">
      <span :class="MAIL_FIELD_LABEL">{{ $t("mail.label.sender") }}</span>
      <USelect
        :model-value="draft.from"
        :items="props.accounts.map((one) => ({ label: one.address, value: one.address }))"
        variant="none"
        class="min-w-0 flex-1"
        :aria-label="$t('mail.label.sender')"
        @update:model-value="(value) => emit('from', String(value))"
      />
    </div>

    <MailAddressField v-model="draft.to" :label="$t('mail.label.to')" :api="props.api">
      <UButton v-if="!showCopies" color="neutral" variant="link" :label="$t('mail.label.cc')" :ui="{ base: 'px-0' }" @click="copies = true" />
      <UTooltip v-else-if="!hasCopies" :text="$t('mail.action.cc_collapse')">
        <UButton size="sm" color="neutral" variant="ghost" square icon="i-material-symbols-expand-less" :aria-label="$t('mail.action.cc_collapse')" @click="copies = false" />
      </UTooltip>
    </MailAddressField>
    <template v-if="showCopies">
      <MailAddressField v-model="draft.cc" :label="$t('mail.label.cc')" :api="props.api" />
      <MailAddressField v-model="draft.bcc" :label="$t('mail.label.bcc')" :api="props.api" />
    </template>

    <div class="flex min-h-10 items-center border-b border-default px-3">
      <UInput v-model="draft.subject" variant="none" :placeholder="$t('mail.label.subject')" :aria-label="$t('mail.label.subject')" class="min-w-0 flex-1" :ui="{ base: 'px-0' }" />
    </div>

    <div class="min-h-0 flex-1 overflow-y-auto overscroll-contain">
      <AsyncMailEditor v-model="draft.html" :placeholder="$t('mail.placeholder.body')" class="flex min-h-full flex-col" @focused="foldCopies" @failed="emit('failed', $event)" />
    </div>

    <ul v-if="draft.attachments.length > 0 || uploading > 0" class="flex shrink-0 flex-wrap gap-2 border-t border-default px-3 py-2" :aria-label="$t('mail.label.attachment')">
      <li v-for="(one, index) in draft.attachments" :key="`${one.kind}:${one.filename}:${index}`" class="flex max-w-60 items-center gap-2 rounded-md border border-default px-2 py-1 text-sm">
        <UIcon name="i-material-symbols-attach-file" class="size-4 shrink-0 text-muted" />
        <span class="min-w-0 flex-1 truncate">{{ one.filename }}</span>
        <span class="shrink-0 tabular-nums text-muted">{{ sizeText(one.size) }}</span>
        <UTooltip :text="$t('mail.action.remove_attachment')">
          <UButton size="sm" color="neutral" variant="ghost" square icon="i-material-symbols-close" :aria-label="$t('mail.action.remove_attachment')" @click="removeAttachment(index)" />
        </UTooltip>
      </li>
      <li v-if="uploading > 0" class="flex items-center gap-2 px-2 py-1 text-sm text-muted">
        <UIcon name="i-material-symbols-progress-activity" class="size-4 animate-spin" />
        {{ $t("mail.status.uploading") }}
      </li>
    </ul>

    <p v-if="props.failure !== ''" role="alert" class="shrink-0 px-3 py-2 text-sm text-error">{{ props.failure }}</p>

    <div class="flex shrink-0 items-center gap-1 border-t border-default px-3 py-2">
      <input ref="picker" type="file" multiple class="hidden" @change="onPick" />
      <UTooltip :text="$t('mail.action.attach_file')">
        <UButton color="neutral" variant="ghost" square icon="i-material-symbols-attach-file" :aria-label="$t('mail.action.attach_file')" @click="picker?.click()" />
      </UTooltip>
      <UTooltip :text="draft.receipt ? $t('mail.action.receipt_off') : $t('mail.action.receipt_on')">
        <UButton
          color="neutral"
          :variant="draft.receipt ? 'soft' : 'ghost'"
          square
          icon="i-material-symbols-mark-email-read-outline"
          :aria-label="draft.receipt ? $t('mail.action.receipt_off') : $t('mail.action.receipt_on')"
          :aria-pressed="draft.receipt"
          @click="draft.receipt = !draft.receipt"
        />
      </UTooltip>
      <UTooltip :text="$t('mail.action.discard_draft')">
        <UButton color="neutral" variant="ghost" square icon="i-material-symbols-delete-outline" :aria-label="$t('mail.action.discard_draft')" @click="emit('discard')" />
      </UTooltip>
      <span v-if="props.status !== ''" class="ms-2 min-w-0 truncate text-sm text-muted">{{ props.status }}</span>
      <UButton class="ms-auto" :label="$t('common.action.send')" icon="i-material-symbols-send-outline" :loading="props.sending" :disabled="uploading > 0" @click="emit('send')" />
    </div>
  </div>
</template>

<script setup lang="ts">
import { mailFailure } from "../domain/mail-refusals";
import { MAIL_FIELD_LABEL, sizeText } from "../domain/mail";
import { AsyncMailEditor } from "../domain/mail-editor-load";

import type { MailApi } from "../domain/mail-api";
import type { Draft, PublicMailAccount } from "../../shared/mail";

const { t } = useI18n();
const props = defineProps<{
  accounts: PublicMailAccount[];
  api: MailApi;
  sending: boolean;
  status: string;
  failure: string;
}>();

const emit = defineEmits<{
  send: [];
  discard: [];
  from: [address: string];
  failed: [reason: string];
}>();

const draft = defineModel<Draft>({ required: true });

const copies = ref(false);
// The cc and bcc rows stand open while they hold anything, whoever opened them: a draft reopened with
// a copy on it shows that copy, and one that has been written into is never folded away under it.
const hasCopies = computed(() => draft.value.cc.length > 0 || draft.value.bcc.length > 0);
const showCopies = computed(() => copies.value || hasCopies.value);
const picker = useTemplateRef<HTMLInputElement>("picker");
const uploading = ref(0);

// Writing the body folds empty copy fields away again, the way every mail client closes them.
function foldCopies(): void {
  if (!hasCopies.value) {
    copies.value = false;
  }
}

async function onPick(event: Event): Promise<void> {
  const input = event.target as HTMLInputElement;
  const files = [...(input.files ?? [])];

  input.value = "";

  if (files.length === 0) {
    return;
  }

  uploading.value += 1;

  try {
    const { files: staged } = await props.api.upload(
      files,
      draft.value.attachments.map((one) => one.size),
    );

    draft.value = {
      ...draft.value,
      attachments: [...draft.value.attachments, ...staged.map((one) => ({ kind: "upload" as const, id: one.id, filename: one.filename, type: one.type, size: one.size }))],
    };
  } catch (error) {
    emit("failed", mailFailure(t, error, t("mail.error.attach_failed")));
  } finally {
    uploading.value -= 1;
  }
}

function removeAttachment(index: number): void {
  draft.value = { ...draft.value, attachments: draft.value.attachments.filter((_, at) => at !== index) };
}
</script>
