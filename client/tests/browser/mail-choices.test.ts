// The mail screen's values chosen through bmo-ui's one select (docs/mail.md, 「Writing」 and 「The screen」), driven
// in Chromium against a stand-in of the service at a wide window and at a phone's width: a list of more than seven
// choices opens on a search and a short one on none, the toolbar's move and label selects act on the chosen mail,
// the address fields keep what a mail client's address field does, and a pick in a select inside the settings
// asks nothing of a form holding changes while a close still does. Whether a list searches is the layer's own rule
// and its own tests; these hold the screen's lists to it, and its own handling around them.

import { afterAll, beforeAll, expect, test } from "bun:test";

import words from "../../i18n/locales/ko.json";
import { ACCOUNT, standBehind, standIn } from "../support/mail-stand-in";
import { drivingBound, standScreen } from "../support/screen";

import type { StandIn } from "../support/mail-stand-in";
import type { Locator, Page } from "playwright-core";
import type { Screen } from "../support/screen";

let screen: Screen;

// What standing the screen may take at most, a bound and never a wait: the dev server builds the screen on its
// first page.
const STANDING_MS = 300_000;
// Each test loads the screen once.
const DRIVING_MS = drivingBound(1);

const WIDTHS = [
  { name: "a wide window", viewport: { width: 1280, height: 900 }, touch: false },
  { name: "a phone", viewport: { width: 360, height: 780 }, touch: true },
] as const;

type Width = (typeof WIDTHS)[number];

beforeAll(async () => {
  screen = await standScreen();
}, STANDING_MS);

afterAll(async () => {
  await screen?.close();
});

// A page of the screen's inbox at one width, its list read, handed to `drive` and closed with its context however
// `drive` ends.
async function onScreen(state: StandIn, width: Width, drive: (page: Page) => Promise<void>): Promise<void> {
  const context = await screen.context({ viewport: width.viewport, hasTouch: width.touch, isMobile: width.touch });

  try {
    await standBehind(context, state);

    const page = await context.newPage();

    await page.goto(`${screen.origin}/mail/${ACCOUNT}/inbox`);
    await page.waitForLoadState("networkidle");
    await drive(page);
    expect(state.unanswered).toEqual([]);
  } finally {
    await screen.release(context);
  }
}

// The select whose control is named so, the open list it stands, and that list's search field.
const control = (page: Page, name: string) => page.locator(`button[aria-label="${name}"]`).first();
const openList = (page: Page) => page.locator('[data-slot="content"][data-side]:has([role="listbox"])');
const search = (page: Page) => openList(page).locator("input");
const choice = (page: Page, label: string) => openList(page).getByRole("option", { name: label, exact: true });

// The list a select opens, waited on until it stands, and whether it stands with a search field.
async function opens(page: Page, select: Locator): Promise<boolean> {
  await select.click();
  await openList(page).waitFor();

  return (await search(page).count()) > 0;
}

// Esc goes to what stands on top, a tooltip among them, so the pointer is rested on the list first and any
// tooltip it left is let go before Esc is pressed.
async function closes(page: Page): Promise<void> {
  const list = (await openList(page).boundingBox())!;

  await page.mouse.move(list.x + 4, list.y + 4);
  await page.locator('[role="tooltip"]').first().waitFor({ state: "detached" });
  await page.keyboard.press("Escape");
  await openList(page).waitFor({ state: "detached" });
}

// A state of many folders and labels, so the lists of places and labels run past seven choices.
function crowded(): StandIn {
  const state = standIn();

  state.folders = ["Projects", "Receipts", "Travel", "Family", "Archive 1", "Archive 2", "Archive 3", "Archive 4"];
  state.labels = ["Work", "Home", "Bills", "Trips", "Kids", "Car", "Garden", "Books", "Music"].map((name, index) => ({ id: name.toLowerCase(), name, colour: "blue", position: index + 1 }));

  return state;
}

// What the stand-in keeps, waited on until it holds or the bound passes.
async function saved(holds: () => boolean): Promise<void> {
  for (const deadline = Date.now() + 10_000; !holds(); await Bun.sleep(50)) {
    if (Date.now() > deadline) throw new Error("The stand-in was never told the change.");
  }
}

const rows = (page: Page) => page.locator('[role="row"][data-row]');

test("the toolbar's move select searches its places and moves the chosen mail to the one chosen", async () => {
  const state = crowded();

  await onScreen(state, WIDTHS[0], async (page) => {
    await rows(page).first().locator('[role="checkbox"]').click();

    expect(await opens(page, control(page, words.mail.action.move))).toBe(true);
    await search(page).fill("travel");
    await choice(page, "Travel").click();

    await saved(() => state.messages.find((one) => one.id === "m1")!.folderName === "Travel");
  });
}, DRIVING_MS);

test("the toolbar's label select half-marks a label part of the choice has, and choosing it puts it on all of it", async () => {
  const state = crowded();

  state.messages[0]!.labelIds = ["work"];

  await onScreen(state, WIDTHS[0], async (page) => {
    await rows(page).nth(0).locator('[role="checkbox"]').click();
    await rows(page).nth(1).locator('[role="checkbox"]').click();

    expect(await opens(page, control(page, words.mail.label.labels))).toBe(true);
    expect(await choice(page, "Work").getByLabel(words.mail.aria.label_on_some).count()).toBe(1);
    expect(await choice(page, "Home").getByLabel(words.mail.aria.label_on_some).count()).toBe(0);

    await choice(page, "Work").click();
    await saved(() => state.messages.slice(0, 2).every((one) => one.labelIds.includes("work")));
  });
}, DRIVING_MS);

for (const width of WIDTHS) {
  test(`at ${width.name}, the settings' folder list searches and the rule field's four do not, and a pick asks nothing of a changed form while Esc does`, async () => {
    const state = crowded();

    await onScreen(state, width, async (page) => {
      if (width.touch) await page.getByRole("button", { name: words.mail.label.role_inbox }).first().click();
      await page.getByRole("button", { name: words.mail.action.settings }).first().click();

      const settings = page.getByRole("dialog").last();

      await settings.getByRole("switch").first().click();

      const standing = await page.getByRole("dialog").count();

      expect(await opens(page, control(page, words.mail.aria.rule_field))).toBe(false);
      await choice(page, words.mail.label.subject).click();
      await openList(page).waitFor({ state: "detached" });

      expect(await opens(page, control(page, words.mail.aria.rule_action))).toBe(false);
      await choice(page, words.mail.label.do_move).click();
      await openList(page).waitFor({ state: "detached" });

      expect(await opens(page, control(page, words.mail.label.folders))).toBe(true);
      await choice(page, "Receipts").click();
      await openList(page).waitFor({ state: "detached" });
      // The settings stand with no question over them until Esc asks one.
      expect(await page.getByRole("dialog").count()).toBe(standing);

      await settings.locator("h3").first().hover();
      await page.keyboard.press("Escape");
      await page.waitForFunction((standing) => document.querySelectorAll('[role="dialog"]').length === standing + 1, standing, { timeout: 10_000 });
    });
  }, DRIVING_MS);

  test(`at ${width.name}, an address field makes chips of what is typed, ended on any keyboard, left, pasted or found, Backspace takes one back and Enter on nothing none`, async () => {
    const state = standIn();

    state.known = [{ name: "Fay", address: "fay@example.com", usedCount: 3, lastUsedAt: 0, position: 1 }];

    await onScreen(state, width, async (page) => {
      await page.getByRole("button", { name: words.mail.action.compose }).first().click();
      // The pointer leaves the compose button, so its tooltip does not open over what is driven next.
      await page.mouse.move(0, 0);

      const field = control(page, words.mail.label.to);
      const chips = () => page.locator("[data-address-chip]").evaluateAll((all) => all.map((one) => one.textContent?.trim() ?? ""));

      // The chips waited on until they read as expected, and held to it: a field that never gets there is
      // shown against them as it last stood.
      const chipsAre = async (expected: string[]) => {
        await page
          .waitForFunction((expected) => [...document.querySelectorAll("[data-address-chip]")].map((one) => one.textContent?.trim() ?? "").join(" ") === expected.join(" "), expected, { timeout: 10_000 })
          .catch(() => undefined);
        expect(await chips()).toEqual(expected);
      };

      // Typed on the closed field, ended by a comma, then by a semicolon.
      await field.focus();
      await page.keyboard.type("ann@example.com,");
      await page.keyboard.type("bob@example.com;");
      await chipsAre(["ann@example.com", "bob@example.com"]);

      // Ended by Enter with no known address standing to be taken.
      await search(page).pressSequentially("cy@example.com");
      await page.keyboard.press("Enter");
      await chipsAre(["ann@example.com", "bob@example.com", "cy@example.com"]);

      // Ended by a comma a phone's keyboard writes, naming no key as it writes it.
      await search(page).pressSequentially("hal@example.com");
      await page.keyboard.insertText(",");
      await chipsAre(["ann@example.com", "bob@example.com", "cy@example.com", "hal@example.com"]);
      expect(await search(page).inputValue()).toBe("");

      // Left typed as the field is left.
      await search(page).pressSequentially("dee@example.com");
      await closes(page);
      await chipsAre(["ann@example.com", "bob@example.com", "cy@example.com", "hal@example.com", "dee@example.com"]);

      // Several pasted at once.
      await opens(page, field);
      await search(page).evaluate((input) => {
        const data = new DataTransfer();

        data.setData("text", "eve@example.com, gus@example.com");
        input.dispatchEvent(new ClipboardEvent("paste", { clipboardData: data, bubbles: true, cancelable: true }));
      });
      await chipsAre(["ann@example.com", "bob@example.com", "cy@example.com", "hal@example.com", "dee@example.com", "eve@example.com", "gus@example.com"]);

      // Backspace with nothing typed takes the last back out.
      await search(page).focus();
      await page.keyboard.press("Backspace");
      await chipsAre(["ann@example.com", "bob@example.com", "cy@example.com", "hal@example.com", "dee@example.com", "eve@example.com"]);

      // A known address found for the words typed and taken from the list.
      await search(page).pressSequentially("fay");
      await choice(page, "Fay <fay@example.com>").click();
      await closes(page);
      await chipsAre(["ann@example.com", "bob@example.com", "cy@example.com", "hal@example.com", "dee@example.com", "eve@example.com", "Fay <fay@example.com>"]);

      // Enter with nothing typed lets no address go.
      await opens(page, field);
      await search(page).focus();
      await page.keyboard.press("Enter");
      await chipsAre(["ann@example.com", "bob@example.com", "cy@example.com", "hal@example.com", "dee@example.com", "eve@example.com", "Fay <fay@example.com>"]);
    });
  }, DRIVING_MS);
}
