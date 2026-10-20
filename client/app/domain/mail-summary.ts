import { mailSummary } from "./mail-api";

import type { MailSummary } from "../../shared/mail";
import type { Ref } from "vue";

// Every mail account's inbox at a glance, kept current for as long as a screen holds it (docs/mail.md,
// 「Reaching the open tabs」): read when the screen mounts, and again whenever the screen hears that a
// folder's counts moved or the stream had to be opened afresh.

export interface MailSummaryHandle {
  summary: Ref<MailSummary | null>;
  refresh: () => Promise<void>;
}

// A read in flight takes any further ask with it: one more read follows it, however many asked.
export function useMailSummary(): MailSummaryHandle {
  const summary = ref<MailSummary | null>(null);
  let reading: Promise<void> | null = null;
  let again = false;

  async function read(): Promise<void> {
    do {
      again = false;

      try {
        summary.value = await mailSummary();
      } catch {
        // A summary that could not be read leaves the last one standing; the next change reads again.
      }
    } while (again);
  }

  function refresh(): Promise<void> {
    if (reading !== null) {
      again = true;

      return reading;
    }

    reading = read().finally(() => {
      reading = null;
    });

    return reading;
  }

  onMounted(() => {
    void refresh();
  });

  return { summary, refresh };
}
