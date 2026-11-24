<template>
  <!-- The server form (docs/mail.md, 「The screen」): a mail server of the owner's own, added or changed. It asks
       the protocol, fixed once the server is kept, the name, the host, the port with the protocol's usual one
       for the security chosen as its placeholder, the security, the certificate name where the security is not
       none, and the sign-in where the protocol is SMTP. The connection test at its foot tests the server as the
       form holds it, keeping nothing, and says beside it what it came to; the one save keeps it. A small form,
       so a centred modal at every width, over the accounts modal or drawer it is opened from; while it holds
       changes every way of closing it asks first. -->
  <UModal
    :open="open"
    :title="props.server === null ? $t('mail.title.server_add') : $t('mail.title.server_edit')"
    :ui="{ footer: 'flex items-center gap-2' }"
    @update:open="onOpen"
  >
    <template #close="{ ui }">
      <ShellClose :class="ui.close()" />
    </template>

    <template #body>
      <form :id="FORM_ID" class="flex flex-col gap-4" @submit.prevent="save">
        <UFormField :label="$t('mail.label.server_protocol')">
          <ChoiceSelect v-model="protocol" :items="protocolItems" value-key="value" :disabled="props.server !== null || protocolItems.length < 2" class="w-full" />
        </UFormField>
        <UFormField :label="$t('mail.label.server_name')" :hint="$t('mail.label.optional')">
          <UInput v-model="name" autocomplete="off" :placeholder="host.trim()" class="w-full" />
        </UFormField>
        <UFormField :label="$t('mail.label.server_host')" required>
          <UInput v-model="host" autocomplete="off" autocapitalize="off" spellcheck="false" class="w-full" />
        </UFormField>
        <UFormField :label="$t('mail.label.server_port')">
          <UInput v-model="port" inputmode="numeric" maxlength="5" autocomplete="off" :placeholder="defaultWords(String(usual))" class="w-full" />
        </UFormField>
        <UFormField :label="$t('mail.label.server_security')">
          <ChoiceSelect v-model="security" :items="securityItems" value-key="value" class="w-full" />
        </UFormField>
        <UFormField v-if="security !== 'none'" :label="$t('mail.label.server_tls_name')" :hint="$t('mail.label.optional')">
          <UInput v-model="tlsName" autocomplete="off" autocapitalize="off" spellcheck="false" :placeholder="host.trim()" class="w-full" />
        </UFormField>
        <UFormField v-if="directionOf(protocol) === 'outgoing'" :label="$t('mail.label.server_auth')">
          <ChoiceSelect v-model="auth" :items="authItems" value-key="value" class="w-full" />
        </UFormField>
        <p v-if="failure !== ''" role="alert" class="text-sm text-error">{{ failure }}</p>
      </form>
    </template>

    <template #footer>
      <UButton color="neutral" variant="outline" :label="$t('mail.action.server_test')" :loading="testing" :disabled="host.trim() === ''" class="shrink-0" @click="test" />
      <span class="min-w-0 flex-1 truncate text-sm" :class="tested === null ? 'text-muted' : tested.reached && tested.signedIn !== false ? 'text-success' : 'text-error'" role="status">
        {{ tested === null ? "" : testWords(t, tested) }}
      </span>
      <UButton type="submit" :form="FORM_ID" :label="$t('common.action.save')" :loading="saving" :disabled="!dirty || host.trim() === ''" class="shrink-0" />
    </template>
  </UModal>
</template>

<script setup lang="ts">
import { useCloseGuard, useOpenedWith } from "bmo-ui/app/domain/form-save";

import { addMailServer, changeMailServer, testMailServer } from "../domain/mail-api";
import { mailFailure } from "../domain/mail-refusals";
import { AUTH_DEFAULT, AUTH_WORDS, SECURITY_WORDS, defaultSecurity, directionOf, protocolWords, securitiesOf, testWords, usualPort } from "../domain/mail-servers";

import type { MailServer, MailServerTest, ServerAuth, ServerProtocol, ServerSecurity } from "../../shared/mail";

const FORM_ID = "mail-server-form";

const props = defineProps<{
  // The server changed, or null for a new one.
  server: MailServer | null;
  // The protocols a new server may speak, the first chosen as the form opens.
  protocols: readonly ServerProtocol[];
}>();

const emit = defineEmits<{
  // The server as the service kept it.
  saved: [server: MailServer];
}>();

const open = defineModel<boolean>("open", { required: true });

const { t } = useI18n();

const protocol = ref<ServerProtocol>("imap");
const name = ref("");
const host = ref("");
const port = ref("");
const security = ref<ServerSecurity>("tls");
const tlsName = ref("");
const auth = ref<ServerAuth>(AUTH_DEFAULT);
const tested = ref<MailServerTest | null>(null);
const testing = ref(false);
const saving = ref(false);
const failure = ref("");

const usual = computed(() => usualPort(protocol.value, security.value));
const values = () => ({ protocol: protocol.value, name: name.value, host: host.value, port: port.value, security: security.value, tlsName: tlsName.value, auth: auth.value });
const openedWith = useOpenedWith(values, { open });
const dirty = openedWith.dirty;
const guard = useCloseGuard(open, openedWith, t);

// A value's words, marked where it is the one the service keeps when none is given.
function defaultWords(value: string): string {
  return t("mail.label.default_value", { value });
}

function choiceWords(key: string, isDefault: boolean): string {
  return isDefault ? defaultWords(t(key)) : t(key);
}

const protocolItems = computed(() => (props.server === null ? props.protocols : [props.server.protocol]).map((one) => ({ label: protocolWords(t, one), value: one })));
const securityItems = computed(() => securitiesOf(protocol.value).map((one) => ({ label: choiceWords(SECURITY_WORDS[one], one === defaultSecurity(protocol.value)), value: one })));
const authItems = computed(() => (Object.keys(AUTH_WORDS) as ServerAuth[]).map((one) => ({ label: choiceWords(AUTH_WORDS[one], one === AUTH_DEFAULT), value: one })));

// The form opening is filled from the server it changes, or from a new server's defaults.
watch(
  open,
  (opening) => {
    if (!opening) return;

    const server = props.server;

    protocol.value = server?.protocol ?? props.protocols[0] ?? "imap";
    name.value = server?.name ?? "";
    host.value = server?.host ?? "";
    port.value = server === null ? "" : String(server.port);
    security.value = server?.security ?? defaultSecurity(protocol.value);
    tlsName.value = server?.tlsName ?? "";
    auth.value = server?.auth ?? AUTH_DEFAULT;
    tested.value = null;
    failure.value = "";
  },
  { immediate: true, flush: "sync" },
);

// A protocol chosen that does not take the security chosen goes to its own default.
watch(protocol, (chosen) => {
  if (!securitiesOf(chosen).includes(security.value)) security.value = defaultSecurity(chosen);
});

// A port is digits alone.
watch(port, (typed) => {
  const digits = typed.replace(/\D/g, "");

  if (digits !== typed) port.value = digits;
});

// What a test said stands for the fields it tested, and goes once any of them changes.
watch(values, () => (tested.value = null));

function onOpen(standing: boolean): void {
  if (standing) open.value = true;
  else void guard.leave();
}

function chosenPort(): number {
  return port.value === "" ? usual.value : Number(port.value);
}

async function test(): Promise<void> {
  testing.value = true;
  failure.value = "";

  try {
    tested.value = await testMailServer({
      protocol: protocol.value,
      host: host.value.trim(),
      port: chosenPort(),
      security: security.value,
      ...(security.value === "none" || tlsName.value.trim() === "" ? {} : { tlsName: tlsName.value.trim() }),
    });
  } catch (error) {
    failure.value = mailFailure(t, error, t("mail.error.server_test_failed"));
  } finally {
    testing.value = false;
  }
}

async function save(): Promise<void> {
  if (!dirty.value || host.value.trim() === "") return;

  saving.value = true;
  failure.value = "";

  const fields = {
    name: name.value.trim() === "" ? host.value.trim() : name.value.trim(),
    host: host.value.trim(),
    port: chosenPort(),
    security: security.value,
    tlsName: security.value === "none" ? "" : tlsName.value.trim(),
    ...(directionOf(protocol.value) === "outgoing" ? { auth: auth.value } : {}),
  };

  try {
    const kept =
      props.server === null
        ? await addMailServer({ protocol: protocol.value, ...fields, tlsName: fields.tlsName === "" ? null : fields.tlsName })
        : await changeMailServer(props.server.id, fields);

    open.value = false;
    emit("saved", kept);
  } catch (error) {
    failure.value = mailFailure(t, error, t("mail.error.server_save_failed"));
  } finally {
    saving.value = false;
  }
}
</script>
