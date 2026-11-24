<template>
  <!-- The servers the owner reaches (docs/mail.md, 「The screen」), the incoming ones and then the outgoing ones,
       each with its name, its protocol, its host and port, its security, who holds it and how many accounts
       stand on it. The owner's own carry an edit and a remove; a removal asks first on its row, and a refusal
       — an account still standing on it among them — is said under that row. An organisation's and the
       platform's servers are read here and changed on the console's organisation screen. -->
  <div class="flex flex-col gap-4 p-4">
    <p v-if="props.failure !== ''" role="alert" class="text-sm text-error">{{ props.failure }}</p>

    <section v-for="direction in DIRECTIONS" :key="direction" class="flex flex-col gap-2" :aria-labelledby="`mail-servers-${direction}`">
      <h3 :id="`mail-servers-${direction}`" class="text-lg font-semibold">{{ $t(DIRECTION_WORDS[direction]) }}</h3>

      <ContentSkeleton v-if="props.servers === null" :rows="2" :label="$t('mail.status.servers_loading')">
        <div class="flex flex-col gap-2">
          <div v-for="row in 2" :key="row" class="flex flex-col gap-1 rounded-md border border-default px-3 py-2">
            <SkeletonBar class="h-5 w-1/3" />
            <SkeletonBar class="h-5 w-1/2" />
            <SkeletonBar class="h-5 w-1/3" />
          </div>
        </div>
      </ContentSkeleton>

      <template v-else>
        <p v-if="ofDirection(direction).length === 0" class="text-sm text-muted">{{ $t("mail.status.no_servers") }}</p>
        <ul class="flex flex-col gap-2">
          <li v-for="server in ofDirection(direction)" :key="server.id" class="flex flex-col gap-1 rounded-md border border-default px-3 py-2" :data-server="server.id">
            <div class="flex items-center gap-2">
              <div class="flex min-w-0 flex-1 flex-col">
                <span class="truncate text-sm font-medium">{{ server.name }} · {{ protocolWords(t, server.protocol) }}</span>
                <span class="truncate text-sm text-muted">{{ serverAddress(server) }} · {{ $t(SECURITY_WORDS[server.security]) }}</span>
                <span class="truncate text-sm text-muted">{{ holderWords(t, server.holder) }} · {{ $t("mail.label.server_accounts", { n: countText(server.accountCount) }) }}</span>
              </div>
              <template v-if="server.mayChange">
                <template v-if="removing === server.id">
                  <UButton size="sm" color="neutral" variant="ghost" :label="$t('common.action.cancel')" @click="removing = null" />
                  <UButton size="sm" color="error" :label="$t('mail.action.server_remove_confirm')" :loading="busy === server.id" @click="remove(server)" />
                </template>
                <template v-else>
                  <IconButton size="sm" icon="i-material-symbols-edit-outline" :label="$t('mail.action.server_edit')" @click="emit('edit', server)" />
                  <IconButton size="sm" icon="i-material-symbols-delete-outline" :label="$t('mail.action.server_remove')" @click="ask(server)" />
                </template>
              </template>
            </div>
            <p v-if="refused?.id === server.id" role="alert" class="text-sm text-error">{{ refused.words }}</p>
          </li>
        </ul>
      </template>
    </section>
  </div>
</template>

<script setup lang="ts">
import { countText } from "bmo-ui/app/domain/numbers";

import { removeMailServer } from "../domain/mail-api";
import { mailFailure } from "../domain/mail-refusals";
import { DIRECTIONS, DIRECTION_WORDS, SECURITY_WORDS, holderWords, protocolWords, serverAddress } from "../domain/mail-servers";

import type { MailServer, ServerDirection } from "../../shared/mail";

const props = defineProps<{
  // The servers the owner reaches, null while they are read.
  servers: MailServer[] | null;
  // Why they could not be read, or nothing.
  failure: string;
}>();

const emit = defineEmits<{
  // An own server to change in the server form.
  edit: [server: MailServer];
  // An own server was removed.
  removed: [];
}>();

const { t } = useI18n();

const removing = ref<number | null>(null);
const busy = ref<number | null>(null);
const refused = ref<{ id: number; words: string } | null>(null);

function ofDirection(direction: ServerDirection): MailServer[] {
  return (props.servers ?? []).filter((server) => server.direction === direction);
}

function ask(server: MailServer): void {
  refused.value = null;
  removing.value = server.id;
}

async function remove(server: MailServer): Promise<void> {
  busy.value = server.id;

  try {
    await removeMailServer(server.id);
    emit("removed");
  } catch (error) {
    refused.value = { id: server.id, words: mailFailure(t, error, t("mail.error.server_remove_failed")) };
  } finally {
    busy.value = null;
    removing.value = null;
  }
}
</script>
