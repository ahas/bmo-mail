import type { MailSummary } from "../../shared/mail";
import type { Ref } from "vue";

// Every mail account's inbox at a glance, kept current for as long as a screen holds it (docs/mail.md,
// 「Reaching the open tabs」). It is read when the screen mounts, when the stream says an account
// changed, and each time the stream opens again — mail that came while it was closed was never said
// on it, so nothing is lost to a dropped stream.

export interface MailSummaryHandle {
  summary: Ref<MailSummary | null>;
  refresh: () => Promise<void>;
}

// A read in flight takes any further ask with it: one more read follows it, however many asked.
export function useMailSummary(onRead?: (next: MailSummary, previous: MailSummary | null) => void): MailSummaryHandle {
  const summary = ref<MailSummary | null>(null);
  let reading: Promise<void> | null = null;
  let again = false;

  async function read(): Promise<void> {
    do {
      again = false;

      try {
        const next = await $fetch<MailSummary>("/api/mail/summary");
        const previous = summary.value;

        summary.value = next;
        onRead?.(next, previous);
      } catch {
        // A summary that could not be read leaves the last one standing; the next word reads again.
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

  useMailEvents(() => {
    void refresh();
  });

  return { summary, refresh };
}
