<template>
  <!-- The mail screen (docs/mail.md, 「The screen」). Where there is room the mail nav, the list and
       the thread stand side by side behind splitters, each scrolling on its own; on a phone the list
       is the body, a thread stands over it, and the mail nav rises from the bottom edge. The
       place — account, view, conversation and search — is the address. -->
  <UDashboardPanel id="mail" :ui="{ body: 'p-0 sm:p-0 gap-0 sm:gap-0 overflow-y-hidden [&>*]:max-w-none' }">
    <template #header>
      <UDashboardNavbar :title="$t('screens.label.mail')" icon="i-material-symbols-mail-outline">
        <template #right>
          <DeployMark />
          <TodoOpenButton />
          <UsageMeters />
        </template>
      </UDashboardNavbar>
    </template>

    <template #body>
      <p v-if="accounts !== null && accounts.length === 0" class="p-6 text-center text-sm text-muted">{{ $t("mail.status.no_accounts") }}</p>

      <!-- The screen's frame stands while the accounts are read, the list keeping its own place
           inside it: what waits for the accounts is the nav's rows and the list's rows, and each
           of them turns into its content once, where it stands. Which frame it is — the splitter or
           the phone's list — is the browser's to say, so the server draws the body empty rather
           than a frame the phone would then trade for its own. -->
      <template v-else-if="painted && (accounts === null || place !== null)">
        <div v-if="narrow" class="relative flex min-h-0 flex-1 flex-col">
          <!-- The list is the body and never leaves it: a thread stands over the list rather than
               in its place, so the list is not taken down and put up again and its scroll, its
               selection and the page it had read are where they were when the thread closes. The
               thread is still a step of its own in the address, so back closes it and a link opens
               it; while it stands the list under it is out of reach. -->
          <MailList
            ref="list"
            v-bind="listProps"
            :narrow="true"
            :choosing="choosing"
            :marking-read="markingRead"
            :inert="place !== null && place.thread !== null"
            @open="openConversation"
            @toggle="toggle"
            @hold="hold"
            @choose="selected = $event"
            @star="starOne"
            @select="selectKind"
            @act="act"
            @move="moveTo"
            @label="labelSelected"
            @refresh="refresh"
            @read-all="readAll"
            @page="goToPage"
            @search="search"
            @sort="chooseSort"
            @unread-only="onlyUnread"
            @nav="navDrawer = true"
            @stop-choosing="stopChoosing"
          />

          <div v-if="place !== null && place.thread !== null" class="absolute inset-0 z-30 flex min-h-0 flex-col bg-default">
            <MailThread
              :messages="threadMessages"
              :labels="labels"
              :account="place.account"
              :address="address"
              :narrow="true"
              :failure="threadFailure"
              :show-images="showImages"
              @back="backToList"
              @write="write"
              @receipt="answerReceipt"
            />
          </div>

          <UButton
            v-if="place?.thread == null && !choosing"
            class="fixed right-4 bottom-[calc(1rem+env(safe-area-inset-bottom))] z-30 shadow-lg"
            size="xl"
            icon="i-material-symbols-edit-outline"
            :label="$t('mail.action.compose')"
            @click="compose"
          />

          <UDrawer v-model:open="navDrawer" :title="$t('mail.label.folders')" :ui="FULL_SHEET">
            <template #body>
              <MailNav
                v-if="accounts !== null && place !== null"
                :accounts="accounts"
                :account="place.account"
                :view="place.view"
                :folders="folders"
                :labels="labels"
                :unread="unreadByAccount"
                :folded="false"
                :foldable="false"
                @compose="navDrawer = false; compose()"
                @settings="navDrawer = false; settingsOpen = true"
                @navigated="navDrawer = false"
              />
            </template>
          </UDrawer>
        </div>

        <USplitter
          v-else
          ref="sides"
          id="mail-splitter"
          auto-save-id="bmo-mail-splitter"
          :items="SIDES"
          :ui="{ handle: SPLIT_HANDLE }"
          class="min-h-0 flex-1"
          @collapse="onFold($event, true)"
          @expand="onFold($event, false)"
        >
          <template #nav>
            <div class="flex min-h-0 min-w-0 flex-1 flex-col">
              <ContentSkeleton v-if="accounts === null || place === null" class="p-3" :rows="8" listClass="flex flex-col" :label="$t('mail.status.accounts_loading')">
                <div class="flex h-8 items-center gap-2 px-2">
                  <SkeletonBar class="size-5 shrink-0 rounded-sm" />
                  <SkeletonBar class="h-4 w-32" />
                </div>
              </ContentSkeleton>
              <MailNav
                v-else
                :accounts="accounts"
                :account="place.account"
                :view="place.view"
                :folders="folders"
                :labels="labels"
                :unread="unreadByAccount"
                :folded="navFolded"
                :foldable="true"
                @compose="compose"
                @settings="settingsOpen = true"
                @fold="foldNav"
              />
            </div>
          </template>

          <template #list>
            <div class="flex min-h-0 min-w-0 flex-1 flex-col">
              <MailList
                ref="list"
                v-bind="listProps"
                :narrow="false"
                :choosing="false"
                :marking-read="markingRead"
                @open="openConversation"
                @toggle="toggle"
                @hold="toggle"
                @choose="selected = $event"
                @star="starOne"
                @select="selectKind"
                @act="act"
                @move="moveTo"
                @label="labelSelected"
                @refresh="refresh"
                @read-all="readAll"
                @page="goToPage"
                @search="search"
                @sort="chooseSort"
                @unread-only="onlyUnread"
              />
            </div>
          </template>

          <template #thread>
            <div class="flex min-h-0 min-w-0 flex-1 flex-col">
              <MailThread
                :messages="place?.thread == null ? [] : threadMessages"
                :labels="labels"
                :account="place?.account ?? ''"
                :address="address"
                :narrow="false"
                :failure="threadFailure"
                :show-images="showImages"
                @back="backToList"
                @write="write"
                @receipt="answerReceipt"
              />
            </div>
          </template>
        </USplitter>

        <MailComposer ref="composer" :accounts="accounts ?? []" :narrow="narrow" @sent="onSent" @saved="onDraftSaved" />

        <!-- The settings hold a form, so while it holds changes every way of closing them asks first
             (docs/web.md, 「Forms」). -->
        <UDrawer v-if="narrow" v-model:open="settingsOpen" :title="$t('mail.action.settings')" :ui="FULL_SHEET" :dismissible="settingsGuard.dismissible.value" @close:prevent="void settingsGuard.leave()">
          <template #body>
            <MailSettingsBody v-model:dirty="settingsDirty" :api="api" :labels="labels" :folders="folders" @labels="refreshAll" @settings="loadSettings" />
          </template>
        </UDrawer>
        <UModal
          v-else
          v-model:open="settingsOpen"
          :title="$t('mail.action.settings')"
          :description="address"
          :dismissible="settingsGuard.dismissible.value"
          :ui="{ content: 'sm:max-w-3xl h-[85dvh] flex flex-col', body: 'flex min-h-0 flex-1 flex-col p-0 sm:p-0' }"
          @close:prevent="void settingsGuard.leave()"
        >
          <template #close="{ ui }">
            <ShellClose :class="ui.close()" @close="void settingsGuard.leave()" />
          </template>
          <template #body>
            <MailSettingsBody v-model:dirty="settingsDirty" :api="api" :labels="labels" :folders="folders" @labels="refreshAll" @settings="loadSettings" />
          </template>
        </UModal>

        <UModal v-model:open="spamAsk" :title="$t('mail.action.report_spam')" :ui="{ footer: 'justify-end' }">
          <template #body>
            <UCheckbox v-model="spamSenders" :label="$t('mail.label.register_spam_senders', { n: countText(spamAddresses.length) })" :disabled="spamAddresses.length === 0" />
          </template>
          <template #footer>
            <UButton color="neutral" variant="ghost" :label="$t('common.action.cancel')" @click="spamAsk = false" />
            <UButton :label="$t('mail.action.report_spam')" :loading="acting" @click="confirmSpam" />
          </template>
        </UModal>
      </template>
    </template>
  </UDashboardPanel>
</template>

<script setup lang="ts">
import { mailFailure } from "../../domain/mail-refusals";
import { useCloseGuard } from "bmo-ui/app/domain/form-save";
import { useIsland } from "bmo-ui/app/domain/island";
import { usePainted } from "bmo-ui/app/domain/loading";
import { useBackClose } from "bmo-ui/app/domain/layers";
import {
  isRole,
  labelKeyword,
  mailPath,
  parseMailPath,
  refsOf,
  ROLE_ICONS,
  ROLE_LABELS,
  selectBy,
  shortcutOf,
  viewRole,
  viewTitle,
} from "../../domain/mail";
import { mailAccounts, mailApi } from "../../domain/mail-api";
import { loadMailEditor } from "../../domain/mail-editor-load";
import { useMailSummary } from "../../domain/mail-summary";
import { countText } from "bmo-ui/app/domain/numbers";
import { useNarrow } from "bmo-ui/app/domain/screen";
import { FULL_SHEET, SPLIT_HANDLE } from "bmo-ui/app/domain/side-parts";

import { sortFirstAscending } from "../../../shared/mail";

import type { MailListAction } from "../../components/MailList.vue";
import type { MoveTarget } from "../../domain/mail";
import type { SelectKind } from "../../domain/mail";
import type { MailConversation, MailFolderState, MailLabel, MailMessage, MailRef, MailSettings, MailSort, MailSortBy, PublicMailAccount } from "../../../shared/mail";
import type { SplitterItem } from "@nuxt/ui";

// The mail screen is one screen for every place inside it: the account, the folder, the search and
// the conversation are all the same three regions reading a different place, and the screen reads
// the address itself and answers it. Named by its own address it would be a different screen for
// every conversation — torn down and built again on each one — and the list a conversation stands
// over would be gone with it.
definePageMeta({ key: () => "mail" });

const SIDES: SplitterItem[] = [
  { slot: "nav", minSize: 12, defaultSize: 16, collapsible: true, collapsedSize: 4 },
  { slot: "list", minSize: 25, defaultSize: 40 },
  { slot: "thread", minSize: 25, defaultSize: 44 },
];

const route = useRoute();
const router = useRouter();
const { t } = useI18n();
const island = useIsland(t);
const narrow = useNarrow();
const painted = usePainted();

const accounts = ref<PublicMailAccount[] | null>(null);
const folders = ref<MailFolderState[]>([]);
const labels = ref<MailLabel[]>([]);
const conversations = ref<MailConversation[] | null>(null);
const total = ref(0);
const listFailure = ref("");
const loading = ref(false);
const threadMessages = ref<MailMessage[] | null>(null);
const threadFailure = ref("");
const selected = ref(new Set<string>());
const focused = ref(-1);
const choosing = ref(false);
const navDrawer = ref(false);
const navFolded = ref(false);
const settingsOpen = ref(false);
const settingsDirty = ref(false);
const settingsGuard = useCloseGuard(settingsOpen, () => settingsDirty.value, t);
const spamAsk = ref(false);
const spamSenders = ref(false);
const acting = ref(false);
const markingRead = ref(false);
const settings = ref<MailSettings | null>(null);

const showImages = computed(() => settings.value?.showImages === true);

// What each conversation's messages were when they were last read. A conversation opened again is
// shown from here at once and read again behind it, so pressing a row that has been read before
// puts its messages up rather than a skeleton. It is this visit's own and is dropped with the page.
const threadsRead = new Map<string, MailMessage[]>();

const list = useTemplateRef<{ focusSearch: () => void }>("list");
const composer = useTemplateRef<{ start: (draft: import("#shared/mail").Draft) => Promise<void> }>("composer");
const sides = useTemplateRef<{ panelsRef: Array<{ collapse: () => void; expand: () => void }> }>("sides");

const segments = computed(() => {
  const path = route.params.path;

  return Array.isArray(path) ? path : typeof path === "string" && path !== "" ? [path] : [];
});
const place = computed(() => parseMailPath(segments.value, accounts.value?.[0]?.key ?? null));
const query = computed(() => (typeof route.query.q === "string" ? route.query.q : ""));
const offset = computed(() => {
  const value = Number(route.query.offset ?? 0);

  return Number.isSafeInteger(value) && value > 0 ? value : 0;
});
// How the list stands, as the address spells it: the column it is ordered by, which way that
// column runs, and whether it holds back what has been read. It is in the address rather than in
// this visit alone, so a link opens the list as it was being read.
const sort = computed<MailSort>(() => ({
  by: route.query.sort === "from" || route.query.sort === "subject" ? route.query.sort : "date",
  ascending: route.query.dir === "asc",
  unread: route.query.unread === "1",
}));
const address = computed(() => accounts.value?.find((one) => one.key === place.value?.account)?.address ?? "");
const api = computed(() => mailApi(place.value?.account ?? ""));
const role = computed(() => (place.value === null ? null : viewRole(place.value.view, folders.value)));
const title = computed(() => (place.value === null ? "" : viewTitle(t, place.value.view, labels.value, folders.value)));

const moveTargets = computed<MoveTarget[]>(() => [
  { key: "inbox", label: t(ROLE_LABELS.inbox), icon: ROLE_ICONS.inbox, to: "folder", folder: folders.value.find((one) => one.role === "inbox")?.path ?? "INBOX" },
  { key: "trash", label: t(ROLE_LABELS.trash), icon: ROLE_ICONS.trash, to: "trash" },
  ...folders.value.filter((one) => one.role === null).map((one) => ({ key: `folder:${one.path}`, label: one.name, icon: "i-material-symbols-folder-outline", to: "folder" as const, folder: one.path })),
]);

const listProps = computed(() => ({
  conversations: conversations.value,
  total: total.value,
  offset: offset.value,
  labels: labels.value,
  moveTargets: moveTargets.value,
  view: place.value?.view ?? "inbox",
  title: title.value,
  role: role.value,
  address: address.value,
  query: query.value,
  sort: sort.value,
  selected: selected.value,
  focused: focused.value,
  loading: loading.value,
  failure: listFailure.value,
}));

// Every account's inbox at a glance, read on mount, on the stream's word and on every open. A change
// in the open account's inbox reads the list and the folders again.
const { summary } = useMailSummary((next, previous) => {
  const open = place.value?.account;
  const now = next.accounts.find((one) => one.account === open);
  const before = previous?.accounts.find((one) => one.account === open);

  if (previous !== null && now !== undefined && (now.unread !== before?.unread || now.newest?.uid !== before?.newest?.uid)) {
    void refreshAll();
  }
});

const unreadByAccount = computed<Record<string, boolean>>(() => Object.fromEntries((summary.value?.accounts ?? []).map((one) => [one.account, one.unread > 0])));

async function loadAccounts(): Promise<void> {
  try {
    const answer = await mailAccounts();

    accounts.value = answer.configured ? answer.accounts : [];
  } catch (error) {
    accounts.value = [];
    listFailure.value = mailFailure(t, error, t("mail.error.accounts_failed"));
  }
}

// The account's own settings, for what a message shows as it opens. They are read with the folders
// and again whenever the settings screen writes them.
async function loadSettings(): Promise<void> {
  if (place.value === null) return;

  try {
    settings.value = await api.value.settings();
  } catch {
    // A setting that could not be read leaves the message showing what it shows by default.
  }
}

async function loadFolders(): Promise<void> {
  if (place.value === null) return;

  try {
    const read = await api.value.folders();

    folders.value = read.folders;
    labels.value = read.labels;
  } catch (error) {
    listFailure.value = mailFailure(t, error, t("mail.error.folders_failed"));
  }
}

let listAsked = 0;

async function loadList(): Promise<void> {
  if (place.value === null) return;

  const asked = ++listAsked;

  loading.value = true;

  try {
    const page = await api.value.list(place.value.view, offset.value, sort.value, place.value.view === "search" ? query.value : undefined);

    if (asked !== listAsked) return;

    conversations.value = page.conversations;
    total.value = page.total;
    listFailure.value = "";

    const keys = new Set(page.conversations.map((one) => one.key));

    selected.value = new Set([...selected.value].filter((key) => keys.has(key)));
  } catch (error) {
    if (asked === listAsked) listFailure.value = mailFailure(t, error, t("mail.error.list_failed"));
  } finally {
    if (asked === listAsked) loading.value = false;
  }
}

function threadKey(place: { account: string; thread: string | null }): string {
  return `${place.account}\u0000${place.thread ?? ""}`;
}

async function loadThread(): Promise<void> {
  const open = place.value;

  threadFailure.value = "";

  if (open === null || open.thread === null) {
    threadMessages.value = null;

    return;
  }

  // A conversation already read stands straight away and is read again underneath; only one never
  // read here puts up the skeleton.
  threadMessages.value = threadsRead.get(threadKey(open)) ?? null;

  try {
    const from = conversations.value?.find((one) => one.key === open.thread)?.messages[0]?.folder ?? (isRole(open.view) ? folders.value.find((one) => one.role === open.view)?.path : undefined) ?? "INBOX";
    const { messages } = await api.value.thread(open.thread, from);

    if (place.value?.thread !== open.thread) return;

    threadMessages.value = messages;
    threadsRead.set(threadKey(open), messages);

    // Opening a thread reads it.
    const unread: MailRef[] = messages.filter((one) => !one.seen).map(({ folder, uid }) => ({ folder, uid }));

    if (unread.length > 0) {
      await api.value.flags(unread, ["\\Seen"], []);

      const hit = conversations.value?.find((one) => one.key === open.thread);

      if (hit !== undefined) hit.unread = false;

      void loadFolders();
    }
  } catch (error) {
    threadFailure.value = mailFailure(t, error, t("mail.error.message_failed"));
  }
}

// Mark-all-read over the whole view. What is chosen is read by the toolbar's own mark-read; this is
// for the folder, the label or the search standing open, whatever page of it the list has read.
async function readAll(): Promise<void> {
  if (place.value === null || markingRead.value) return;

  markingRead.value = true;

  try {
    const { marked } = await api.value.readAll(place.value.view, place.value.view === "search" ? query.value : undefined);

    island.say({ title: marked === 0 ? t("mail.status.no_unread") : t("mail.message.marked_read", { n: countText(marked) }), tone: marked === 0 ? "info" : "success" });
    await refreshAll();
  } catch (error) {
    island.fail(error, t("mail.error.mark_read_failed"));
  } finally {
    markingRead.value = false;
  }
}

function refresh(): Promise<void> {
  return loadList();
}

async function refreshAll(): Promise<void> {
  await Promise.all([loadFolders(), loadList()]);
}

// The editor is loaded apart from this screen, and fetched once the screen stands and the browser
// is idle, so the first composer opened finds it already here rather than on its skeleton.
onNuxtReady(() => {
  void loadMailEditor().catch(() => undefined);
});

onMounted(async () => {
  await loadAccounts();

  if (place.value !== null && segments.value.length === 0) {
    await router.replace(mailPath(place.value));
  }
});

// What each of the three reads is asked for by name rather than as a list of parts. A watch whose
// source hands back a fresh array is a watch on nothing: the array is a new one every time any
// part of it is read again, so it never compares equal and the read runs on every change of the
// address — opening a conversation would empty the list and read it back, which is the one thing
// the list standing under the conversation is for.
const folderPlace = computed(() => JSON.stringify([place.value?.account, accounts.value !== null]));
const listPlace = computed(() => JSON.stringify([place.value?.account, place.value?.view, offset.value, query.value, sort.value]));
const threadPlace = computed(() => JSON.stringify([place.value?.account, place.value?.thread]));

watch(folderPlace, () => {
  folders.value = [];
  labels.value = [];
  settings.value = null;
  void loadFolders();
  void loadSettings();
});

watch(
  listPlace,
  () => {
    conversations.value = null;
    selected.value = new Set();
    focused.value = -1;
    choosing.value = false;
    void loadList();
  },
  { immediate: true },
);

watch(threadPlace, () => void loadThread(), { immediate: true });

function go(change: Partial<{ view: string; thread: string | null }>, extra: Record<string, string> = {}): void {
  if (place.value === null) return;

  const next = { ...place.value, ...change };
  const target = mailPath(next);
  // An empty value in `extra` says to drop that word rather than to write it empty, which is how a
  // list is put back to the column, the direction or the page it stands on by default.
  const params = new URLSearchParams(Object.entries(extra).filter(([, value]) => value !== ""));

  if (next.view === "search" && query.value !== "" && extra.q === undefined) params.set("q", query.value);
  if (offset.value > 0 && change.view === undefined && extra.offset === undefined) params.set("offset", String(offset.value));

  // How the list stands goes wherever the screen goes, folder and thread alike: it is a way of
  // reading rather than a place, and one chosen in the inbox holds in the next folder opened.
  if (sort.value.by !== "date" && extra.sort === undefined) params.set("sort", sort.value.by);
  if (sort.value.ascending && extra.dir === undefined) params.set("dir", "asc");
  if (sort.value.unread && extra.unread === undefined) params.set("unread", "1");

  const search = params.toString();

  void router.push(search === "" ? target : `${target}?${search}`);
}

async function openConversation(conversation: MailConversation): Promise<void> {
  // A draft opens in the composer, where it is written on; everything else opens as a thread.
  if (place.value?.view === "drafts" && conversation.messages[0] !== undefined) {
    try {
      const { draft } = await api.value.openDraft(conversation.messages[0]);

      await composer.value?.start(draft);
    } catch (error) {
      island.fail(error, t("mail.error.draft_open_failed"));
    }

    return;
  }

  focused.value = (conversations.value ?? []).findIndex((one) => one.key === conversation.key);
  go({ thread: conversation.key });
}

function backToList(): void {
  if (window.history.state?.back !== undefined && window.history.state.back !== null) {
    router.back();
  } else {
    go({ thread: null });
  }
}

function goToPage(next: number): void {
  go({ thread: null }, { ...(query.value !== "" ? { q: query.value } : {}), offset: String(next) });
}

function search(text: string): void {
  if (text === "") {
    go({ view: "inbox", thread: null });
  } else {
    go({ view: "search", thread: null }, { q: text });
  }
}

// A column pressed again turns round; another column is taken up the way that column opens — a
// date at its newest, a name and a subject at their first. The page starts over, since the row
// that was fiftieth in one order is not the fiftieth in another.
function chooseSort(by: MailSortBy): void {
  const ascending = sort.value.by === by ? !sort.value.ascending : sortFirstAscending(by);

  go({}, { sort: by === "date" ? "" : by, dir: ascending ? "asc" : "", offset: "" });
}

function onlyUnread(on: boolean): void {
  go({}, { unread: on ? "1" : "", offset: "" });
}

function toggle(key: string): void {
  const next = new Set(selected.value);

  if (next.has(key)) next.delete(key);
  else next.add(key);

  selected.value = next;

  if (narrow.value && next.size === 0) choosing.value = false;
}

// A finger held on a row starts choosing, with that row chosen; one held while the list is already
// choosing adds it to what is chosen, which is where a drag from it carries on.
function hold(key: string): void {
  selected.value = choosing.value ? new Set([...selected.value, key]) : new Set([key]);
  choosing.value = true;
}

function stopChoosing(): void {
  choosing.value = false;
  selected.value = new Set();
}

// Choosing is a state the screen enters, so Back leaves it.
useBackClose(choosing, stopChoosing);

function selectKind(kind: SelectKind): void {
  selected.value = selectBy(kind, conversations.value ?? []);
}

function chosenRefs(): MailRef[] {
  return refsOf(selected.value, conversations.value ?? []);
}

// A change of place empties the selection, since what was chosen has left the list; a change of
// state or of labels keeps it, so the next action can go to the same messages.
async function perform(work: () => Promise<unknown>, fallback: string, done?: string, keep = false): Promise<void> {
  acting.value = true;

  try {
    await work();

    if (done !== undefined) island.say({ title: done, tone: "success" });

    if (!keep) {
      selected.value = new Set();
      choosing.value = false;
    }

    await refreshAll();
  } catch (error) {
    island.fail(error, fallback);
  } finally {
    acting.value = false;
  }
}

// Whether the conversation open beside the list is among what an action moves away.
function leavesOpenThread(): boolean {
  return place.value?.thread !== null && place.value?.thread !== undefined && selected.value.has(place.value.thread);
}

function act(action: MailListAction): void {
  const refs = chosenRefs();

  if (refs.length === 0) return;

  const leaving = leavesOpenThread();
  const after = () => {
    if (leaving) go({ thread: null });
  };

  switch (action) {
    case "read":
      void perform(() => api.value.flags(refs, ["\\Seen"], []), t("mail.error.mark_read_failed"), undefined, true);
      break;
    case "unread":
      void perform(() => api.value.flags(refs, [], ["\\Seen"]), t("mail.error.mark_unread_failed"), undefined, true);
      break;
    case "star":
      void perform(() => api.value.flags(refs, ["\\Flagged"], []), t("mail.error.star_failed"), undefined, true);
      break;
    case "unstar":
      void perform(() => api.value.flags(refs, [], ["\\Flagged"]), t("mail.error.unstar_failed"), undefined, true);
      break;
    case "trash":
      void perform(() => api.value.move(refs, "trash"), t("mail.error.trash_failed"), t("mail.message.trashed")).then(after);
      break;
    case "delete":
      void perform(() => api.value.deleteForever(refs), t("mail.error.delete_failed"), t("mail.message.deleted")).then(after);
      break;
    case "spam":
      spamSenders.value = false;
      spamAsk.value = true;
      break;
    case "not-spam":
      void perform(() => api.value.move(refs, "not-spam"), t("mail.error.not_spam_failed"), t("mail.message.not_spam")).then(after);
      break;
  }
}

// The senders of the chosen conversations, the account itself never among them.
const spamAddresses = computed(() => {
  const chosen = (conversations.value ?? []).filter((one) => selected.value.has(one.key));

  return [...new Set(chosen.flatMap((one) => one.from.map((person) => person.address)))].filter((one) => one !== address.value);
});

async function confirmSpam(): Promise<void> {
  const refs = chosenRefs();
  const senders = spamSenders.value ? spamAddresses.value : [];
  const leaving = leavesOpenThread();

  await perform(
    async () => {
      await api.value.move(refs, "spam");

      if (senders.length > 0) await api.value.addSpamSenders(senders);

      // Closed before the selection empties, so the dialog never reads a count of none on its way out.
      spamAsk.value = false;
    },
    t("mail.error.spam_failed"),
    t("mail.message.reported_spam"),
  );

  if (leaving) go({ thread: null });
}

function moveTo(target: MoveTarget): void {
  const refs = chosenRefs();
  const leaving = leavesOpenThread();

  void perform(() => api.value.move(refs, target.to, target.folder), t("mail.error.move_failed"), t("mail.message.moved_to", { name: target.label })).then(() => {
    if (leaving) go({ thread: null });
  });
}

function labelSelected(id: string, on: boolean): void {
  const refs = chosenRefs();

  void perform(() => api.value.flags(refs, on ? [labelKeyword(id)] : [], on ? [] : [labelKeyword(id)]), t("mail.error.label_failed"), undefined, true);
}

function starOne(conversation: MailConversation): void {
  const refs = conversation.messages;

  void perform(() => api.value.flags(refs, conversation.flagged ? [] : ["\\Flagged"], conversation.flagged ? ["\\Flagged"] : []), t("mail.error.star_change_failed"), undefined, true);
}

async function compose(): Promise<void> {
  try {
    const { draft } = await api.value.startDraft("new");

    await composer.value?.start(draft);
  } catch (error) {
    island.fail(error, t("mail.error.compose_failed"));
  }
}

async function write(kind: "reply" | "reply-all" | "forward", message: MailMessage): Promise<void> {
  try {
    const { draft } = await api.value.startDraft(kind, { folder: message.folder, uid: message.uid });

    await composer.value?.start(draft);
  } catch (error) {
    island.fail(error, t("mail.error.compose_failed"));
  }
}

async function answerReceipt(message: MailMessage, send: boolean, done: () => void): Promise<void> {
  try {
    await api.value.answerReceipt({ folder: message.folder, uid: message.uid }, send);

    if (send) island.say({ title: t("mail.message.receipt_sent"), tone: "success" });

    await loadThread();
  } catch (error) {
    island.fail(error, t("mail.error.receipt_failed"));
  } finally {
    done();
  }
}

// The undo-send button stands for as long as the server holds the message; taking it back reopens the draft.
function onSent(held: { account: string; id: string; draft: MailRef; holdMs: number }): void {
  const sender = mailApi(held.account);
  let cancelled = false;

  island.say({
    title: t("mail.status.sending"),
    duration: held.holdMs,
    actions: [
      {
        label: t("mail.action.cancel_send"),
        press: async () => {
          cancelled = true;

          try {
            const { draft } = await sender.cancelSend(held.id);

            if (draft === null) {
              island.say({ title: t("mail.status.already_sent") });

              return;
            }

            const reopened = await sender.openDraft(draft);

            await composer.value?.start(reopened.draft);
          } catch (error) {
            island.fail(error, t("mail.error.cancel_send_failed"));
          }
        }
      }
    ]
  });

  setTimeout(async () => {
    if (cancelled) return;

    try {
      const { state } = await sender.sendState(held.id);

      if (state?.state === "failed") {
        island.say({ title: t("mail.error.send_failed"), description: t("mail.message.kept_in_drafts", { reason: state.reason }), tone: "error" });
      } else if (state?.state === "sent") {
        island.say({ title: t("mail.message.sent"), tone: "success" });
      }
    } catch {
      // The send's state could not be read; the Sent and Drafts folders say where the message is.
    }

    if (held.account === place.value?.account) void refreshAll();
  }, held.holdMs + 2_000);
}

function onDraftSaved(account: string): void {
  if (account === place.value?.account && place.value.view === "drafts") void loadList();
}

function onFold(index: number, folded: boolean): void {
  if (index === 0) navFolded.value = folded;
}

function foldNav(folded: boolean): void {
  const panel = sides.value?.panelsRef[0];

  if (folded) panel?.collapse();
  else panel?.expand();
}

// Keys on a desktop (docs/mail.md, 「The screen」). None fires inside a field.
function onKey(event: KeyboardEvent): void {
  if (narrow.value || settingsOpen.value) return;

  const shortcut = shortcutOf(event);
  const rows = conversations.value ?? [];

  if (shortcut === null) return;

  event.preventDefault();

  const current = rows[focused.value];

  switch (shortcut) {
    case "compose":
      void compose();
      break;
    case "search":
      list.value?.focusSearch();
      break;
    case "next":
    case "previous": {
      if (rows.length === 0) break;

      focused.value = Math.min(rows.length - 1, Math.max(0, focused.value + (shortcut === "next" ? 1 : -1)));

      const row = rows[focused.value]!;

      if (place.value?.thread !== null) go({ thread: row.key });
      break;
    }
    case "select":
      if (current !== undefined) toggle(current.key);
      break;
    case "star":
      if (current !== undefined) starOne(current);
      break;
    case "trash":
      if (selected.value.size === 0 && current !== undefined) selected.value = new Set([current.key]);
      act(role.value === "trash" || role.value === "junk" ? "delete" : "trash");
      break;
    case "reply":
    case "reply-all":
    case "forward": {
      const last = threadMessages.value?.[threadMessages.value.length - 1];

      if (last !== undefined) void write(shortcut, last);
      break;
    }
  }
}

onMounted(() => window.addEventListener("keydown", onKey));
onBeforeUnmount(() => window.removeEventListener("keydown", onKey));

useHead({ title: computed(() => (title.value === "" ? t("screens.label.mail") : t("mail.title.page_of", { title: title.value }))) });
</script>
