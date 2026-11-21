// Carrying by the pointer on the mail screen (docs/mail.md, 「The screen」 and 「Filters and spam」), driven with
// the mouse in Chromium against a stand-in of the service: a wide list's row dropped on the mail nav moves to
// the folder or takes the label it lands on, the row under the pointer marked while it is carried there, and a
// filter rule carried to another place in the order lands where its line stood. Each result is read back from a
// page loaded again, as the stand-in kept it.

import { afterAll, beforeAll, expect, test } from "bun:test";

import words from "../../i18n/locales/ko.json";
import { ACCOUNT, standBehind, standIn } from "../support/mail-stand-in";
import { standScreen } from "../support/screen";

import type { StandIn } from "../support/mail-stand-in";
import type { Screen } from "../support/screen";
import type { Locator, Page } from "playwright-core";

let screen: Screen;

// What a test may take at most, a bound and never a wait: the dev server builds the screen on its first page.
const STANDING_MS = 300_000;
const DRIVING_MS = 120_000;

beforeAll(async () => {
  screen = await standScreen();
}, STANDING_MS);

afterAll(async () => {
  await screen?.close();
});

// A page of the screen at one of its views, wide enough for the list's rows to be carried, its list read, handed
// to `drive` and closed with its context however `drive` ends.
async function onScreen(state: StandIn, view: string, drive: (page: Page) => Promise<void>): Promise<void> {
  const context = await screen.context({ viewport: { width: 1400, height: 900 } });

  try {
    await standBehind(context, state);

    const page = await context.newPage();

    await visit(page, view);
    await drive(page);
    expect(state.unanswered).toEqual([]);
  } finally {
    await context.close();
  }
}

// A view of the screen opened, or the one open loaded again, and waited on until the screen has read what it
// shows: it reads its list once from what it held and once more as its folders arrive.
async function visit(page: Page, view: string | null): Promise<void> {
  if (view === null) await page.reload();
  else await page.goto(`${screen.origin}/mail/${ACCOUNT}/${encodeURIComponent(view)}`);
  await page.waitForLoadState("networkidle");
}

const rows = (page: Page) => page.locator('[role="row"][data-row]');

// The conversations the list shows, top to bottom, by their keys.
const shown = (page: Page) => rows(page).evaluateAll((all) => all.map((row) => row.getAttribute("data-row") ?? ""));

// The list waited on until it shows these conversations, top to bottom, and held to them: a list that never
// does is shown against them as it last stood.
async function listShows(page: Page, keys: string[]): Promise<void> {
  await page
    .waitForFunction((keys) => [...document.querySelectorAll('[role="row"][data-row]')].map((row) => row.getAttribute("data-row")).join(" ") === keys.join(" "), keys, { timeout: 10_000 })
    .catch(() => undefined);
  expect(await shown(page)).toEqual(keys);
}

// The mail nav's row of a view, as its link names it: the last such, since the account's own row above the
// folders links to its inbox as well.
const navRow = (page: Page, view: string) =>
  page.locator("li", { has: page.locator(`a[href="/mail/${ACCOUNT}/${encodeURIComponent(view)}"]`) }).last();

const shadow = (target: Locator) => target.evaluate((element) => getComputedStyle(element).boxShadow);

// The pointer pressed on `from` and carried over `to`, its middle or its lower part, and held there; the caller
// lets go.
async function carry(page: Page, from: Locator, to: Locator, at: "middle" | "lower" = "middle"): Promise<void> {
  const start = (await from.boundingBox())!;
  const end = (await to.boundingBox())!;

  await page.mouse.move(start.x + start.width / 2, start.y + start.height / 2);
  await page.mouse.down();
  await page.mouse.move(start.x + start.width / 2 + 8, start.y + start.height / 2 + 4, { steps: 2 });
  await page.mouse.move(end.x + end.width / 2, end.y + end.height * (at === "lower" ? 0.8 : 0.5), { steps: 8 });
}

// What the stand-in keeps, waited on until it holds or the bound passes.
async function saved(holds: () => boolean): Promise<void> {
  for (const deadline = Date.now() + 10_000; !holds(); await Bun.sleep(50)) {
    if (Date.now() > deadline) throw new Error("The stand-in was never told the change.");
  }
}

const conversation = (state: StandIn, id: string) => state.messages.find((one) => one.id === id)!;

test("a row carried onto a folder moves there alone, the folder marked while it is over it, and a reload reads it moved", async () => {
  const state = standIn();

  await onScreen(state, "inbox", async (page) => {
    const projects = navRow(page, "folder:Projects");

    expect(await shadow(projects)).toBe("none");

    await carry(page, rows(page).first(), projects);
    await page.waitForFunction((element) => getComputedStyle(element!).boxShadow !== "none", await projects.elementHandle());
    await page.mouse.up();

    await listShows(page, ["thread-m2", "thread-m3"]);
    expect(conversation(state, "m1").folderName).toBe("Projects");
    expect(await shadow(projects)).toBe("none");

    await visit(page, null);
    await listShows(page, ["thread-m2", "thread-m3"]);

    // From the folder it was moved to, back to the inbox; the trash takes the next.
    await visit(page, "folder:Projects");
    await listShows(page, ["thread-m1"]);
    await carry(page, rows(page).first(), navRow(page, "inbox"));
    await page.mouse.up();
    await listShows(page, []);
    expect(conversation(state, "m1").folderName).toBe("INBOX");

    await visit(page, "inbox");
    await listShows(page, ["thread-m1", "thread-m2", "thread-m3"]);
    await carry(page, rows(page).nth(2), navRow(page, "trash"));
    await page.mouse.up();
    await listShows(page, ["thread-m1", "thread-m2"]);
    expect(conversation(state, "m3").folderName).toBe("Trash");
  });
}, DRIVING_MS);

test("chosen rows carried onto a label all take it, and the open view takes no drop", async () => {
  const state = standIn();

  await onScreen(state, "inbox", async (page) => {
    await rows(page).nth(1).locator('[role="checkbox"]').click();
    await rows(page).nth(2).locator('[role="checkbox"]').click();
    await carry(page, rows(page).nth(1), navRow(page, "label:work"));
    await page.mouse.up();
    await saved(() => conversation(state, "m3").labelIds.includes("work"));

    await visit(page, "label:work");
    await listShows(page, ["thread-m2", "thread-m3"]);
    expect(conversation(state, "m1").labelIds).toEqual([]);

    // The inbox open, its own nav row answers nothing to a row carried over it and let go there.
    await visit(page, "inbox");
    await listShows(page, ["thread-m1", "thread-m2", "thread-m3"]);

    const inbox = navRow(page, "inbox");
    const box = (await inbox.boundingBox())!;

    await carry(page, rows(page).first(), inbox);
    await page.mouse.move(box.x + box.width / 2 + 6, box.y + box.height / 2, { steps: 3 });
    expect(await shadow(inbox)).toBe("none");
    await page.mouse.up();
    await listShows(page, ["thread-m1", "thread-m2", "thread-m3"]);
    expect(conversation(state, "m1").folderName).toBe("INBOX");
  });
}, DRIVING_MS);

test("a rule carried by its handle below the last lands there with a line marking it, moves down by its button, and a reload reads the order", async () => {
  const state = standIn();

  await onScreen(state, "inbox", async (page) => {
    const rules = page.locator("li[data-rule]");
    const order = () => rules.evaluateAll((all) => all.map((row) => ["alpha", "beta", "gamma"].find((word) => row.textContent?.includes(word))));
    const settings = async () => {
      await page.getByRole("button", { name: words.mail.action.settings, exact: true }).click();
      await rules.nth(2).waitFor();
    };

    await settings();
    expect(await order()).toEqual(["alpha", "beta", "gamma"]);
    // Each rule leads with its handle, left of its switch.
    const handles = await rules.evaluateAll((all) =>
      all.map((row) => {
        const lead = row.firstElementChild!;

        return lead.getAttribute("aria-hidden") === "true" && lead.getBoundingClientRect().right <= row.querySelector('[role="switch"]')!.getBoundingClientRect().left;
      }),
    );

    expect(handles).toEqual([true, true, true]);

    await carry(page, rules.nth(0).locator(":scope > [aria-hidden=true]"), rules.nth(2), "lower");
    await page.waitForFunction((element) => getComputedStyle(element!).boxShadow.includes("3px"), await rules.nth(2).elementHandle());
    await page.mouse.up();
    await page.waitForFunction(() => document.querySelector("li[data-rule]")?.textContent?.includes("beta") === true);
    expect(await order()).toEqual(["beta", "gamma", "alpha"]);
    expect(await shadow(rules.nth(2))).toBe("none");

    await rules.nth(0).getByRole("button", { name: words.mail.action.move_down, exact: true }).click();
    await page.waitForFunction(() => document.querySelector("li[data-rule]")?.textContent?.includes("gamma") === true);
    expect(await order()).toEqual(["gamma", "beta", "alpha"]);

    await visit(page, null);
    await settings();
    expect(await order()).toEqual(["gamma", "beta", "alpha"]);
    expect(state.rules.map((one) => one.id)).toEqual([3, 2, 1]);
  });
}, DRIVING_MS);
