import { addMailAccount, startAuthorization } from "./mail-api";

import type { MailAccount } from "../../shared/mail";

// An account added through its provider's page (docs/mail.md, 「The screen」): the browser leaves for the
// page the service names and comes back to the mail screen with the provider's answer in its query. What the
// screen started is kept in this tab's session storage until then, and the screen that comes back finishes
// the adding with it. Nothing here knows a provider: the kind, the page and the answer are the service's.

const KEPT = "bmo-mail-authorization";

interface Waiting {
  id: string;
  kind: string;
  name: string;
}

// Starts an authorization of a kind and sends the browser to the provider's page.
export async function beginAuthorization(kind: string, name: string): Promise<void> {
  const returnTo = `${window.location.origin}${window.location.pathname}`;
  const started = await startAuthorization(kind, returnTo);
  const waiting: Waiting = { id: started.id, kind, name };

  try {
    sessionStorage.setItem(KEPT, JSON.stringify(waiting));
  } catch {
    // A tab that keeps nothing cannot finish the adding; the provider's page still opens, and the adding
    // is started again.
  }

  window.location.assign(started.url);
}

// Finishes the authorization this tab started, with the query the browser came back with; none where the
// tab started none or came back with nothing.
export async function finishAuthorization(query: Record<string, string>): Promise<MailAccount | null> {
  let waiting: Waiting | null = null;

  try {
    const kept = sessionStorage.getItem(KEPT);

    waiting = kept === null ? null : (JSON.parse(kept) as Waiting);
    sessionStorage.removeItem(KEPT);
  } catch {
    return null;
  }

  if (waiting === null || Object.keys(query).length === 0) return null;

  return addMailAccount({
    kind: waiting.kind,
    ...(waiting.name === "" ? {} : { name: waiting.name }),
    authorization: { id: waiting.id, answer: query },
  });
}
