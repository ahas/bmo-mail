<template>
  <!-- One's own mail accounts (docs/mail.md, 「The screen」): listed with their kind and address, each
       removed from its row after a second press, and one added from a kind the service declares. A kind
       that signs in by password draws its form from the fields it declares; one that signs in by
       authorization offers the button that opens its provider's page. The screen names a kind and its
       fields by the pack's words under their names and knows nothing else of any kind. -->
  <div class="flex min-h-0 flex-1 flex-col overflow-y-auto overscroll-contain">
    <section class="flex flex-col gap-3 border-b border-default p-4" aria-labelledby="mail-accounts-list">
      <h3 id="mail-accounts-list" class="text-lg font-semibold">{{ $t("mail.title.accounts") }}</h3>
      <p v-if="props.accounts.length === 0" class="text-sm text-muted">{{ $t("mail.status.no_accounts") }}</p>
      <ul class="flex flex-col gap-2">
        <li v-for="account in props.accounts" :key="account.name" class="flex items-center gap-2 rounded-md border border-default px-3 py-2">
          <div class="flex min-w-0 flex-1 flex-col">
            <span class="truncate text-sm font-medium" :title="account.address">{{ account.address }}</span>
            <span class="truncate text-xs text-muted">{{ account.name }} · {{ kindName(account.kind) }}</span>
          </div>
          <template v-if="removing === account.name">
            <UButton size="sm" color="neutral" variant="ghost" :label="$t('common.action.cancel')" @click="removing = null" />
            <UButton size="sm" color="error" :label="$t('mail.action.account_remove_confirm')" :loading="busy === account.name" @click="remove(account)" />
          </template>
          <UTooltip v-else :text="$t('mail.action.account_remove')">
            <UButton size="sm" color="neutral" variant="ghost" square icon="i-material-symbols-delete-outline" :aria-label="$t('mail.action.account_remove')" @click="removing = account.name" />
          </UTooltip>
        </li>
      </ul>
      <p v-if="removing !== null" class="text-sm text-muted" role="status">{{ $t("mail.message.account_remove_warning") }}</p>
    </section>

    <section class="flex flex-col gap-3 p-4" aria-labelledby="mail-accounts-add">
      <h3 id="mail-accounts-add" class="text-lg font-semibold">{{ $t("mail.action.account_add") }}</h3>
      <p v-if="failure !== ''" role="alert" class="text-sm text-error">{{ failure }}</p>
      <ContentSkeleton v-if="kinds === null" :rows="4" :label="$t('mail.status.kinds_loading')">
        <div class="flex flex-col gap-2">
          <SkeletonBar v-for="row in 4" :key="row" class="h-8 w-full" />
        </div>
      </ContentSkeleton>

      <form v-else-if="kind !== null" class="flex flex-col gap-3" @submit.prevent="add">
        <UFormField :label="$t('mail.label.account_kind')">
          <USelect v-model="chosenKind" :items="kindItems" class="w-full" />
        </UFormField>

        <template v-if="kind.signIn === 'password'">
          <UFormField :label="$t('mail.label.account_address')" required>
            <UInput v-model="address" type="email" autocomplete="off" class="w-full" />
          </UFormField>
          <UFormField :label="$t('mail.label.account_name')" :hint="$t('mail.label.optional')">
            <UInput v-model="name" autocomplete="off" :placeholder="localPart" class="w-full" />
          </UFormField>
          <UFormField v-for="field in kind.fields" :key="field.name" :label="fieldName(field.name)" :hint="field.required ? undefined : $t('mail.label.optional')" :required="field.required">
            <UInput
              v-model="values[field.name]"
              :type="field.kind === 'secret' ? 'password' : field.kind === 'port' ? 'number' : 'text'"
              :autocomplete="field.kind === 'secret' ? 'new-password' : 'off'"
              class="w-full"
            />
          </UFormField>
          <div class="flex justify-end">
            <UButton type="submit" :label="$t('mail.action.account_add')" :loading="busy === 'add'" />
          </div>
        </template>

        <template v-else>
          <UFormField :label="$t('mail.label.account_name')" :hint="$t('mail.label.optional')">
            <UInput v-model="name" autocomplete="off" class="w-full" />
          </UFormField>
          <div class="flex justify-end">
            <UButton type="submit" :label="$t('mail.action.account_sign_in', { kind: kindName(kind.name) })" :loading="busy === 'add'" />
          </div>
        </template>
      </form>
    </section>
  </div>
</template>

<script setup lang="ts">
import { addMailAccount, mailAccountKinds, removeMailAccount } from "../domain/mail-api";
import { beginAuthorization } from "../domain/mail-authorization";
import { mailFailure } from "../domain/mail-refusals";

import type { MailAccount, MailAccountKind } from "../../shared/mail";

const { t, te } = useI18n();
const props = defineProps<{
  accounts: MailAccount[];
}>();

const emit = defineEmits<{
  // An account was added, by its name.
  added: [name: string];
  // An account was removed.
  removed: [];
}>();

const kinds = ref<MailAccountKind[] | null>(null);
const chosenKind = ref("");
const address = ref("");
const name = ref("");
const values = ref<Record<string, string>>({});
const removing = ref<string | null>(null);
const busy = ref<string | null>(null);
const failure = ref("");

const kind = computed(() => kinds.value?.find((one) => one.name === chosenKind.value) ?? null);
const kindItems = computed(() => (kinds.value ?? []).map((one) => ({ label: kindName(one.name), value: one.name })));
const localPart = computed(() => address.value.split("@")[0] ?? "");

// A kind's or a field's words where the pack holds them, else the name the service declares.
function kindName(name: string): string {
  const key = `mail.account_kind.${name}`;

  return te(key) ? t(key) : name;
}

function fieldName(name: string): string {
  const key = `mail.account_field.${name}`;

  return te(key) ? t(key) : name;
}

// A kind chosen starts its form over, each field at its default.
watch(kind, (chosen) => {
  values.value = Object.fromEntries((chosen?.fields ?? []).map((field) => [field.name, field.default ?? ""]));
});

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
  const chosen = kind.value;

  if (chosen === null) return Promise.resolve();

  return run(
    "add",
    async () => {
      if (chosen.signIn !== "password") {
        await beginAuthorization(chosen.name, name.value.trim());
        return;
      }

      const settings: Record<string, string> = {};
      const secrets: Record<string, string> = {};

      for (const field of chosen.fields) {
        const value = String(values.value[field.name] ?? "").trim();

        if (value === "") continue;
        if (field.kind === "secret") secrets[field.name] = value;
        else settings[field.name] = value;
      }

      const added = await addMailAccount({
        kind: chosen.name,
        address: address.value.trim(),
        ...(name.value.trim() === "" ? {} : { name: name.value.trim() }),
        settings,
        secrets,
      });

      address.value = "";
      name.value = "";
      values.value = Object.fromEntries(chosen.fields.map((field) => [field.name, field.default ?? ""]));
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

onMounted(async () => {
  try {
    kinds.value = await mailAccountKinds();
    chosenKind.value = kinds.value[0]?.name ?? "";
  } catch (error) {
    kinds.value = [];
    failure.value = mailFailure(t, error, t("mail.error.kinds_failed"));
  }
});
</script>
