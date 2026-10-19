import { useIsland } from "bmo-ui/app/domain/island";
import { mailPath, parseMailPath } from "./mail";
import { useMailSummary } from "./mail-summary";

import type { MailSummary, MailSummaryMessage } from "../../shared/mail";
import type { Translate } from "bmo-ui/shared/wording";
import type { ComputedRef } from "vue";

// The notice new mail raises on whatever console page is open (docs/mail.md, 「Reaching the open
// tabs」): one message per account on the island at the top of the screen, naming the sender and
// subject of the newest message and how many came, and the whole of it opens that conversation.
// The mail screen open on that same account raises none — its list refreshes instead — and the
// sidebar's Mail entry carries its mark while any inbox holds unread mail.

export interface MailArrivalNotice {
  account: string;
  address: string;
  // The unread messages that came since the last summary, newest first.
  messages: MailSummaryMessage[];
}

// What arrived between two summaries: per account, the unread messages newer than the newest the
// earlier summary knew, less those the console itself moved into the inbox. The first summary a tab
// reads is what was already there, and says nothing; neither does an account whose server did not
// answer either time.
export function arrivalsBetween(previous: MailSummary | null, next: MailSummary): MailArrivalNotice[] {
  if (previous === null) {
    return [];
  }

  const arrivals: MailArrivalNotice[] = [];

  for (const account of next.accounts) {
    const before = previous.accounts.find((one) => one.account === account.account);

    if (account.failed || before === undefined || before.failed) {
      continue;
    }

    const known = before.newest?.uid ?? 0;
    const fresh = account.recentUnread.filter((message) => message.uid > known && !message.movedIn).sort((a, b) => b.date - a.date);

    if (fresh.length > 0) {
      arrivals.push({ account: account.account, address: account.address, messages: fresh });
    }
  }

  return arrivals;
}

// The account the mail screen at this path is open on, or null when the path is not the mail
// screen. A bare `/mail` opens the first account.
export function mailScreenAccount(path: string, firstAccount: string | null): string | null {
  const match = /^\/mail(?:\/(.*))?$/.exec(path.replace(/\/+$/, ""));

  if (match === null) {
    return null;
  }

  return parseMailPath((match[1] ?? "").split("/").filter((part) => part !== ""), firstAccount)?.account ?? null;
}

// Whether the notice for an account stands down: the mail screen is open on that account, and its
// list refreshes on its own.
export function noticeSuppressed(account: string, path: string, firstAccount: string | null): boolean {
  return mailScreenAccount(path, firstAccount) === account;
}

// The notice's words: which account, how many, who wrote and about what.
export function noticeText(t: Translate, arrival: MailArrivalNotice): { title: string; description: string } {
  const newest = arrival.messages[0]!;
  const sender = newest.from[0];
  const who = sender === undefined ? t("mail.label.no_sender") : sender.name !== "" ? sender.name : sender.address;
  const count = arrival.messages.length;

  return {
    title:
      count === 1
        ? t("mail.title.arrival_one", { address: arrival.address })
        : t("mail.title.arrival_many", { n: count.toLocaleString("ko-KR"), address: arrival.address }),
    description: t("mail.message.arrival", { who, subject: newest.subject.trim() === "" ? t("mail.label.no_subject") : newest.subject })
  };
}

// Where pressing the notice goes: the newest message's conversation, from the account's inbox.
export function noticeTarget(arrival: MailArrivalNotice): string {
  return mailPath({ account: arrival.account, view: "inbox", thread: arrival.messages[0]!.key });
}

export function anyUnread(summary: MailSummary | null): boolean {
  return (summary?.accounts ?? []).some((account) => account.unread > 0);
}

// The notices, for the layout that stands around every page: what goes up on the island, and
// whether the sidebar's Mail entry carries its mark.
export function useMailNotices(t: Translate): { unread: ComputedRef<boolean> } {
  const route = useRoute();
  const island = useIsland(t);

  const { summary } = useMailSummary((next, previous) => {
    const first = next.accounts[0]?.account ?? null;

    for (const arrival of arrivalsBetween(previous, next)) {
      if (noticeSuppressed(arrival.account, route.path, first)) {
        continue;
      }

      island.say({
        key: `mail:${arrival.account}`,
        icon: "i-material-symbols-mail-outline",
        ...noticeText(t, arrival),
        to: noticeTarget(arrival)
      });
    }
  });

  return { unread: computed(() => anyUnread(summary.value)) };
}
