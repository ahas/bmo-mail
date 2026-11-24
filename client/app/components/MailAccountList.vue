<template>
  <!-- One's own mail accounts (docs/mail.md, 「The screen」): listed with their address and their incoming and
       outgoing servers by name, each removed from its row after a second press, and one added. An account
       reached through mail servers is added from its address, a name that may be left empty, an incoming and an
       outgoing server chosen among those the owner reaches — their organisation's first, each group under its
       holder, then the platform's, then their own — and the fields its kind declares. The kind is the one whose
       incoming protocol is the chosen incoming server's, so no protocol is ever picked. Each server select ends
       with a new server, which opens the server form and chooses the server it keeps. A kind that signs in by
       authorization, where the service declares one, is chosen apart and offers the button that opens its
       provider's page. The screen names a kind and its fields by the pack's words under their names. -->
  <div class="flex flex-col">
    <!-- The tab and the modal already name the list, so it stands with no heading of its own. -->
    <section class="flex flex-col gap-3 border-b border-default p-4" :aria-label="$t('mail.title.accounts')">
      <p v-if="props.accounts.length === 0" class="text-sm text-muted">{{ $t("mail.status.no_accounts") }}</p>
      <ul class="flex flex-col gap-2">
        <li v-for="account in props.accounts" :key="account.name" class="flex items-center gap-2 rounded-md border border-default px-3 py-2">
          <div class="flex min-w-0 flex-1 flex-col">
            <span class="truncate text-sm font-medium">{{ account.address }}</span>
            <span class="truncate text-sm text-muted">{{ serversLine(account) }}</span>
          </div>
          <template v-if="removing === account.name">
            <UButton size="sm" color="neutral" variant="ghost" :label="$t('common.action.cancel')" @click="removing = null" />
            <UButton size="sm" color="error" :label="$t('mail.action.account_remove_confirm')" :loading="busy === account.name" @click="remove(account)" />
          </template>
          <IconButton v-else size="sm" icon="i-material-symbols-delete-outline" :label="$t('mail.action.account_remove')" @click="removing = account.name" />
        </li>
      </ul>
      <p v-if="removing !== null" class="text-sm text-muted" role="status">{{ $t("mail.message.account_remove_warning") }}</p>
    </section>

    <section class="flex flex-col gap-3 p-4" aria-labelledby="mail-accounts-add">
      <h3 id="mail-accounts-add" class="text-lg font-semibold">{{ $t("mail.action.account_add") }}</h3>
      <ContentSkeleton v-if="props.kinds === null || props.servers === null" :rows="4" :label="$t('mail.status.kinds_loading')">
        <div class="flex flex-col gap-3">
          <SkeletonBar v-for="row in 4" :key="row" class="h-8 w-full" />
        </div>
      </ContentSkeleton>

      <form v-else class="flex flex-col gap-3" @submit.prevent="add">
        <UFormField v-if="authorizationKinds.length > 0" :label="$t('mail.label.account_kind')">
          <ChoiceSelect v-model="way" :items="wayItems" value-key="value" class="w-full" />
        </UFormField>

        <template v-if="authorizationKind === null">
          <UFormField :label="$t('mail.label.account_address')" required>
            <UInput v-model="address" type="email" autocomplete="off" class="w-full" />
          </UFormField>
          <UFormField :label="$t('mail.label.account_name')" :hint="$t('mail.label.optional')">
            <UInput v-model="name" autocomplete="off" :placeholder="localPart" class="w-full" />
          </UFormField>
          <UFormField v-for="direction in DIRECTIONS" :key="direction" :label="$t(DIRECTION_WORDS[direction])" required>
            <ChoiceSelect
              :model-value="chosen[direction]"
              :items="serverItems(direction)"
              value-key="value"
              class="w-full"
              :aria-label="$t(DIRECTION_WORDS[direction])"
              @update:model-value="(value: string) => choose(direction, value)"
            />
          </UFormField>
          <UFormField v-for="field in fields" :key="field.name" :label="fieldName(field.name)" :hint="field.required ? undefined : $t('mail.label.optional')" :required="field.required">
            <UInput
              v-model="values[field.name]"
              :type="field.kind === 'secret' ? 'password' : field.kind === 'port' ? 'number' : 'text'"
              :autocomplete="field.kind === 'secret' ? 'new-password' : 'off'"
              class="w-full"
            />
          </UFormField>
          <p v-if="failure !== ''" role="alert" class="text-sm text-error">{{ failure }}</p>
          <div class="flex justify-end">
            <UButton type="submit" :label="$t('mail.action.account_add')" :loading="busy === 'add'" :disabled="!complete" />
          </div>
        </template>

        <template v-else>
          <UFormField :label="$t('mail.label.account_name')" :hint="$t('mail.label.optional')">
            <UInput v-model="name" autocomplete="off" class="w-full" />
          </UFormField>
          <p v-if="failure !== ''" role="alert" class="text-sm text-error">{{ failure }}</p>
          <div class="flex justify-end">
            <UButton type="submit" :label="$t('mail.action.account_sign_in', { kind: kindName(authorizationKind.name) })" :loading="busy === 'add'" />
          </div>
        </template>
      </form>
    </section>
  </div>
</template>

<script setup lang="ts">
import { useOpenedWith } from "bmo-ui/app/domain/form-save";

import { addMailAccount, removeMailAccount } from "../domain/mail-api";
import { beginAuthorization } from "../domain/mail-authorization";
import { mailFailure } from "../domain/mail-refusals";
import { DIRECTIONS, DIRECTION_WORDS, holderGroups, holderWords, serverAddress } from "../domain/mail-servers";

import type { SelectMenuItem } from "@nuxt/ui";
import type { MailAccount, MailAccountKind, MailServer, ServerDirection } from "../../shared/mail";

// The value of each server select's last entry, which opens the server form rather than choosing.
const NEW_SERVER = "new";

const props = defineProps<{
  accounts: MailAccount[];
  // The kinds the service declares and the servers the owner reaches, null while they are read.
  kinds: MailAccountKind[] | null;
  servers: MailServer[] | null;
  // Opens the server form for a new server of a direction; answers the server it kept, or null.
  newServer: (direction: ServerDirection) => Promise<MailServer | null>;
}>();

const emit = defineEmits<{
  // An account was added, by its name.
  added: [name: string];
  // An account was removed.
  removed: [];
}>();

// Whether the form holds what is not yet added.
const dirty = defineModel<boolean>("dirty", { default: false });

const { t, te } = useI18n();

// The way an account is added: through mail servers, or the name of a kind that signs in by authorization.
const way = ref("");
const address = ref("");
const name = ref("");
const chosen = ref<Record<ServerDirection, string>>({ incoming: "", outgoing: "" });
const values = ref<Record<string, string>>({});
const removing = ref<string | null>(null);
const busy = ref<string | null>(null);
const failure = ref("");

const serverKinds = computed(() => (props.kinds ?? []).filter((kind) => kind.signIn === "password" && kind.servers));
const authorizationKinds = computed(() => (props.kinds ?? []).filter((kind) => kind.signIn === "authorization"));
const authorizationKind = computed(() => authorizationKinds.value.find((kind) => kind.name === way.value) ?? null);
const wayItems = computed(() => [{ label: t("mail.label.account_by_servers"), value: "" }, ...authorizationKinds.value.map((kind) => ({ label: kindName(kind.name), value: kind.name }))]);

function chosenServer(direction: ServerDirection): MailServer | undefined {
  return props.servers?.find((server) => String(server.id) === chosen.value[direction]);
}

// The kind an account on the chosen incoming server is of: the one whose incoming protocol is the server's.
const kind = computed(() => serverKinds.value.find((one) => one.servers?.incoming === chosenServer("incoming")?.protocol) ?? null);
// The fields the kind declares, or, before a server is chosen, the first server kind's.
const fields = computed(() => (kind.value ?? serverKinds.value[0])?.fields ?? []);
const localPart = computed(() => address.value.split("@")[0] ?? "");
const complete = computed(() => address.value.trim() !== "" && kind.value !== null && chosenServer("outgoing") !== undefined && fields.value.every((field) => !field.required || String(values.value[field.name] ?? "").trim() !== ""));

// The servers a select offers: those of the direction whose protocol a declared kind reaches.
function offered(direction: ServerDirection): MailServer[] {
  const protocols = new Set(serverKinds.value.map((one) => one.servers?.[direction]));

  return (props.servers ?? []).filter((server) => server.direction === direction && protocols.has(server.protocol));
}

function serverItems(direction: ServerDirection): SelectMenuItem[][] {
  const groups = holderGroups(offered(direction)).map((group) => [
    { type: "label" as const, label: holderWords(t, group.holder) },
    ...group.servers.map((server) => ({ label: server.name, description: serverAddress(server), value: String(server.id) })),
  ]);

  return [...groups, [{ label: t("mail.label.server_new"), icon: "i-material-symbols-add", value: NEW_SERVER }]];
}

// A server chosen in a select. Its last entry chooses nothing itself: it opens the server form, and the server
// kept there is chosen.
async function choose(direction: ServerDirection, value: string): Promise<void> {
  if (value !== NEW_SERVER) {
    chosen.value[direction] = value;
    return;
  }

  const kept = await props.newServer(direction);

  if (kept !== null) chosen.value[direction] = String(kept.id);
}

// Each select starts on the first server it offers, so an organisation's own server is chosen for its people.
watch(
  () => [props.servers, props.kinds] as const,
  () => {
    for (const direction of DIRECTIONS) {
      if (chosen.value[direction] === "") chosen.value[direction] = String(offered(direction)[0]?.id ?? "");
    }
  },
  { immediate: true },
);

// The fields start at their defaults, and start over when the kind's fields change.
watch(
  fields,
  (declared) => {
    values.value = Object.fromEntries(declared.map((field) => [field.name, values.value[field.name] ?? field.default ?? ""]));
  },
  { immediate: true },
);

const openedWith = useOpenedWith(() => ({ way: way.value, address: address.value, name: name.value, chosen: chosen.value, values: values.value }), {
  ready: () => props.kinds !== null && props.servers !== null,
});

watch(openedWith.dirty, (holds) => (dirty.value = holds), { immediate: true });

// A kind's or a field's words where the pack holds them, else the name the service declares.
function kindName(kindName: string): string {
  const key = `mail.account_kind.${kindName}`;

  return te(key) ? t(key) : kindName;
}

function fieldName(fieldName: string): string {
  const key = `mail.account_field.${fieldName}`;

  return te(key) ? t(key) : fieldName;
}

// The account's servers by name, where the owner still reaches them; its kind where neither is known.
function serversLine(account: MailAccount): string {
  const named = [account.incomingServer, account.outgoingServer]
    .map((id) => props.servers?.find((server) => server.id === id)?.name)
    .filter((one): one is string => one !== undefined);

  return named.length === 0 ? kindName(account.kind) : named.join(" · ");
}

async function run(what: string, work: () => Promise<void>, fallback: string): Promise<void> {
  busy.value = what;
  failure.value = "";

  try {
    await work();
  } catch (error) {
    failure.value = mailFailure(t, error, fallback);
  } finally {
    busy.value = null;
  }
}

function add(): Promise<void> {
  return run(
    "add",
    async () => {
      if (authorizationKind.value !== null) {
        await beginAuthorization(authorizationKind.value.name, name.value.trim());
        return;
      }

      const adding = kind.value;
      const incoming = chosenServer("incoming");
      const outgoing = chosenServer("outgoing");

      if (adding === null || incoming === undefined || outgoing === undefined) return;

      const settings: Record<string, string> = {};
      const secrets: Record<string, string> = {};

      for (const field of adding.fields) {
        const value = String(values.value[field.name] ?? "").trim();

        if (value === "") continue;
        if (field.kind === "secret") secrets[field.name] = value;
        else settings[field.name] = value;
      }

      const added = await addMailAccount({
        kind: adding.name,
        address: address.value.trim(),
        ...(name.value.trim() === "" ? {} : { name: name.value.trim() }),
        incomingServer: incoming.id,
        outgoingServer: outgoing.id,
        settings,
        secrets,
      });

      address.value = "";
      name.value = "";
      values.value = Object.fromEntries(adding.fields.map((field) => [field.name, field.default ?? ""]));
      await openedWith.take();
      emit("added", added.name);
    },
    t("mail.error.account_add_failed"),
  );
}

function remove(account: MailAccount): Promise<void> {
  return run(
    account.name,
    async () => {
      await removeMailAccount(account.name);
      removing.value = null;
      emit("removed");
    },
    t("mail.error.account_remove_failed"),
  );
}
</script>
