<template>
  <!-- One address field of the composer (docs/mail.md, 「Writing」): its label in the column the labels
       share, the addresses already in it as chips, each cut inside itself rather than carrying the row
       past its edge and taken out by its own button, and the layer's one select after them, where an
       address is typed and the known addresses are found as it is typed. Typing on the field opens the
       select with what was typed; a comma, a semicolon or Enter ends an address and makes it a chip,
       leaving the field makes what was typed one, pasting several addresses makes a chip of each, and
       Backspace in an empty search takes the last chip back out. The row wraps rather than running off
       a phone's edge, and the select is allowed to shrink with it. -->
  <div class="flex min-h-10 items-start gap-2 border-b border-default px-3 py-1">
    <span :class="[MAIL_FIELD_LABEL, 'flex min-h-8 items-center']">{{ props.label }}</span>
    <div class="flex min-h-8 min-w-0 flex-1 flex-wrap items-center gap-1">
      <UBadge v-for="one in model" :key="one.address" color="neutral" variant="subtle" class="min-w-0 max-w-full gap-0.5 pe-0.5" data-address-chip>
        <span class="min-w-0 truncate">{{ addressText(one) }}</span>
        <UTooltip :text="$t('mail.action.remove_address_named', { address: addressText(one) })">
          <UButton
            color="neutral"
            variant="link"
            size="xs"
            icon="i-material-symbols-close"
            class="shrink-0 p-0"
            :aria-label="$t('mail.action.remove_address_named', { address: addressText(one) })"
            @click="remove(one.address)"
          />
        </UTooltip>
      </UBadge>
      <ChoiceSelect
        :open="open"
        :model-value="chosenTexts"
        :items="chosenItems"
        :find="find"
        value-key="value"
        multiple
        create-item
        variant="none"
        :trailing="false"
        :aria-label="props.label"
        class="min-w-24 flex-1"
        :ui="{ base: 'px-0' }"
        @update:open="onOpen"
        @update:search-term="(words: string) => (typed = words)"
        @update:model-value="choose"
        @create="add"
        @keydown="onControlKey"
      >
        <template #default>
          <span class="block h-4" />
        </template>
        <template #content-top>
          <span ref="typing" hidden />
        </template>
      </ChoiceSelect>
    </div>
    <div class="flex min-h-8 shrink-0 items-center">
      <slot />
    </div>
  </div>
</template>

<script setup lang="ts">
import { addressText, endsAddress, MAIL_FIELD_LABEL, readAddresses, typedAddresses } from "../domain/mail";

import type { MailApi } from "../domain/mail-api";
import type { MailMailbox } from "../../shared/mail";

const props = defineProps<{ label: string; api: MailApi }>();
const model = defineModel<MailMailbox[]>({ required: true });

const open = ref(false);
// The words standing in the select's search, as the select hands them on.
const typed = ref("");
// The keys typed on the closed field, which the search takes as its first words once the list stands open:
// from the first such key until then, every key typed is kept for it.
let pending = "";
let seeding = false;

const chosenTexts = computed(() => model.value.map(addressText));
const chosenItems = computed(() => chosenTexts.value.map((text) => ({ label: text, value: text })));

// The known addresses for the words typed, asked of the service as the select's road.
async function find(words: string): Promise<{ label: string; value: string }[]> {
  return (await props.api.addresses(words.trim())).map((one) => ({ label: addressText(one), value: addressText(one) }));
}

function add(text: string): void {
  const read = readAddresses(text);

  if (read.length > 0) {
    model.value = [...model.value, ...read.filter((one) => !model.value.some((held) => held.address === one.address))];
  }
}

function remove(address: string): void {
  model.value = model.value.filter((one) => one.address !== address);
}

// The select's own choosing: a known address taken from the list, or a chosen one let go there.
function choose(texts: string[]): void {
  model.value = readAddresses(texts.join(","));
}

// Leaving the field — the list closing by Esc, Tab or a press outside it — makes what was typed an
// address, as every mail client does.
function onOpen(standing: boolean): void {
  open.value = standing;

  if (!standing && typed.value.trim() !== "") {
    add(typed.value);
    typed.value = "";
  }
}

// The open list's search field, while it stands.
let searchField: HTMLInputElement | null = null;

// A key typed where the search field is not, carried into it: a separator ends the address typed so far,
// Enter ends it too, Backspace takes back a letter or, with none typed, the last chip, and any other
// letter is typed on.
function typeInto(field: HTMLInputElement, event: KeyboardEvent): void {
  if (endsAddress(event.key) || event.key === "Enter") {
    if (field.value.trim() !== "") add(field.value);
    setWords(field, "");
  } else if (event.key === "Backspace") {
    if (field.value === "") model.value = model.value.slice(0, -1);
    else setWords(field, field.value.slice(0, -1));
  } else {
    setWords(field, `${field.value}${event.key}`);
  }
}

// A key on the field's control. Closed, a letter opens the list with it as the first of the words, and
// Backspace takes the last chip back out. Open, the sheet the field stands in on a touch screen keeps the
// focus on the control, so what is typed there goes on into the search.
function onControlKey(event: KeyboardEvent): void {
  if (event.ctrlKey || event.metaKey || event.altKey || event.isComposing) return;

  const letter = event.key.length === 1 && event.key !== " ";

  if (seeding) {
    if (letter) {
      event.preventDefault();
      pending += event.key;
    }
  } else if (open.value) {
    if (searchField !== null && (event.key.length === 1 || event.key === "Enter" || event.key === "Backspace")) {
      event.preventDefault();
      event.stopPropagation();
      typeInto(searchField, event);
    }
  } else if (event.key === "Backspace" && model.value.length > 0) {
    event.preventDefault();
    model.value = model.value.slice(0, -1);
  } else if (letter) {
    event.preventDefault();
    pending = event.key;
    seeding = true;
    open.value = true;
  }
}

// The search field's words set as typing sets them, so the select hears them through its own field.
function setWords(input: HTMLInputElement, words: string): void {
  input.value = words;
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

// The search field's own keys, heard before the list hears them: a separator ends the address typed,
// Enter ends it where no choice stands highlighted to be taken instead, and Backspace in an empty search
// takes the last chip back out, while Enter with nothing typed does nothing. A letter typed while the list itself holds the focus, as it does when a
// finger opened it, goes on into the search, since typing is what the field is for.
function onSearchKey(event: KeyboardEvent, list: HTMLElement, field: HTMLInputElement): void {
  const target = event.target;

  if (event.isComposing) return;

  if (target !== field) {
    if (event.key.length !== 1 || event.ctrlKey || event.metaKey || event.altKey || target instanceof HTMLInputElement) return;

    event.preventDefault();
    field.focus();
    typeInto(field, event);

    return;
  }

  const ends = endsAddress(event.key) || (event.key === "Enter" && list.querySelector("[data-highlighted]") === null);

  if (ends && field.value.trim() !== "") {
    event.preventDefault();
    event.stopPropagation();
    add(field.value);
    setWords(field, "");
  } else if (endsAddress(event.key)) {
    event.preventDefault();
  } else if (event.key === "Enter" && field.value.trim() === "" && list.querySelector("[data-highlighted]") !== null) {
    // Nothing typed, Enter is no choice: the addresses listed are the chips already standing, and a key
    // that let one go would take an address out unasked.
    event.preventDefault();
    event.stopPropagation();
  } else if (event.key === "Backspace" && field.value === "" && model.value.length > 0) {
    model.value = model.value.slice(0, -1);
  }
}

// Several addresses pasted into the search become a chip each at once; one is pasted as it is.
function onSearchPaste(event: ClipboardEvent): void {
  const input = event.target;
  const pasted = event.clipboardData?.getData("text") ?? "";

  if (!(input instanceof HTMLInputElement) || input.closest("[data-slot=content]") === null) return;

  const words = `${input.value}${pasted}`;

  if (typedAddresses(words)) {
    event.preventDefault();
    add(words);
    setWords(input, "");
  }
}

// The opened list, heard through a mark standing at its top while it is open: the keys and pastes of
// its search field, and the first words a key on the closed field left for it.
const typing = useTemplateRef<HTMLElement>("typing");

watch(typing, (mark, _was, onCleanup) => {
  const list = mark?.parentElement;
  const input = list?.querySelector("input");

  if (list == null || input == null) return;

  const onKey = (event: KeyboardEvent): void => onSearchKey(event, list, input);

  searchField = input;
  list.addEventListener("keydown", onKey, true);
  list.addEventListener("paste", onSearchPaste, true);
  onCleanup(() => {
    searchField = null;
    list.removeEventListener("keydown", onKey, true);
    list.removeEventListener("paste", onSearchPaste, true);
  });

  if (seeding) {
    // What was ended by a separator before the list stood is an address already; the rest is typed on.
    const cut = Math.max(pending.lastIndexOf(","), pending.lastIndexOf(";"));

    if (cut >= 0) add(pending.slice(0, cut));
    input.focus();
    setWords(input, pending.slice(cut + 1));
    pending = "";
    seeding = false;
  }
}, { flush: "post" });
</script>
