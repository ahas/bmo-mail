<template>
  <!-- The labels to put on or take off the chosen mail (docs/mail.md, 「The screen」): the toolbar's
       label select, several at once, checked where all of the chosen mail has the label and half-marked
       where only some does. Choosing a label puts it on all of it, and taking a checked one off takes
       it off all of it. -->
  <MailToolChoice
    icon="i-material-symbols-label-outline"
    :label="$t('mail.label.labels')"
    :items="items"
    value-key="value"
    multiple
    :model-value="[...props.all]"
    :disabled="props.disabled || props.labels.length === 0"
    @update:model-value="toggle"
  >
    <template #item-leading="{ item }">
      <UIcon name="i-material-symbols-label" class="size-5 shrink-0" :style="{ color: colourOf(item.value) }" />
    </template>
    <template #item-trailing="{ item }">
      <UIcon v-if="halfOn(item.value)" name="i-material-symbols-check-indeterminate-small" class="size-5 shrink-0" :aria-label="$t('mail.aria.label_on_some')" />
    </template>
  </MailToolChoice>
</template>

<script setup lang="ts">
import { labelChanges } from "../domain/mail";

import type { MailLabel } from "../../shared/mail";

const props = defineProps<{ labels: MailLabel[]; all: Set<string>; some: Set<string>; disabled?: boolean }>();
const emit = defineEmits<{ toggle: [id: string, on: boolean] }>();

const items = computed(() => props.labels.map((label) => ({ label: label.name, value: label.id })));

function colourOf(id: string): string | undefined {
  return props.labels.find((label) => label.id === id)?.colour;
}

function halfOn(id: string): boolean {
  return props.some.has(id) && !props.all.has(id);
}

function toggle(chosen: string[]): void {
  for (const change of labelChanges(props.all, chosen)) emit("toggle", change.id, change.on);
}
</script>
