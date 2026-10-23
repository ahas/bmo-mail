<template>
  <!-- The mail nav (docs/mail.md, 「The screen」): the accounts, the compose button, the folder roles
       with the Starred role after the inbox, the labels with their colours, the other folders, and
       the accounts and settings buttons. An entry carries its title and a mark for unread mail,
       never a count. Folded, only the icons stand, each named on hover, with the unread mark on the
       icon's own corner.

       An account stands as its own avatar rather than as the account icon: the accounts are a list
       of identities, and folded they would otherwise be one icon repeated with nothing left to tell
       them apart. The letter is the address's own local part and the colour the whole address's,
       never the domain's logo, on which two accounts of one domain would collide. -->
  <nav class="flex min-h-0 flex-1 flex-col" :aria-label="$t('mail.aria.nav')">
    <div class="flex shrink-0 flex-col gap-2 p-3" :class="{ 'items-center px-2': props.folded }">
      <UTooltip v-if="props.folded" :text="$t('mail.action.compose')">
        <UButton icon="i-material-symbols-edit-outline" square :aria-label="$t('mail.action.compose')" @click="emit('compose')" />
      </UTooltip>
      <UButton v-else icon="i-material-symbols-edit-outline" :label="$t('mail.action.compose')" block @click="emit('compose')" />
    </div>

    <div class="min-h-0 flex-1 overflow-y-auto overscroll-contain pb-3">
      <ul class="flex flex-col" :aria-label="$t('mail.aria.accounts')">
        <li v-for="one in props.accounts" :key="one.name">
          <MailNavEntry
            :to="mailPath({ account: one.name, view: 'inbox', thread: null })"
            :name="one.address"
            icon="i-material-symbols-account-circle-outline"
            :avatar="addressMark(one.address, one.name)"
            :unread="props.unread[one.name] === true"
            :folded="props.folded"
            :current="one.name === props.account"
            current-as="true"
            name-on-hover
            @navigated="emit('navigated')"
          />
        </li>
      </ul>

      <USeparator class="my-2" />

      <ul class="flex flex-col" :aria-label="$t('mail.label.folders')">
        <li v-for="entry in roleEntries" :key="entry.view">
          <MailNavEntry
            :to="mailPath({ account: props.account, view: entry.view, thread: null })"
            :name="entry.label"
            :icon="entry.icon"
            :unread="entry.unread"
            :folded="props.folded"
            :current="entry.view === props.view"
            @navigated="emit('navigated')"
          />
        </li>
      </ul>

      <template v-if="props.labels.length > 0">
        <p v-if="!props.folded" class="px-3 pt-3 pb-1 text-sm text-muted">{{ $t("mail.label.labels") }}</p>
        <USeparator v-else class="my-2" />
        <ul class="flex flex-col" :aria-label="$t('mail.label.labels')">
          <li v-for="label in props.labels" :key="label.id">
            <MailNavEntry
              :to="mailPath({ account: props.account, view: `label:${label.id}`, thread: null })"
              :name="label.name"
              icon="i-material-symbols-label"
              :icon-colour="label.colour"
              :folded="props.folded"
              :current="`label:${label.id}` === props.view"
              @navigated="emit('navigated')"
            />
          </li>
        </ul>
      </template>

      <template v-if="otherFolders.length > 0">
        <p v-if="!props.folded" class="px-3 pt-3 pb-1 text-sm text-muted">{{ $t("mail.label.folders") }}</p>
        <USeparator v-else class="my-2" />
        <ul class="flex flex-col" :aria-label="$t('mail.aria.other_folders')">
          <li v-for="folder in otherFolders" :key="folder.name">
            <MailNavEntry
              :to="mailPath({ account: props.account, view: `folder:${folder.name}`, thread: null })"
              :name="folder.shortName"
              icon="i-material-symbols-folder-outline"
              :unread="folder.unreadCount > 0"
              :folded="props.folded"
              :current="`folder:${folder.name}` === props.view"
              @navigated="emit('navigated')"
            />
          </li>
        </ul>
      </template>
    </div>

    <div class="flex shrink-0 items-center gap-1 border-t border-default p-2" :class="props.folded ? 'flex-col' : 'justify-end'">
      <UTooltip :text="$t('mail.action.accounts')">
        <UButton color="neutral" variant="ghost" square icon="i-material-symbols-manage-accounts-outline" :aria-label="$t('mail.action.accounts')" @click="emit('accounts')" />
      </UTooltip>
      <UTooltip :text="$t('mail.action.settings')">
        <UButton color="neutral" variant="ghost" square icon="i-material-symbols-settings-outline" :aria-label="$t('mail.action.settings')" @click="emit('settings')" />
      </UTooltip>
      <UTooltip v-if="props.foldable" :text="props.folded ? $t('common.action.expand') : $t('common.action.collapse')">
        <UButton
          color="neutral"
          variant="ghost"
          square
          :icon="props.folded ? 'i-material-symbols-left-panel-open-outline' : 'i-material-symbols-left-panel-close-outline'"
          :aria-label="props.folded ? $t('common.action.expand') : $t('common.action.collapse')"
          @click="emit('fold', !props.folded)"
        />
      </UTooltip>
    </div>
  </nav>
</template>

<script setup lang="ts">
import { addressMark, mailPath, NAV_ROLES, ROLE_ICONS, ROLE_LABELS } from "../domain/mail";

import type { MailAccount, MailFolder, MailLabel } from "../../shared/mail";

const { t } = useI18n();
const props = defineProps<{
  accounts: MailAccount[];
  account: string;
  view: string;
  folders: MailFolder[];
  labels: MailLabel[];
  // Which accounts have unread mail in their inbox, by name.
  unread: Record<string, boolean>;
  folded: boolean;
  foldable: boolean;
}>();

const emit = defineEmits<{
  compose: [];
  accounts: [];
  settings: [];
  fold: [folded: boolean];
  // A choice was made, which a nav standing in a drawer answers by closing.
  navigated: [];
}>();

// The roles in order, the Starred role after the inbox. A role's unread mark is its folders' together, the sent
// role being two folders on some servers.
const roleEntries = computed(() => {
  const entries = NAV_ROLES.map((role) => ({
    view: role as string,
    label: t(ROLE_LABELS[role]),
    icon: ROLE_ICONS[role],
    unread: role !== "sent" && role !== "drafts" && role !== "trash" && props.folders.some((folder) => folder.role === role && folder.unreadCount > 0),
  }));

  entries.splice(1, 0, { view: "starred", label: t("mail.label.view_starred"), icon: "i-material-symbols-star-outline", unread: false });

  return entries;
});

const otherFolders = computed(() => props.folders.filter((folder) => folder.role === null));
</script>
