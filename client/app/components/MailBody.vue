<template>
  <!-- A message's body (docs/mail.md, 「Reading」). HTML is cleaned in the browser and shown in a
       frame that runs nothing and grows to its content's height, so the thread region is the only
       thing that scrolls. Remote images wait behind a bar until the reader shows them for this
       message, unless the account's settings say to show them as a message opens. Plain text stands
       as text, its lines kept and its addresses made links.

       A message that paints itself — its own background, its own text colour — is shown on a light
       surface of its own, since its colours and the console's theme read together are what leaves
       grey text on white. That surface fills the card it stands in from edge to edge and holds its
       own padding inside, so nothing of it is cut away by the padding around it.

       What no wrapping can bring inside the frame — a layout table whose cells will not narrow is
       the common case — is scaled down until it stands within it, since nothing inside the frame is
       allowed to run and the measuring is therefore the console's to do. -->
  <div class="flex flex-col gap-2">
    <div v-if="held" class="flex flex-wrap items-center gap-2 rounded-md bg-elevated px-3 py-2 text-sm">
      <UIcon name="i-material-symbols-hide-image-outline" class="size-4 shrink-0 text-muted" />
      <span class="min-w-0 flex-1">{{ $t("mail.message.images_hidden") }}</span>
      <UButton size="sm" color="neutral" variant="outline" :label="$t('mail.action.show_images')" @click="allowRemote = true" />
    </div>

    <div v-if="props.message.html != null" :class="painted ? '-mx-4 bg-white px-4 py-3 text-[#111827]' : ''">
      <iframe
        ref="frame"
        :srcdoc="document"
        sandbox="allow-same-origin allow-popups allow-popups-to-escape-sandbox"
        referrerpolicy="no-referrer"
        class="block w-full border-0"
        :style="{ height: `${height}px` }"
        :title="$t('mail.aria.body_of', { subject })"
        @load="measure"
      />
    </div>
    <!-- eslint-disable-next-line vue/no-v-html -- escaped by plainTextHtml, links made by it alone -->
    <div v-else class="text-base break-words whitespace-pre-wrap text-default" v-html="plain" />
  </div>
</template>

<script setup lang="ts">
import { useDarkScheme } from "bmo-ui/app/domain/scheme";

import { cidUrl } from "../domain/mail-api";
import { bodyFitter } from "../domain/mail-fit";
import { frameDocument, paintsItself, plainTextHtml } from "../domain/mail-frame";

import type { FittedFrame } from "../domain/mail-fit";

import type { MailMessage } from "../../shared/mail";

const { t } = useI18n();
const props = defineProps<{
  message: MailMessage;
  account: string;
  // Whether the account shows a message's outside pictures as it opens, from its mail settings.
  showImages: boolean;
}>();

const allowRemote = ref(props.showImages);
const height = ref(120);
const frame = useTemplateRef<HTMLIFrameElement>("frame");
const cleaned = ref<{ html: string; blocked: boolean } | null>(null);
const dark = useDarkScheme();

const subject = computed(() => props.message.subject || t("mail.label.fallback_subject"));
const plain = computed(() => plainTextHtml(props.message.text ?? ""));
const held = computed(() => cleaned.value?.blocked === true && !allowRemote.value);
const painted = computed(() => cleaned.value !== null && paintsItself(cleaned.value.html));

// DOMPurify needs the browser's DOM, so the cleaning module is loaded where the body is shown.
async function clean(): Promise<void> {
  if (props.message.html == null || import.meta.server) {
    return;
  }

  const { cleanMailHtml } = await import("../domain/mail-body");

  cleaned.value = cleanMailHtml(props.message.html, {
    allowRemote: allowRemote.value,
    cidUrl: (cid) => cidUrl(props.account, props.message, cid),
  });
}

const document = ref("");

watch(
  [cleaned, dark],
  () => {
    if (cleaned.value === null) {
      return;
    }

    document.value = frameDocument(cleaned.value.html, {
      allowRemote: allowRemote.value,
      origin: window.location.origin,
      dark: dark.value,
      painted: painted.value,
    });
  },
);

watch([() => props.message, allowRemote], () => void clean());
// A message opens with the account's answer about pictures; asking for them here is this message's
// own and is not undone by the setting being read again.
watch(
  () => props.showImages,
  (on) => {
    if (on) allowRemote.value = true;
  },
);
onMounted(() => void clean());

let observer: ResizeObserver | null = null;
let fit: (() => FittedFrame | null) | null = null;

// The frame is as tall as what it holds and never wider. A picture that loads later grows the body
// and is fitted again; so is a frame the window or the panels beside it have made narrower, which
// the body itself would never report.
function refit(): void {
  const fitted = fit?.() ?? null;

  if (fitted !== null) {
    height.value = fitted.height;
  }
}

function measure(): void {
  const element = frame.value;

  if (element === null || element === undefined || element.contentDocument === null) {
    return;
  }

  fit = bodyFitter(element);
  refit();
  observer?.disconnect();
  observer = new ResizeObserver(refit);
  observer.observe(element.contentDocument.body);
  observer.observe(element);
}

onBeforeUnmount(() => observer?.disconnect());
</script>
