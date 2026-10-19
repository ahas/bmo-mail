<template>
  <!-- The sender beside their name (docs/mail.md, 「Sender avatars」): the logo their domain
       publishes, and where it publishes none the console's identity mark for that address — its
       first letter in a colour of its own. The console looks the logo up and fetches it, so this
       asks the console and never the sender's own server; a domain that answers with nothing falls
       back to the mark. -->
  <span
    aria-hidden="true"
    class="flex shrink-0 items-center justify-center overflow-hidden rounded-full leading-none font-semibold select-none"
    :class="[props.size === 'card' ? 'size-9 text-base' : 'size-6 text-sm', logo === null ? mark.class : 'bg-white']"
  >
    <img v-if="logo !== null" :src="logo" alt="" class="size-full object-contain" @error="failed = true">
    <template v-else>{{ mark.text }}</template>
  </span>
</template>

<script setup lang="ts">
import { addressMark, senderDomain } from "../domain/mail";
import { avatarUrl } from "../domain/mail-api";

const props = defineProps<{
  // The sender's address, which is both what the logo is looked up by and what the mark is drawn from.
  address: string;
  // The sender as the row names them, whose first letter stands where there is no logo.
  name: string;
  // Beside a row of a list, or at the head of a message card.
  size: "row" | "card";
}>();

const failed = ref(false);

watch(
  () => props.address,
  () => {
    failed.value = false;
  },
);

const mark = computed(() => addressMark(props.address, props.name));

const logo = computed(() => {
  const domain = failed.value ? null : senderDomain(props.address);

  return domain === null ? null : avatarUrl(domain);
});
</script>
