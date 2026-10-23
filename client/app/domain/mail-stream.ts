import { mailAddress } from "./mail-api";

import type { MailEvent } from "../../shared/mail";

// The mail service's event stream as a screen reads it (docs/api.md, 「The event stream」), through the
// console's relay as bmo-ui's console plugin contract has it. The browser resumes it after the last event
// it read on its own; where the service no longer keeps the events since then it refuses the stream, and
// the screen opens it afresh and reads its collections again.

export const MAIL_EVENT_KINDS = ["message.created", "message.changed", "message.deleted", "folder.changed", "send.changed"] as const;

// How long a stream the service refused waits before it is opened afresh.
const REOPEN_MS = 3_000;

// Calls `onEvent` with each event while the screen holding it stands, and `onReset` each time the stream is
// opened afresh after it was refused, which is when what happened in between is not known.
export function useMailStream(onEvent: (event: MailEvent) => void, onReset: () => void): void {
  let source: EventSource | null = null;
  let reopening: ReturnType<typeof setTimeout> | null = null;
  let standing = false;

  function read(message: MessageEvent<string>): void {
    try {
      onEvent(JSON.parse(message.data) as MailEvent);
    } catch {
      // An event that does not read is passed over; the next one says where things stand.
    }
  }

  function open(): void {
    source = new EventSource(mailAddress("/events"));

    for (const kind of MAIL_EVENT_KINDS) source.addEventListener(kind, read);

    source.onerror = () => {
      // A stream the browser is reconnecting on its own resumes by itself; one it gave up on was refused.
      if (source?.readyState !== EventSource.CLOSED || !standing) return;

      source = null;
      reopening = setTimeout(() => {
        reopening = null;

        if (!standing) return;

        open();
        onReset();
      }, REOPEN_MS);
    };
  }

  onMounted(() => {
    standing = true;
    open();
  });

  onBeforeUnmount(() => {
    standing = false;
    source?.close();
    source = null;

    if (reopening !== null) clearTimeout(reopening);
  });
}
