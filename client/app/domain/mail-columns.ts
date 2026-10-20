import { getCurrentInstance, onMounted } from "vue";

import { clampColumn } from "./mail";

import type { Ref } from "vue";

// How wide the mail list's sender and date columns were dragged (docs/mail.md, 「Where each piece of state
// lives」), kept in the browser they were dragged in, so the list is found at the widths it was left at.

const MAIL_COLUMNS_KEY = "bmo-mail.columns";

// A column nobody dragged has no width here at all, and takes the width its own contents ask for instead.
export interface MailColumnWidths {
  from: number | null;
  date: number | null;
}

export interface MailColumnSettings {
  widths: Ref<MailColumnWidths>;
  setWidth: (column: "from" | "date", width: number | null) => void;
}

let restored = false;

// A width kept in this browser is put back once the component asking for it has mounted: the server drew
// the default, and a value put back before hydration leaves the widths the server drew standing.
function afterMount(restore: () => void): void {
  if (getCurrentInstance() === null) {
    restore();
    return;
  }

  onMounted(restore);
}

function readMailColumns(): MailColumnWidths {
  const held = (value: unknown, column: "from" | "date"): number | null => (typeof value === "number" && Number.isFinite(value) ? clampColumn(column, value) : null);

  try {
    const raw = window.localStorage.getItem(MAIL_COLUMNS_KEY);
    const parsed: unknown = raw === null ? null : JSON.parse(raw);

    if (parsed === null || typeof parsed !== "object") {
      return { from: null, date: null };
    }

    const stored = parsed as { from?: unknown; date?: unknown };

    return { from: held(stored.from, "from"), date: held(stored.date, "date") };
  } catch {
    return { from: null, date: null };
  }
}

export function useMailColumns(): MailColumnSettings {
  const widths = useState<MailColumnWidths>("bmo-mail-columns", () => ({ from: null, date: null }));

  if (import.meta.client && !restored) {
    restored = true;
    afterMount(() => {
      widths.value = readMailColumns();
    });
  }

  function setWidth(column: "from" | "date", width: number | null): void {
    widths.value = { ...widths.value, [column]: width === null ? null : clampColumn(column, width) };

    if (import.meta.client) {
      try {
        window.localStorage.setItem(MAIL_COLUMNS_KEY, JSON.stringify(widths.value));
      } catch {
        // A browser that refuses storage still keeps the columns as they stand for this visit.
      }
    }
  }

  return { widths, setWidth };
}
