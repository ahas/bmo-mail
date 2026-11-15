<template>
  <!-- One account's mail settings (docs/mail.md, 「The screen」 and 「Filters and spam」), one section
       each: the signature, images, read receipts, filters, labels and spam senders. The first three
       are one form, saved by the one Save button at the foot (docs/web.md, 「Forms」); the filters,
       labels and spam senders are records, each saved as it is added, changed, moved or removed
       beside it. The signature is written in the same editor a message's body is, so a picture or a
       table can go into it. -->
  <div class="flex min-h-0 flex-1 flex-col">
    <div class="flex min-h-0 flex-1 flex-col overflow-y-auto overscroll-contain">
      <p v-if="failure !== ''" role="alert" class="px-4 pt-4 text-sm text-error">{{ failure }}</p>
      <ContentSkeleton v-if="settings === null" :rows="6" :label="$t('mail.status.settings_loading')">
        <div class="flex flex-col gap-3 border-b border-default p-4">
          <SkeletonBar class="h-5 w-24" />
          <SkeletonBar class="h-4 w-1/2" />
        </div>
      </ContentSkeleton>

      <template v-else>
        <section class="flex flex-col gap-3 border-b border-default p-4" aria-labelledby="mail-settings-signature">
          <h3 id="mail-settings-signature" class="text-lg font-semibold">{{ $t("mail.title.signature") }}</h3>
          <AsyncMailEditor v-model="signature" :placeholder="$t('mail.placeholder.signature')" compact class="rounded-md border border-default overflow-hidden" @failed="failure = $event" />
        </section>

        <section class="flex flex-col gap-3 border-b border-default p-4" aria-labelledby="mail-settings-images">
          <h3 id="mail-settings-images" class="text-lg font-semibold">{{ $t("mail.title.images") }}</h3>
          <USwitch v-model="showImages" :label="$t('mail.label.show_images_always')" />
        </section>

        <section class="flex flex-col gap-3 border-b border-default p-4" aria-labelledby="mail-settings-receipt">
          <h3 id="mail-settings-receipt" class="text-lg font-semibold">{{ $t("mail.title.receipts") }}</h3>
          <USwitch v-model="receiptDefault" :label="$t('mail.label.receipt_default')" />
        </section>

        <section class="flex flex-col gap-3 border-b border-default p-4" aria-labelledby="mail-settings-rules">
          <h3 id="mail-settings-rules" class="text-lg font-semibold">{{ $t("mail.title.filters") }}</h3>
          <p v-if="rules.length === 0" class="text-sm text-muted">{{ $t("mail.status.no_rules") }}</p>
          <!-- A rule is carried by the pointer to another place in the order, a line marking where it will land; its
               buttons move it one place for a reader without a pointer. -->
          <ul class="flex flex-col gap-2" @dragover="ruleOver" @drop="ruleDropped">
            <li
              v-for="(rule, index) in rules"
              :key="rule.id"
              draggable="true"
              data-rule
              class="flex cursor-grab items-center gap-2 rounded-md border border-default px-3 py-2 active:cursor-grabbing"
              :class="[ruleLand === index ? 'shadow-[0_-3px_0_0_var(--ui-primary)]' : '', ruleLand === rules.length && index === rules.length - 1 ? 'shadow-[0_3px_0_0_var(--ui-primary)]' : '', ruleCarried === index ? 'opacity-50' : '']"
              @dragstart="carryRule($event, index)"
              @dragend="setRuleDown"
            >
              <USwitch :model-value="rule.on" :aria-label="rule.on ? $t('mail.action.rule_off') : $t('mail.action.rule_on')" @update:model-value="(on: boolean) => toggleRule(rule, on)" />
              <span class="min-w-0 flex-1 text-sm break-words">{{ ruleText(rule) }}</span>
              <UTooltip :text="$t('mail.action.move_up')">
                <UButton size="sm" color="neutral" variant="ghost" square icon="i-material-symbols-arrow-upward" :aria-label="$t('mail.action.move_up')" :disabled="index === 0" @click="moveRule(index, -1)" />
              </UTooltip>
              <UTooltip :text="$t('mail.action.move_down')">
                <UButton size="sm" color="neutral" variant="ghost" square icon="i-material-symbols-arrow-downward" :aria-label="$t('mail.action.move_down')" :disabled="index === rules.length - 1" @click="moveRule(index, 1)" />
              </UTooltip>
              <UTooltip :text="$t('mail.action.rule_delete')">
                <UButton size="sm" color="neutral" variant="ghost" square icon="i-material-symbols-delete-outline" :aria-label="$t('mail.action.rule_delete')" @click="removeRule(rule)" />
              </UTooltip>
            </li>
          </ul>

          <div class="flex flex-col gap-2 rounded-md border border-default p-3">
            <div class="flex flex-wrap items-center gap-2">
              <USelect v-model="form.field" :items="FIELD_ITEMS" class="w-32" :aria-label="$t('mail.aria.rule_field')" />
              <UInput v-if="form.field === 'header'" v-model="form.header" :placeholder="$t('mail.placeholder.header_name')" class="w-40" :aria-label="$t('mail.placeholder.header_name')" />
              <UInput v-model="form.contains" :placeholder="$t('mail.placeholder.contains')" class="min-w-40 flex-1" :aria-label="$t('mail.placeholder.contains')" />
            </div>
            <div class="flex flex-wrap items-center gap-2">
              <USelect v-model="form.action" :items="ACTION_ITEMS" class="w-40" :aria-label="$t('mail.aria.rule_action')" />
              <USelect v-if="form.action === 'label'" v-model="form.label" :items="props.labels.map((one) => ({ label: one.name, value: one.id }))" :placeholder="$t('mail.label.labels')" class="w-40" :aria-label="$t('mail.label.labels')" />
              <USelect v-if="form.action === 'move'" v-model="form.folder" :items="props.folders.map((one) => ({ label: one.shortName, value: one.name }))" :placeholder="$t('mail.label.folders')" class="w-40" :aria-label="$t('mail.label.folders')" />
            </div>
            <div class="flex flex-wrap justify-end gap-2">
              <UButton color="neutral" variant="outline" :label="$t('mail.action.spam_header_rule')" :loading="busy === 'presets'" @click="addPresets" />
              <UButton color="neutral" variant="outline" :label="$t('mail.action.apply_inbox')" :loading="busy === 'run'" :disabled="rules.length === 0" @click="runRules" />
              <UButton :label="$t('mail.action.rule_add')" :loading="busy === 'rule'" @click="addRule" />
            </div>
          </div>
          <p v-if="ran !== ''" class="text-sm text-muted" role="status">{{ ran }}</p>
        </section>

        <section class="flex flex-col gap-3 border-b border-default p-4" aria-labelledby="mail-settings-labels">
          <h3 id="mail-settings-labels" class="text-lg font-semibold">{{ $t("mail.label.labels") }}</h3>
          <ul class="flex flex-col gap-2">
            <li v-for="label in props.labels" :key="label.id" class="flex items-center gap-2">
              <UPopover>
                <UTooltip :text="$t('mail.aria.label_color')">
                  <UButton color="neutral" variant="ghost" size="sm" square icon="i-material-symbols-label" :style="{ color: label.colour }" :aria-label="$t('mail.aria.label_color')" />
                </UTooltip>
                <template #content>
                  <div class="flex gap-1 p-2">
                    <UButton v-for="color in LABEL_COLORS" :key="color" size="sm" color="neutral" variant="ghost" square icon="i-material-symbols-circle" :style="{ color }" :aria-label="color" @click="recolor(label, color)" />
                  </div>
                </template>
              </UPopover>
              <UInput :model-value="label.name" class="min-w-0 flex-1" :aria-label="$t('mail.aria.label_name')" @change="(event: Event) => rename(label, (event.target as HTMLInputElement).value)" />
              <UTooltip :text="$t('mail.action.label_delete')">
                <UButton size="sm" color="neutral" variant="ghost" square icon="i-material-symbols-delete-outline" :aria-label="$t('mail.action.label_delete')" :loading="busy === `label:${label.id}`" @click="removeLabel(label)" />
              </UTooltip>
            </li>
          </ul>
          <div class="flex items-center gap-2">
            <UInput v-model="newLabel" :placeholder="$t('mail.placeholder.new_label')" class="min-w-0 flex-1" :aria-label="$t('mail.placeholder.new_label')" @keydown.enter="addLabel" />
            <UButton :label="$t('mail.action.add')" :loading="busy === 'label'" :disabled="newLabel.trim() === ''" @click="addLabel" />
          </div>
        </section>

        <section class="flex flex-col gap-3 p-4" aria-labelledby="mail-settings-spam">
          <h3 id="mail-settings-spam" class="text-lg font-semibold">{{ $t("mail.title.spam_senders") }}</h3>
          <p v-if="settings.spamSenders.length === 0" class="text-sm text-muted">{{ $t("mail.label.none") }}</p>
          <ul class="flex flex-col gap-1">
            <li v-for="address in settings.spamSenders" :key="address" class="flex items-center gap-2 text-sm">
              <span class="min-w-0 flex-1 truncate">{{ address }}</span>
              <UTooltip :text="$t('mail.action.remove')">
                <UButton size="sm" color="neutral" variant="ghost" square icon="i-material-symbols-close" :aria-label="$t('mail.action.remove')" @click="removeSpam(address)" />
              </UTooltip>
            </li>
          </ul>
          <div class="flex items-center gap-2">
            <UInput v-model="newSpam" :placeholder="$t('mail.placeholder.address')" class="min-w-0 flex-1" :aria-label="$t('mail.placeholder.spam_sender')" @keydown.enter="addSpam" />
            <UButton :label="$t('mail.action.add')" :disabled="newSpam.trim() === ''" @click="addSpam" />
          </div>
        </section>
      </template>
    </div>

    <div v-if="settings !== null" class="flex shrink-0 justify-end border-t border-default px-4 py-3">
      <UButton :label="$t('common.action.save')" :loading="busy === 'settings'" :disabled="!dirty || busy === 'settings'" @click="saveSettings" />
    </div>
  </div>
</template>

<script setup lang="ts">
import { mailFailure } from "../domain/mail-refusals";
import { LABEL_COLORS } from "../domain/mail";
import { AsyncMailEditor } from "../domain/mail-editor-load";
import { roleFolder } from "../domain/mail-api";

import { SPAM_HEADER_PRESETS } from "../../shared/mail";

import type { MailApi } from "../domain/mail-api";
import type { MailFolder, MailLabel, MailRule, MailSettings, MailSettingsPatch, RuleAction, RuleField } from "../../shared/mail";

const { t } = useI18n();
const props = defineProps<{ api: MailApi; labels: MailLabel[]; folders: MailFolder[] }>();

// Labels changed here change the mail nav and the lists, and the settings change what an open
// message shows; the page reads each again when it hears so.
const emit = defineEmits<{ labels: []; settings: [] }>();

const FIELD_ITEMS = [
  { label: t("mail.label.field_from"), value: "from" },
  { label: t("mail.label.field_to"), value: "to" },
  { label: t("mail.label.subject"), value: "subject" },
  { label: t("mail.label.field_header"), value: "header" },
];

const ACTION_ITEMS = [
  { label: t("mail.label.do_label"), value: "label" },
  { label: t("mail.label.do_star"), value: "star" },
  { label: t("mail.label.do_read"), value: "read" },
  { label: t("mail.label.do_move"), value: "move" },
  { label: t("mail.label.do_spam"), value: "spam" },
  { label: t("mail.label.do_trash"), value: "trash" },
];

const settings = ref<MailSettings | null>(null);
const rules = ref<MailRule[]>([]);
const signature = ref("");
const showImages = ref(false);
const receiptDefault = ref(false);
const failure = ref("");
const busy = ref<string | null>(null);
const ran = ref("");
const newLabel = ref("");
const newSpam = ref("");
const form = reactive({ field: "from" as RuleField, header: "", contains: "", action: "label" as RuleAction["kind"], label: "", folder: "" });

async function run(name: string, work: () => Promise<void>, fallback: string): Promise<void> {
  busy.value = name;
  failure.value = "";

  try {
    await work();
  } catch (error) {
    failure.value = mailFailure(t, error, fallback);
  } finally {
    busy.value = null;
  }
}

async function load(): Promise<void> {
  await run(
    "load",
    async () => {
      const [read, ruleList] = await Promise.all([props.api.settings(), props.api.rules()]);

      settings.value = read;
      signature.value = read.signature;
      showImages.value = read.showImages;
      receiptDefault.value = read.readReceipts;
      rules.value = ruleList;
    },
    t("mail.error.settings_read_failed"),
  );
}

onMounted(load);
watch(() => props.api, load);

// The form's settings that differ from what is saved, sent together by its one save.
const changed = computed<MailSettingsPatch>(() => {
  const held = settings.value;

  if (held === null) return {};

  return {
    ...(signature.value === held.signature ? {} : { signature: signature.value }),
    ...(showImages.value === held.showImages ? {} : { showImages: showImages.value }),
    ...(receiptDefault.value === held.readReceipts ? {} : { readReceipts: receiptDefault.value }),
  };
});
const dirty = computed(() => Object.keys(changed.value).length > 0);
const dirtyModel = defineModel<boolean>("dirty", { default: false });

watch(dirty, (now) => {
  dirtyModel.value = now;
}, { immediate: true });

function saveSettings(): Promise<void> {
  return run(
    "settings",
    async () => {
      const saved = await props.api.saveSettings(changed.value);

      // What was saved is what the form holds from now on, so it holds no change after a save.
      settings.value = saved;
      signature.value = saved.signature;
      showImages.value = saved.showImages;
      receiptDefault.value = saved.readReceipts;
      emit("settings");
    },
    t("mail.error.save_failed"),
  );
}

const FIELD_NAMES: Record<RuleField, string> = { from: t("mail.label.field_from"), to: t("mail.label.field_to"), subject: t("mail.label.subject"), header: t("mail.label.field_header") };

function actionText(action: RuleAction): string {
  switch (action.kind) {
    case "label":
      return t("mail.label.do_label_named", { name: props.labels.find((one) => one.id === action.labelId)?.name ?? t("mail.label.missing_label") });
    case "star":
      return t("mail.label.do_star");
    case "read":
      return t("mail.label.do_read");
    case "move":
      return t("mail.label.do_move_named", { folder: props.folders.find((one) => one.name === action.folderName)?.shortName ?? action.folderName });
    case "spam":
      return t("mail.label.do_spam");
    case "trash":
      return t("mail.label.do_trash");
  }
}

// A rule as one line: when it applies, then what it does.
function ruleText(rule: MailRule): string {
  const when = rule.match.conditions
    .map((one) => t("mail.label.rule_condition", { field: one.field === "header" ? (one.header ?? "") : FIELD_NAMES[one.field], text: one.contains }))
    .join(` ${rule.match.kind === "all" ? t("mail.label.match_all") : t("mail.label.match_any")} `);

  return `${when} → ${rule.actions.map(actionText).join(", ")}`;
}

function reload(): Promise<void> {
  return props.api.rules().then((read) => {
    rules.value = read;
  });
}

function addRule(): Promise<void> {
  const action: RuleAction =
    form.action === "label" ? { kind: "label", labelId: form.label } : form.action === "move" ? { kind: "move", folderName: form.folder } : ({ kind: form.action } as RuleAction);

  return run(
    "rule",
    async () => {
      await props.api.createRule({ match: { kind: "all", conditions: [{ field: form.field, header: form.field === "header" ? form.header : null, contains: form.contains }] }, actions: [action], on: true });
      form.contains = "";
      await reload();
    },
    t("mail.error.rule_add_failed"),
  );
}

function addPresets(): Promise<void> {
  return run(
    "presets",
    async () => {
      for (const preset of SPAM_HEADER_PRESETS) {
        await props.api.createRule(preset);
      }

      await reload();
    },
    t("mail.error.rule_add_failed"),
  );
}

function toggleRule(rule: MailRule, on: boolean): Promise<void> {
  return run("toggle", async () => {
    await props.api.updateRule(rule.id, { on });
    await reload();
  }, t("mail.error.rule_change_failed"));
}

// A rule moved one place up or down: its position among the others, the rest closing up around it.
function moveRule(index: number, by: number): Promise<void> {
  return placeRule(index, index + by);
}

// A rule put at a place from 0 among the others, the rest closing up around it.
function placeRule(index: number, to: number): Promise<void> {
  const rule = rules.value[index]!;

  return run("order", async () => {
    await props.api.updateRule(rule.id, { position: to + 1 });
    await reload();
  }, t("mail.error.rule_order_failed"));
}

const ruleCarried = ref<number | null>(null);
// Where among the rules the carried one would be put down: before the rule at that index.
const ruleLand = ref<number | null>(null);

function carryRule(event: DragEvent, index: number): void {
  event.dataTransfer?.setData("text/plain", ruleText(rules.value[index]!));
  if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
  ruleCarried.value = index;
}

function setRuleDown(): void {
  ruleCarried.value = null;
  ruleLand.value = null;
}

/** Where the pointer stands among the rules: before the first whose middle it has not passed. */
function ruleOver(event: DragEvent): void {
  if (ruleCarried.value === null) return;

  event.preventDefault();

  const rows = [...(event.currentTarget as HTMLElement).querySelectorAll<HTMLElement>(":scope > [data-rule]")];
  const at = rows.findIndex((row) => {
    const box = row.getBoundingClientRect();

    return event.clientY < box.top + box.height / 2;
  });

  ruleLand.value = at === -1 ? rows.length : at;
}

/** A rule let go where the line stood: one change of its place, none where it lands beside itself. */
function ruleDropped(event: DragEvent): void {
  const from = ruleCarried.value;
  const land = ruleLand.value;

  if (from === null || land === null) return;

  event.preventDefault();
  setRuleDown();

  const to = land > from ? land - 1 : land;

  if (to !== from) void placeRule(from, to);
}

function removeRule(rule: MailRule): Promise<void> {
  return run("remove", async () => {
    await props.api.deleteRule(rule.id);
    await reload();
  }, t("mail.error.rule_delete_failed"));
}

function runRules(): Promise<void> {
  return run(
    "run",
    async () => {
      const result = await props.api.runRules(roleFolder(props.folders, "inbox") ?? "INBOX");

      ran.value = t("mail.message.rules_ran", { n: result.movedCount.toLocaleString("ko-KR") });
      emit("labels");
    },
    t("mail.error.rule_apply_failed"),
  );
}

function addLabel(): Promise<void> {
  const name = newLabel.value.trim();

  return run(
    "label",
    async () => {
      await props.api.createLabel(name, LABEL_COLORS[props.labels.length % LABEL_COLORS.length]!);
      newLabel.value = "";
      emit("labels");
    },
    t("mail.error.label_create_failed"),
  );
}

function rename(label: MailLabel, name: string): Promise<void> {
  return run(`label:${label.id}`, async () => {
    await props.api.updateLabel(label.id, { name });
    emit("labels");
  }, t("mail.error.label_rename_failed"));
}

function recolor(label: MailLabel, colour: string): Promise<void> {
  return run(`label:${label.id}`, async () => {
    await props.api.updateLabel(label.id, { colour });
    emit("labels");
  }, t("mail.error.label_color_failed"));
}

function removeLabel(label: MailLabel): Promise<void> {
  return run(`label:${label.id}`, async () => {
    await props.api.deleteLabel(label.id);
    emit("labels");
  }, t("mail.error.label_delete_failed"));
}

// The always-spam senders stand in the settings as the service reads them, so each change reads them again.
async function reloadSpam(): Promise<void> {
  const read = await props.api.settings();

  settings.value = { ...settings.value!, spamSenders: read.spamSenders };
}

function addSpam(): Promise<void> {
  return run(
    "spam",
    async () => {
      await props.api.addSpamSenders([newSpam.value.trim()]);
      newSpam.value = "";
      await reloadSpam();
    },
    t("mail.error.address_add_failed"),
  );
}

function removeSpam(address: string): Promise<void> {
  return run(
    "spam",
    async () => {
      await props.api.removeSpamSender(address);
      await reloadSpam();
    },
    t("mail.error.address_remove_failed"),
  );
}
</script>
