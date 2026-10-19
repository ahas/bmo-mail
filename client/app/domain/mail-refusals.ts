import type { Translate } from "bmo-ui/shared/wording";

// A refusal of the mail service, said in the screen's own words (docs/api.md, 「Refusals」): the service
// answers an English sentence for a log and a code, with the refusal's named values beside them, and the
// screen shows the pack's sentence for that code, `mail.error.<code>`, those values filled in. A code the
// pack holds no sentence for, or a failure that carries no code at all, is said by the caller's own words.
export function mailFailure(t: Translate, error: unknown, fallback: string): string {
  const body = (error as { data?: Record<string, unknown> } | null)?.data;
  const code = body?.code;

  if (typeof code !== "string" || code === "") {
    return fallback;
  }

  const values: Record<string, string | number> = {};

  for (const [name, value] of Object.entries(body ?? {})) {
    if (name !== "code" && name !== "error" && (typeof value === "string" || typeof value === "number")) {
      values[name] = value;
    }
  }

  const key = `mail.error.${code}`;
  const said = t(key, values);

  return said === key ? fallback : said;
}
