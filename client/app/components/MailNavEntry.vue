<template>
  <!-- One row of the mail nav (docs/mail.md, 「The screen」): its mark, its name, and a dot where it
       holds unread mail. Folded, the name is gone and only the mark stands, so the dot moves onto
       the mark's own top corner instead of keeping a cell at the row's end — a dot in a cell of its
       own pushes the mark off centre, and the rows with one would then stand a few pixels aside
       from the rows without.

       A row that is one thing among many of its kind — an account among the accounts — wears its
       own avatar in place of the icon, because the icon is the kind's and every row of that list
       would stand as the same mark once the names are gone. Such a row's name is an address rather
       than a word, longer than the panel is wide and read from its front, so it is cut at the width
       it has and named on hover at every width rather than set moving: a moving address shows a
       false one at every moment but two. -->
  <UTooltip :text="props.name" :disabled="!props.folded && !props.nameOnHover">
    <NuxtLink
      :to="props.to"
      class="flex h-9 items-center gap-2 px-3 text-sm transition-colors hover:bg-elevated active:bg-accented"
      :class="[props.current ? 'bg-elevated font-semibold text-highlighted' : 'text-default', props.folded ? 'justify-center' : '']"
      :aria-current="props.current ? props.currentAs : undefined"
      :aria-label="props.name"
      @click="emit('navigated')"
    >
      <span class="relative flex shrink-0">
        <UAvatar v-if="props.avatar !== null" v-bind="props.avatar" size="2xs" class="size-5 text-sm" />
        <UIcon v-else :name="props.icon" class="size-5 shrink-0" :style="props.iconColour === null ? undefined : { color: props.iconColour }" />
        <UIcon
          v-if="props.unread && props.folded"
          name="i-material-symbols-circle"
          class="absolute -end-1 -top-1 size-2 rounded-full text-primary outline-2 outline-default"
          :aria-label="$t('screens.aria.mail_unread')"
        />
      </span>

      <span v-if="!props.folded" class="min-w-0 flex-1 truncate" :class="{ 'font-semibold': props.unread }">{{ props.name }}</span>

      <UIcon v-if="props.unread && !props.folded" name="i-material-symbols-circle" class="size-2 shrink-0 text-primary" :aria-label="$t('screens.aria.mail_unread')" />
    </NuxtLink>
  </UTooltip>
</template>

<script setup lang="ts">
import type { AvatarProps } from "@nuxt/ui";

const props = withDefaults(
  defineProps<{
    to: string;
    name: string;
    icon: string;
    // The row's own avatar, where the row is one identity among others that share the icon. The
    // icon stands for every row that has none.
    avatar?: AvatarProps | null;
    // A label's own colour, which its mark wears; every other mark takes the row's text colour.
    iconColour?: string | null;
    unread?: boolean;
    folded: boolean;
    current: boolean;
    // What `aria-current` says of the row standing: an account is the true one, a view is the page.
    currentAs?: "true" | "page";
    // Whether the row names itself on hover at every width, and not only folded, which a row whose
    // name is too long for the panel asks for.
    nameOnHover?: boolean;
  }>(),
  { avatar: null, iconColour: null, unread: false, currentAs: "page", nameOnHover: false }
);

const emit = defineEmits<{ navigated: [] }>();
</script>
