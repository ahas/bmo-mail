import type { Translate } from "bmo-ui/shared/wording";

// A refusal of the mail service, said in the screen's own words (docs/api.md, 「Refusals」): the service
// answers a kebab-case code and an English sentence for a log, and the screen shows the pack's sentence for
// that code, `mail.error.<code>`. A code the pack holds no sentence for, or a failure that carries no code at
// all, is said by the caller's own words.
export function mailFailure(t: Translate, error: unknown, fallback: string): string {
  const code = (error as { data?: { code?: unknown } } | null)?.data?.code;

  if (typeof code !== "string" || code === "") {
    return fallback;
  }

  const key = `mail.error.${code}`;
  const said = t(key);

  return said === key ? fallback : said;
}
