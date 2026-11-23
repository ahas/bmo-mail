// What the mail screen offers for an account whose kind moves no mail between folders (docs/mail.md, 「The
// screen」), a POP3 account's: driven in Chromium against a stand-in of the service declaring that kind, at a wide
// window, at a window short of room and at a phone's width. Nothing that moves mail stands — trash, spam, the move
// select, a folder taking a carried row, the `#` key, a rule's moving actions and the always-spam senders — while
// what does not move mail stands as on any account. The same screen over a kind that moves mail draws them, so
// every control looked for here is one the screen draws by that name.

import { afterAll, beforeAll, expect, test } from "bun:test";

import words from "../../i18n/locales/ko.json";
import { CLOSING_MS, STANDING_MS, drivingBound } from "../support/bounds";
import { ACCOUNT, standBehind, standIn } from "../support/mail-stand-in";
import { standScreen } from "../support/screen";

import type { StandIn } from "../support/mail-stand-in";
import type { Locator, Page } from "playwright-core";
import type { Screen } from "../support/screen";

let screen: Screen;

// Wide enough for the list beside the thread to stand its whole bar, no action folded into its menu.
const WIDE = { viewport: { width: 1920, height: 1000 }, touch: false };
const SHORT = { viewport: { width: 900, height: 900 }, touch: false };
const PHONE = { viewport: { width: 360, height: 780 }, touch: true };

type Width = typeof WIDE;

beforeAll(async () => {
  screen = await standScreen();
}, STANDING_MS);

afterAll(async () => {
  await screen?.close();
}, CLOSING_MS);

// An account of a kind that moves mail or not; one that moves none holds the inbox and the drafts alone, as a
// POP3 account does.
function account(moves: boolean): StandIn {
  const state = standIn();

  state.moves = moves;

  if (!moves) {
    state.roles = ["inbox", "drafts"];
    state.folders = [];
  }

  return state;
}

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

const rows = (page: Page) => page.locator('[role="row"][data-row]');
const tools = (page: Page) => page.getByRole("toolbar", { name: words.mail.aria.list_tools });
// A control the screen names so: a button, or a select's own control.
const named = (scope: Page | Locator, name: string) => scope.locator(`:is(button, [role="combobox"])[aria-label="${name}"]`);

// How many of each control that moves mail the list's toolbar draws: trash, spam and the move select.
async function moving(page: Page): Promise<number[]> {
  const bar = tools(page);

  return await Promise.all([words.mail.action.trash, words.mail.action.report_spam, words.mail.action.move].map((name) => named(bar, name).count()));
}

// The toolbar's cells that hold nothing to press or read: each would stand as a divider with nothing beside it.
const emptyCells = (page: Page) =>
  tools(page).evaluate((bar) => [...bar.children].filter((cell) => cell.querySelector("button, [role='combobox'], [role='checkbox']") === null && (cell.textContent ?? "").trim() === "").length);

// The names of what the toolbar's one more menu holds, opened and read.
async function moreMenu(page: Page): Promise<string[]> {
  await tools(page).locator('button[aria-haspopup="menu"]').last().click();

  const menu = page.getByRole("menu");

  await menu.waitFor();

  const items = await menu.getByRole("menuitem").allInnerTexts();

  await page.keyboard.press("Escape");
  await menu.waitFor({ state: "detached" });

  return items.map((one) => one.trim());
}

// What the stand-in keeps, waited on until it holds or the bound passes.
async function saved(holds: () => boolean): Promise<void> {
  for (const deadline = Date.now() + 10_000; !holds(); await Bun.sleep(50)) {
    if (Date.now() > deadline) throw new Error("The stand-in was never told the change.");
  }
}

const conversation = (state: StandIn, id: string) => state.messages.find((one) => one.id === id)!;

test("at a wide window the chosen mail offers no trash, spam or move where the account moves none, and `#` moves nothing", async () => {
  await onScreen(account(true), WIDE, async (page) => {
    await rows(page).first().locator('[role="checkbox"]').click();
    await named(tools(page), words.mail.action.trash).waitFor();

    expect(await moving(page)).toEqual([1, 1, 1]);
  });

  const state = account(false);

  await onScreen(state, WIDE, async (page) => {
    await rows(page).first().locator('[role="checkbox"]').click();
    await named(tools(page), words.mail.action.mark_unread).waitFor();

    expect(await moving(page)).toEqual([0, 0, 0]);
    expect(await emptyCells(page)).toBe(0);
    expect(await named(tools(page), words.mail.action.star).count()).toBe(1);
    expect(await named(tools(page), words.mail.label.labels).count()).toBe(1);

    // Keys are handled in the order pressed, so the star the last one sets on the focused row is saved after
    // anything `#` would have done to the chosen one.
    await page.locator("body").press("#");
    await page.locator("body").press("j");
    await page.locator("body").press("s");
    await saved(() => state.messages.some((one) => one.flags.includes("\\Flagged")));

    expect(state.messages.map((one) => one.folderName)).toEqual(["INBOX", "INBOX", "INBOX"]);
  });
}, drivingBound(2));

test("at a window short of room the gathered bar offers no trash or move and its menu no spam where the account moves none", async () => {
  await onScreen(account(false), SHORT, async (page) => {
    await rows(page).first().locator('[role="checkbox"]').click();
    await named(tools(page), words.mail.action.mark_unread).waitFor();

    // The bar gathered: star stands in its one menu rather than beside it.
    expect(await named(tools(page), words.mail.action.star).count()).toBe(0);
    expect(await moving(page)).toEqual([0, 0, 0]);
    expect(await emptyCells(page)).toBe(0);
    expect(await moreMenu(page)).toEqual([words.mail.action.star]);
  });
}, drivingBound(1));

test("at a phone's width the choosing row offers no trash or move and its menu no spam where the account moves none", async () => {
  await onScreen(account(false), PHONE, async (page) => {
    const box = (await rows(page).first().boundingBox())!;
    const touch = await page.context().newCDPSession(page);
    const point = { x: box.x + box.width / 2, y: box.y + box.height / 2 };

    await touch.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [point] });
    await named(page, words.mail.action.end_choosing).waitFor();
    await touch.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });

    expect(await moving(page)).toEqual([0, 0, 0]);
    expect(await emptyCells(page)).toBe(0);
    expect(await named(tools(page), words.mail.action.mark_unread).count()).toBe(1);
    expect(await moreMenu(page)).toEqual([words.mail.action.star]);
  });
}, drivingBound(1));

test("a carried row is taken by a label but by no folder where the account moves none", async () => {
  const state = account(false);

  await onScreen(state, WIDE, async (page) => {
    const navRow = (view: string) => page.locator("li", { has: page.locator(`a[href="/mail/${ACCOUNT}/${view}"]`) }).last();
    const shadow = (target: Locator) => target.evaluate((element) => getComputedStyle(element).boxShadow);
    const carry = async (to: Locator) => {
      const start = (await rows(page).first().boundingBox())!;
      const end = (await to.boundingBox())!;

      await page.mouse.move(start.x + start.width / 2, start.y + start.height / 2);
      await page.mouse.down();
      await page.mouse.move(start.x + start.width / 2 + 8, start.y + start.height / 2 + 4, { steps: 2 });
      await page.mouse.move(end.x + end.width / 2, end.y + end.height / 2, { steps: 8 });
    };
    const drafts = navRow("drafts");
    const work = navRow(encodeURIComponent("label:work"));

    await carry(drafts);
    expect(await shadow(drafts)).toBe("none");
    await page.mouse.up();

    // The label takes the next carry; by then the one let go over the drafts has done whatever it would.
    await carry(work);
    await page.waitForFunction((element) => getComputedStyle(element!).boxShadow !== "none", await work.elementHandle());
    await page.mouse.up();
    await saved(() => conversation(state, "m1").labelIds.includes("work"));

    expect(conversation(state, "m1").folderName).toBe("INBOX");
  });
}, drivingBound(1));

test("the settings offer a rule no moving action and stand no always-spam senders where the account moves none", async () => {
  await onScreen(account(false), WIDE, async (page) => {
    await page.getByRole("button", { name: words.mail.action.settings, exact: true }).click();

    const settings = page.getByRole("dialog").last();

    await settings.getByRole("heading", { name: words.mail.label.labels, exact: true }).waitFor();
    await settings.locator(`button[aria-label="${words.mail.aria.rule_action}"]`).click();

    const options = await page.getByRole("option").allInnerTexts();

    await page.keyboard.press("Escape");
    await page.getByRole("option").first().waitFor({ state: "detached" });

    expect(options.map((one) => one.trim())).toEqual([words.mail.label.do_label, words.mail.label.do_star, words.mail.label.do_read]);
    expect(await settings.getByRole("heading", { name: words.mail.title.spam_senders }).count()).toBe(0);
    expect(await settings.getByRole("button", { name: words.mail.action.spam_header_rule }).count()).toBe(0);

    // Left untouched, the settings close on Esc with no question.
    await settings.locator("h3").first().hover();
    await page.keyboard.press("Escape");
    await settings.waitFor({ state: "detached" });
  });
}, drivingBound(1));
