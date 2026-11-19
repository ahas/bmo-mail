<template>
  <!-- One address field of the composer (docs/mail.md, 「Writing」): its label in the column the labels
       share, the addresses already in it as chips, each cut inside itself rather than carrying the row
       past its edge and taken out by its own button, and the layer's one select after them, where an
       address is typed and the known addresses are found as it is typed. The select ends an address at
       a comma, a semicolon or Enter, cuts a paste at them and takes what was left typed as the field is
       left; the field adds what the select hands it as addresses. Typing on the closed field opens the
       select with what was typed, Backspace in an empty search takes the last chip back out, and Enter
       with nothing typed lets no chip go. The row wraps rather than running off a phone's edge, and the
       select is allowed to shrink with it. -->
  <div class="flex min-h-10 items-start gap-2 border-b border-default px-3 py-1">
    <span :class="[MAIL_FIELD_LABEL, 'flex min-h-8 items-center']">{{ props.label }}</span>
    <div ref="row" class="flex min-h-8 min-w-0 flex-1 flex-wrap items-center gap-1">
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
        v-model:open="open"
        :model-value="chosenTexts"
        :items="chosenItems"
        :find="find"
        value-key="value"
        multiple
        create-item
        :separators="ADDRESS_SEPARATORS"
        take-on-close
        variant="none"
        :trailing="false"
        :aria-label="props.label"
        class="min-w-24 flex-1"
        :ui="{ base: 'px-0' }"
        @update:model-value="choose"
        @create="add"
        @keydown="onControlKey"
      >
        <template #default>
          <span class="block h-4" />
        </template>
      </ChoiceSelect>
    </div>
    <div class="flex min-h-8 shrink-0 items-center">
      <slot />
    </div>
  </div>
</template>

<script setup lang="ts">
import { ADDRESS_SEPARATORS, addressText, MAIL_FIELD_LABEL, readAddresses } from "../domain/mail";

import type { MailApi } from "../domain/mail-api";
import type { MailMailbox } from "../../shared/mail";

const props = defineProps<{ label: string; api: MailApi }>();
const model = defineModel<MailMailbox[]>({ required: true });

const open = ref(false);
// The keys typed on the closed field, which the search takes as its first words once the list stands open:
// from the first such key until then, every key typed is kept for it.
let pending = "";
let seeding = false;
// The row the chips and the select stand in.
const row = useTemplateRef<HTMLElement>("row");

const chosenTexts = computed(() => model.value.map(addressText));
const chosenItems = computed(() => chosenTexts.value.map((text) => ({ label: text, value: text })));

// The known addresses for the words typed, asked of the service as the select's road.
async function find(words: string): Promise<{ label: string; value: string }[]> {
  return (await props.api.addresses(words.trim())).map((one) => ({ label: addressText(one), value: addressText(one) }));
}

// The addresses as last written. The field's value is the parent's, handed back only as the parent draws
// again, and the select may hand several values on at once, so each is added to what was written before it.
let written: MailMailbox[] | null = null;

function held(): MailMailbox[] {
  return written ?? model.value;
}

function write(next: MailMailbox[]): void {
  written = next;
  model.value = next;
  void nextTick(() => {
    written = null;
  });
}

function add(text: string): void {
  const read = readAddresses(text).filter((one) => !held().some((standing) => standing.address === one.address));

  if (read.length > 0) write([...held(), ...read]);
}

function remove(address: string): void {
  write(held().filter((one) => one.address !== address));
}

function removeLast(): void {
  write(held().slice(0, -1));
}

// The select's own choosing: a known address taken from the list or named by the words ended, or a chosen
// one let go there.
function choose(texts: string[]): void {
  write(readAddresses(texts.join(",")));
}

// The open list, found as the list the select's control names as the one it controls, and its search field.
function openList(): HTMLElement | null {
  const listId = row.value?.querySelector("[aria-controls]")?.getAttribute("aria-controls");

  return listId ? (document.getElementById(listId)?.closest<HTMLElement>("[data-slot=content]") ?? null) : null;
}

function searchField(): HTMLInputElement | null {
  return openList()?.querySelector("input") ?? null;
}

// The search field's words set as typing sets them, so the select hears them through its own field and
// ends an address at a separator they hold.
function setWords(field: HTMLInputElement, words: string): void {
  field.value = words;
  field.dispatchEvent(new Event("input", { bubbles: true }));
}

// A key typed where the search field is not, carried into it: Enter goes to the field as Enter, Backspace
// takes back a letter or, with none typed, the last chip, and any other letter is typed on.
function typeInto(field: HTMLInputElement, event: KeyboardEvent): void {
  if (event.key === "Enter") {
    field.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
  } else if (event.key === "Backspace") {
    if (field.value === "") removeLast();
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
    const field = searchField();

    if (field !== null && (event.key.length === 1 || event.key === "Enter" || event.key === "Backspace")) {
      event.preventDefault();
      event.stopPropagation();
      typeInto(field, event);
    }
  } else if (event.key === "Backspace" && held().length > 0) {
    event.preventDefault();
    removeLast();
  } else if (letter) {
    event.preventDefault();
    pending = event.key;
    seeding = true;
    open.value = true;
  }
}

// The open list's keys, heard before its search field hears them. Enter with nothing typed is no choice:
// the addresses listed are the chips already standing, and a key that let one go would take an address out
// unasked. Backspace in an empty search takes the last chip back out. A letter typed while the list itself
// holds the focus, as it does when a finger opened it, goes on into the search, since typing is what the
// field is for.
function onListKey(event: KeyboardEvent): void {
  const list = event.currentTarget;
  const field = list instanceof HTMLElement ? list.querySelector("input") : null;

  if (field === null || event.isComposing) return;

  if (event.target !== field) {
    if (event.key.length !== 1 || event.ctrlKey || event.metaKey || event.altKey || event.target instanceof HTMLInputElement) return;

    event.preventDefault();
    field.focus();
    typeInto(field, event);
  } else if (event.key === "Enter" && field.value.trim() === "") {
    event.preventDefault();
    event.stopPropagation();
  } else if (event.key === "Backspace" && field.value === "" && held().length > 0) {
    removeLast();
  }
}

// The open list's keys are heard while it stands, and a list opened by a key on the closed field takes
// what was typed until it stood as its first words.
watch(
  open,
  (standing, _was, onCleanup) => {
    const seeded = seeding ? pending : null;
    const list = standing ? openList() : null;
    const field = list?.querySelector("input");

    seeding = false;
    pending = "";
    if (list == null || field == null) return;

    list.addEventListener("keydown", onListKey, true);
    onCleanup(() => list.removeEventListener("keydown", onListKey, true));

    if (seeded !== null) {
      field.focus();
      setWords(field, seeded);
    }
  },
  { flush: "post" },
);
</script>
