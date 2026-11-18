<template>
  <!-- A value chosen from a toolbar's icon (docs/mail.md, 「The screen」): the layer's one select, whose
       control is the toolbar's small square button showing the icon alone, named by its tooltip, so the
       layer decides whether the opened list searches. Everything else it is handed is the select's. -->
  <UTooltip :text="props.label">
    <ChoiceSelect v-bind="$attrs" :items="props.items" variant="ghost" size="sm" :trailing="false" :aria-label="props.label" :ui="TOOL_UI">
      <template #default>
        <UIcon :name="props.icon" class="size-4 shrink-0" />
      </template>
      <template v-for="name in passed" #[name]="scope" :key="name">
        <slot :name="name" v-bind="scope ?? {}" />
      </template>
    </ChoiceSelect>
  </UTooltip>
</template>

<script setup lang="ts">
import type { ChoiceItems } from "bmo-ui/app/domain/choices";

defineOptions({ inheritAttrs: false });

const props = defineProps<{ icon: string; label: string; items: ChoiceItems }>();
const slots = useSlots();

// The control takes the small square button's own padding, so it stands in the toolbar's row at the size
// of the buttons beside it.
const TOOL_UI = { base: "p-1.5" };

const passed = computed(() => Object.keys(slots).filter((name) => name !== "default"));
</script>
