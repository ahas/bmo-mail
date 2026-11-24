<template>
  <!-- The accounts modal's body (docs/mail.md, 「The screen」): two tabs at its head, the owner's own accounts and
       the servers they reach. The kinds the service declares and the servers are read once here and shared by
       both; the server form stands over either, opened from the servers tab's add and edit and from the account
       form's server selects. Both tabs stay drawn while the other stands, so an account half written keeps what
       it holds. -->
  <div class="flex min-h-0 flex-1 flex-col">
    <div class="flex h-9 shrink-0 items-stretch pointer-coarse:h-10" :class="TAB_ROW_LINE">
      <UTabs v-model="tab" :items="tabItems" variant="link" :content="false" :ui="TAB_ROW_UI" class="min-w-0 flex-1" />
      <div v-if="tab === 'servers'" class="flex shrink-0 items-center border-s border-default px-1">
        <CreateButton contained :label="$t('mail.action.server_add')" @click="openForm(null, SERVER_PROTOCOLS)" />
      </div>
    </div>

    <div class="flex min-h-0 flex-1 flex-col overflow-y-auto overscroll-contain">
      <div v-show="tab === 'accounts'">
        <p v-if="kindsFailure !== ''" role="alert" class="px-4 pt-4 text-sm text-error">{{ kindsFailure }}</p>
        <MailAccountList
          v-model:dirty="dirty"
          :accounts="props.accounts"
          :kinds="kinds"
          :servers="servers"
          :new-server="newServer"
          @added="emit('added', $event)"
          @removed="emit('removed')"
        />
      </div>
      <MailServerList v-show="tab === 'servers'" :servers="servers" :failure="serversFailure" @edit="openForm($event, [$event.protocol])" @removed="readServers" />
    </div>

    <MailServerForm v-model:open="formOpen" :server="editing" :protocols="formProtocols" @saved="onSaved" />
  </div>
</template>

<script setup lang="ts">
import { TAB_ROW_LINE, TAB_ROW_UI } from "bmo-ui/app/domain/tabs";

import { mailAccountKinds, mailServers } from "../domain/mail-api";
import { mailFailure } from "../domain/mail-refusals";
import { SERVER_PROTOCOLS } from "../domain/mail-servers";

import type { MailAccount, MailAccountKind, MailServer, ServerDirection, ServerProtocol } from "../../shared/mail";

const props = defineProps<{
  accounts: MailAccount[];
}>();

const emit = defineEmits<{
  // An account was added, by its name.
  added: [name: string];
  // An account was removed.
  removed: [];
}>();

// Whether the account form holds what is not yet added.
const dirty = defineModel<boolean>("dirty", { default: false });

const { t } = useI18n();

const tab = ref<"accounts" | "servers">("accounts");
const tabItems = computed(() => [
  { label: t("mail.label.tab_accounts"), value: "accounts" },
  { label: t("mail.label.tab_servers"), value: "servers" },
]);

const kinds = ref<MailAccountKind[] | null>(null);
const servers = ref<MailServer[] | null>(null);
const kindsFailure = ref("");
const serversFailure = ref("");

const formOpen = ref(false);
const editing = ref<MailServer | null>(null);
const formProtocols = ref<readonly ServerProtocol[]>(SERVER_PROTOCOLS);
// What waits on the server form opened from an account's server select: the server it kept, or null.
let waiting: ((server: MailServer | null) => void) | null = null;

async function readServers(): Promise<void> {
  try {
    servers.value = await mailServers();
    serversFailure.value = "";
  } catch (error) {
    servers.value ??= [];
    serversFailure.value = mailFailure(t, error, t("mail.error.servers_failed"));
  }
}

function openForm(server: MailServer | null, protocols: readonly ServerProtocol[]): void {
  editing.value = server;
  formProtocols.value = protocols;
  formOpen.value = true;
}

// A new server for an account's select: of the protocols a declared kind reaches in that direction.
function newServer(direction: ServerDirection): Promise<MailServer | null> {
  const protocols = SERVER_PROTOCOLS.filter((protocol) => (kinds.value ?? []).some((kind) => kind.servers?.[direction] === protocol));

  waiting?.(null);
  openForm(null, protocols);

  return new Promise((resolve) => (waiting = resolve));
}

async function onSaved(server: MailServer): Promise<void> {
  const answer = waiting;

  waiting = null;
  await readServers();
  answer?.(server);
}

// The form closed without keeping anything answers what waits on it with nothing.
watch(formOpen, (standing) => {
  if (standing) return;

  waiting?.(null);
  waiting = null;
});

onMounted(async () => {
  const reading = readServers();

  try {
    kinds.value = await mailAccountKinds();
  } catch (error) {
    kinds.value = [];
    kindsFailure.value = mailFailure(t, error, t("mail.error.kinds_failed"));
  }

  await reading;
});
</script>
