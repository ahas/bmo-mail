<template>
  <!-- One address field of the composer: the addresses as chips, completed from the ones the account
       has used and heard from, and anything typed that reads as an address taken as one. The label
       stands in the composer's own label column; a chip too long for the row is cut inside itself
       rather than carrying the row past its edge, and the box typed into is allowed to shrink with
       the row — a text box keeps a width of its own by default, which on a phone is wider than the
       whole row and pushes what stands beside it off the screen. -->
  <div class="flex min-h-10 items-start gap-2 border-b border-default px-3 py-1">
    <span :class="[MAIL_FIELD_LABEL, 'flex min-h-8 items-center']">{{ props.label }}</span>
    <UInputMenu
      v-model="chosen"
      v-model:search-term="typed"
      multiple
      :items="suggestions"
      create-item="always"
      ignore-filter
      :aria-label="props.label"
      variant="none"
      class="min-w-0 flex-1"
      :ui="{ base: 'px-0 flex-wrap', tagsItem: 'min-w-0 max-w-full', tagsItemText: 'truncate min-w-0', tagsInput: 'min-w-0' }"
      @create="add"
      @keydown="onKey"
      @blur="addTyped"
    />
    <div class="flex min-h-8 shrink-0 items-center">
      <slot />
    </div>
  </div>
</template>

<script setup lang="ts">
import { addressText, MAIL_FIELD_LABEL, readAddresses } from "../domain/mail";

import type { MailApi } from "../domain/mail-api";
import type { MailMailbox } from "../../shared/mail";

const props = defineProps<{ label: string; api: MailApi }>();
const model = defineModel<MailMailbox[]>({ required: true });

const typed = ref("");
const suggestions = ref<string[]>([]);

const chosen = computed<string[]>({
  get: () => model.value.map(addressText),
  set: (texts) => {
    model.value = readAddresses(texts.join(","));
  },
});

function add(text: string): void {
  const read = readAddresses(text);

  if (read.length > 0) {
    model.value = [...model.value, ...read.filter((one) => !model.value.some((held) => held.address === one.address))];
  }

  typed.value = "";
}

// A comma or a semicolon ends one address, as in every mail client.
function onKey(event: KeyboardEvent): void {
  if (event.key === "," || event.key === ";") {
    event.preventDefault();
    addTyped();
  }
}

function addTyped(): void {
  if (typed.value.trim() !== "") {
    add(typed.value);
  }
}

let timer: ReturnType<typeof setTimeout> | null = null;

// Completion is asked a moment after typing stops, not on every key.
watch(typed, (text) => {
  if (timer !== null) {
    clearTimeout(timer);
  }

  if (text.trim() === "") {
    suggestions.value = [];

    return;
  }

  timer = setTimeout(async () => {
    try {
      suggestions.value = (await props.api.addresses(text.trim())).map(addressText);
    } catch {
      suggestions.value = [];
    }
  }, 200);
});

onBeforeUnmount(() => {
  if (timer !== null) {
    clearTimeout(timer);
  }
});
</script>
