<template>
  <!-- The composer (docs/mail.md, 「Writing」 and 「The screen」). On a desktop it is a panel docked at
       the bottom right with minimise, maximise and close buttons; maximise makes it a centred large
       modal. On a phone it is a full-height drawer. The draft is saved a few seconds after the writing
       stops and again when it closes; the Send button hands it to the server's hold, which the page
       offers to take back. -->
  <UDrawer v-if="props.narrow" v-model:open="open" :ui="{ content: 'h-[100dvh]', container: 'flex min-h-0 flex-1 flex-col p-0 gap-0 overflow-hidden', header: 'w-full', body: 'flex min-h-0 flex-1 flex-col' }" handle-only :content="HELD_OPEN">
    <template #header>
      <div class="flex w-full items-center gap-2 border-b border-default px-4 py-3">
        <h2 class="min-w-0 flex-1 truncate text-lg font-semibold">{{ title }}</h2>
        <UTooltip :text="$t('common.action.close')">
          <UButton color="neutral" variant="ghost" square icon="i-material-symbols-close" :aria-label="$t('common.action.close')" @click="close" />
        </UTooltip>
      </div>
    </template>
    <template #body>
      <MailComposeForm v-model="draft" v-bind="formProps" @send="send" @discard="discard" @from="switchFrom" @failed="failure = $event" />
    </template>
  </UDrawer>

  <UModal v-else-if="maximized" v-model:open="open" :content="HELD_OPEN" :ui="{ content: 'sm:max-w-5xl h-[85dvh] flex flex-col', body: 'flex min-h-0 flex-1 flex-col p-0 sm:p-0' }">
    <template #header>
      <div class="flex w-full items-center gap-2">
        <h2 class="min-w-0 flex-1 truncate text-lg font-semibold">{{ title }}</h2>
        <UTooltip :text="$t('mail.action.minimize')">
          <UButton color="neutral" variant="ghost" square icon="i-material-symbols-minimize" :aria-label="$t('mail.action.minimize')" @click="minimize" />
        </UTooltip>
        <UTooltip :text="$t('mail.action.shrink')">
          <UButton color="neutral" variant="ghost" square icon="i-material-symbols-close-fullscreen" :aria-label="$t('mail.action.shrink')" @click="maximized = false" />
        </UTooltip>
        <UTooltip :text="$t('common.action.close')">
          <UButton color="neutral" variant="ghost" square icon="i-material-symbols-close" :aria-label="$t('common.action.close')" @click="close" />
        </UTooltip>
      </div>
    </template>
    <template #body>
      <MailComposeForm v-model="draft" v-bind="formProps" @send="send" @discard="discard" @from="switchFrom" @failed="failure = $event" />
    </template>
  </UModal>

  <!-- The docked panel stands on the page body itself, clear of the layout that sizes the page's
       own parts. -->
  <Teleport v-else-if="open" to="body">
  <section
    class="fixed right-4 bottom-0 z-40 flex w-[560px] max-w-[calc(100vw-2rem)] flex-col overflow-hidden rounded-t-lg border border-b-0 border-default bg-default shadow-xl"
    :class="minimized ? '' : 'h-[min(640px,85dvh)]'"
    :aria-label="$t('mail.action.compose')"
  >
    <div class="flex h-10 shrink-0 items-center gap-1 border-b border-default bg-elevated px-3">
      <button type="button" class="min-w-0 flex-1 truncate text-start text-sm font-semibold" @click="minimized = !minimized">{{ title }}</button>
      <UTooltip :text="minimized ? $t('common.action.expand') : $t('mail.action.minimize')">
        <UButton color="neutral" variant="ghost" size="sm" square :icon="minimized ? 'i-material-symbols-expand-less' : 'i-material-symbols-minimize'" :aria-label="minimized ? $t('common.action.expand') : $t('mail.action.minimize')" @click="minimized = !minimized" />
      </UTooltip>
      <UTooltip :text="$t('mail.action.maximize')">
        <UButton color="neutral" variant="ghost" size="sm" square icon="i-material-symbols-open-in-full" :aria-label="$t('mail.action.maximize')" @click="maximize" />
      </UTooltip>
      <UTooltip :text="$t('common.action.close')">
        <UButton color="neutral" variant="ghost" size="sm" square icon="i-material-symbols-close" :aria-label="$t('common.action.close')" @click="close" />
      </UTooltip>
    </div>
    <MailComposeForm v-show="!minimized" v-model="draft" v-bind="formProps" @send="send" @discard="discard" @from="switchFrom" @failed="failure = $event" />
  </section>
  </Teleport>
</template>

<script setup lang="ts">
import { useBackClose } from "bmo-ui/app/domain/layers";
import { mailFailure } from "../domain/mail-refusals";
import { mailApi } from "../domain/mail-api";

import type { Draft, MailRef, PublicMailAccount } from "../../shared/mail";
import type { Ref } from "vue";

const { t } = useI18n();
const props = defineProps<{ accounts: PublicMailAccount[]; narrow: boolean }>();

const emit = defineEmits<{
  // The message went to the server's hold; the page offers to take it back.
  sent: [held: { account: string; id: string; draft: MailRef; holdMs: number }];
  // A draft was saved or thrown away, which moves the Drafts folder.
  saved: [account: string];
}>();

// How long the writing rests before the draft is saved.
const SAVE_AFTER_MS = 3_000;

// A half-written message is not thrown away by a press beside it, so the drawer and the modal
// refuse a press outside themselves and the drawer is dragged shut by its grabber alone. What
// they do not refuse is Esc, which is how the back gesture closes whatever stands on top of the
// screen (docs/web.md, 「The back gesture」) — the draft is saved on its way out either way.
const HELD_OPEN = { onInteractOutside: (event: Event) => event.preventDefault() };

const open = ref(false);
const minimized = ref(false);
const maximized = ref(false);
const draft = ref<Draft | null>(null) as Ref<Draft>;
const sending = ref(false);
const status = ref("");
const failure = ref("");
// What the draft held when it was last saved or opened, so an untouched one is never saved.
let savedText = "";
let timer: ReturnType<typeof setTimeout> | null = null;
let saving: Promise<void> | null = null;
// The editor rewrites the HTML it is handed into its own form as it mounts. That rewrite is not
// writing, so for a moment after opening the draft's form is taken as it settles, not saved.
let settling: ReturnType<typeof setTimeout> | null = null;
const SETTLE_MS = 800;

const account = computed(() => props.accounts.find((one) => one.address === draft.value?.from)?.key ?? props.accounts[0]?.key ?? "");
const api = computed(() => mailApi(account.value));
const title = computed(() => (draft.value?.subject ?? "").trim() || t("mail.title.new_mail"));
const formProps = computed(() => ({ accounts: props.accounts, api: api.value, sending: sending.value, status: status.value, failure: failure.value }));

// The docked panel is drawn by this screen rather than by the component library, so it is not
// among what the back gesture finds on the page by itself and says here that it stands.
const docked = computed({
  get: () => open.value && !props.narrow && !maximized.value,
  set: (value: boolean) => {
    if (!value) void close();
  },
});
useBackClose(docked);

function snapshot(value: Draft): string {
  return JSON.stringify({ ...value, saved: null });
}

async function save(): Promise<void> {
  if (draft.value === null || snapshot(draft.value) === savedText) {
    return;
  }

  if (saving !== null) {
    await saving;

    return save();
  }

  const text = snapshot(draft.value);

  saving = (async () => {
    status.value = t("mail.status.saving");

    try {
      const { saved } = await api.value.saveDraft(draft.value);

      draft.value = { ...draft.value, saved };
      savedText = text;
      status.value = t("mail.status.draft_saved");
      failure.value = "";
      emit("saved", account.value);
    } catch (error) {
      status.value = "";
      failure.value = mailFailure(t, error, t("mail.error.draft_failed"));
    }
  })();

  try {
    await saving;
  } finally {
    saving = null;
  }
}

watch(
  draft,
  () => {
    if (timer !== null) clearTimeout(timer);

    if (settling !== null) {
      savedText = snapshot(draft.value);

      return;
    }

    if (open.value && draft.value !== null && snapshot(draft.value) !== savedText) {
      status.value = "";
      timer = setTimeout(() => void save(), SAVE_AFTER_MS);
    }
  },
  { deep: true },
);

// Opens the composer on a draft; one already open is saved first, as one composer stands at a time.
async function start(next: Draft): Promise<void> {
  if (open.value) {
    await close();
  }

  draft.value = next;
  savedText = snapshot(next);
  status.value = "";
  failure.value = "";
  minimized.value = false;
  maximized.value = false;
  open.value = true;

  if (settling !== null) clearTimeout(settling);

  settling = setTimeout(() => {
    settling = null;
    savedText = snapshot(draft.value);
  }, SETTLE_MS);
}

async function close(): Promise<void> {
  if (timer !== null) clearTimeout(timer);

  await save();
  open.value = false;
}

function minimize(): void {
  maximized.value = false;
  minimized.value = true;
}

function maximize(): void {
  minimized.value = false;
  maximized.value = true;
}

async function discard(): Promise<void> {
  if (timer !== null) clearTimeout(timer);

  const held = draft.value?.saved ?? null;

  savedText = snapshot(draft.value);
  open.value = false;

  if (held !== null) {
    try {
      await api.value.discardDraft(held);
      emit("saved", account.value);
    } catch {
      // A draft that could not be thrown away stays in the Drafts folder, where it can be thrown away again.
    }
  }
}

// Another account to send from: the draft moves to that account's mailbox, the copy saved in the
// first one thrown away.
async function switchFrom(address: string): Promise<void> {
  const held = draft.value.saved;
  const before = account.value;

  draft.value = { ...draft.value, from: address, saved: null };
  savedText = "";

  if (held !== null) {
    await mailApi(before)
      .discardDraft(held)
      .catch(() => undefined);
  }
}

async function send(): Promise<void> {
  if (timer !== null) clearTimeout(timer);

  if (draft.value.to.length + draft.value.cc.length + draft.value.bcc.length === 0) {
    failure.value = t("mail.error.recipient_needed");

    return;
  }

  sending.value = true;
  failure.value = "";

  try {
    if (saving !== null) await saving;

    const held = await api.value.send(draft.value);

    // The server holds the message now; nothing of it is saved again as the composer closes.
    savedText = snapshot(draft.value);
    open.value = false;
    emit("sent", { account: account.value, id: held.id, draft: held.draft, holdMs: held.holdMs });
  } catch (error) {
    failure.value = mailFailure(t, error, t("common.error.not_sent"));
  } finally {
    sending.value = false;
  }
}

// When the modal or the drawer is closed from outside the buttons — Back, Esc — the draft is saved.
watch(open, (value) => {
  if (!value && draft.value !== null) void save();
});

onBeforeUnmount(() => {
  if (timer !== null) clearTimeout(timer);
  if (settling !== null) clearTimeout(settling);
});

defineExpose({ start, isOpen: () => open.value });
</script>
