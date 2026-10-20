<template>
  <!-- The labels to put on or take off the chosen mail: checked where all of it has the label,
       half-checked where only some does. -->
  <ul class="flex max-h-80 min-w-48 flex-col gap-1 overflow-y-auto p-2" :aria-label="$t('mail.label.labels')">
    <li v-for="label in props.labels" :key="label.id" class="flex items-center gap-2 px-1 py-1">
      <UCheckbox
        :model-value="props.all.has(label.id) ? true : props.some.has(label.id) ? 'indeterminate' : false"
        :label="label.name"
        @update:model-value="emit('toggle', label.id, !props.all.has(label.id))"
      />
      <UIcon name="i-material-symbols-label" class="ms-auto size-4 shrink-0" :style="{ color: label.colour }" />
    </li>
  </ul>
</template>

<script setup lang="ts">
import type { MailLabel } from "../../shared/mail";

const props = defineProps<{ labels: MailLabel[]; all: Set<string>; some: Set<string> }>();
const emit = defineEmits<{ toggle: [id: string, on: boolean] }>();
</script>
