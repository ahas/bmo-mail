// The accounts modal's two tabs (docs/mail.md, 「The screen」, the accounts), driven in Chromium against a stand-in
// of the service at a phone's width and at a wide window: the servers tab lists a company's servers under its
// name and the platform's under the platform's word, with no edit or remove; a server of the owner's own is added
// through the server form after a connection test; an account is added on servers chosen in the account form's
// two selects, the outgoing one through the select's new-server entry; and removing a server an account stands on
// is refused in the pack's words under its row. Each press is made only where a finger reaches the control: in
// view, scrolled to where it is out of it, and with nothing drawn over it, and no part of the modal runs past
// the screen's width.

import { afterAll, beforeAll, expect, test } from "bun:test";

import words from "../../i18n/locales/ko.json";
import { CLOSING_MS, STANDING_MS, drivingBound } from "../support/bounds";
import { ACCOUNT, COMPANY, UNREACHED_HOST, standBehind, standIn } from "../support/mail-stand-in";
import { standScreen } from "../support/screen";

import type { StandIn } from "../support/mail-stand-in";
import type { Locator, Page } from "playwright-core";
import type { Screen } from "../support/screen";

let screen: Screen;

const WIDTHS = [
  { name: "a phone", viewport: { width: 360, height: 780 }, touch: true },
  { name: "a wide window", viewport: { width: 1280, height: 900 }, touch: false },
] as const;

type Width = (typeof WIDTHS)[number];

const OWN_HOST = "mail.example.org";
const NEW_SMTP_HOST = "submit.example.org";
const ADDRESS = "new@example.org";

beforeAll(async () => {
  screen = await standScreen();
}, STANDING_MS);

afterAll(async () => {
  await screen?.close();
}, CLOSING_MS);

// A page of the screen's inbox at one width, its list read, handed to `drive` and closed with its context however
// `drive` ends.
async function onScreen(state: StandIn, width: Width, drive: (page: Page) => Promise<void>): Promise<void> {
  const context = await screen.context({ viewport: width.viewport, hasTouch: width.touch, isMobile: width.touch, locale: "ko-KR" });

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

// A control pressed as a hand presses it: scrolled into view where it stands out of it, held to stand inside the
// screen with nothing drawn over its centre, then tapped by a finger or clicked by a pointer.
async function press(page: Page, width: Width, control: Locator): Promise<void> {
  await control.scrollIntoViewIfNeeded();

  const box = (await control.boundingBox())!;
  const centre = { x: box.x + box.width / 2, y: box.y + box.height / 2 };

  expect(centre.x).toBeGreaterThanOrEqual(0);
  expect(centre.y).toBeGreaterThanOrEqual(0);
  expect(centre.x).toBeLessThanOrEqual(width.viewport.width);
  expect(centre.y).toBeLessThanOrEqual(width.viewport.height);
  expect(await control.evaluate((element, at) => element.contains(document.elementFromPoint(at.x, at.y)), centre)).toBe(true);

  if (width.touch) await control.tap();
  else await control.click();
}

// Whether anything the page draws runs past the screen's width: the page itself, or a box inside the dialogs.
async function runsWide(page: Page): Promise<string[]> {
  return page.evaluate(() => {
    const wide = document.documentElement.scrollWidth > window.innerWidth ? ["the page"] : [];

    for (const box of document.querySelectorAll('[role="dialog"] *')) {
      const style = getComputedStyle(box);

      if (style.overflowX !== "visible" && style.overflowX !== "hidden" && box.scrollWidth > box.clientWidth + 1) wide.push(box.className.toString());
      if (box.getBoundingClientRect().right > window.innerWidth + 1) wide.push(`${box.tagName}: ${box.textContent?.slice(0, 40)}`);
    }

    return wide;
  });
}

const accountsDialog = (page: Page) => page.getByRole("dialog").filter({ has: page.getByRole("tab", { name: words.mail.label.tab_servers }) });
const serverForm = (page: Page) => page.getByRole("dialog").filter({ has: page.getByRole("button", { name: words.mail.action.server_test }) });
const openList = (page: Page) => page.locator('[data-slot="content"][data-side]:has([role="listbox"])');
const serverRow = (page: Page, text: string) => accountsDialog(page).locator("li[data-server]").filter({ hasText: text });

// The accounts opened from the mail nav, which a phone holds in its drawer.
async function openAccounts(page: Page, width: Width): Promise<Locator> {
  if (width.touch) await press(page, width, page.getByRole("button", { name: words.mail.label.role_inbox }).first());
  await press(page, width, page.getByRole("button", { name: words.mail.action.accounts }).last());

  const dialog = accountsDialog(page);

  await dialog.waitFor();

  return dialog;
}

// A server select's list opened, its words read in order, and the choice holding `text` pressed.
async function chooseIn(page: Page, width: Width, select: Locator, text: string): Promise<string> {
  await press(page, width, select);
  await openList(page).waitFor();

  const shown = await openList(page).innerText();

  await press(page, width, openList(page).getByRole("option").filter({ hasText: text }).first());
  await openList(page).waitFor({ state: "detached" });

  return shown;
}

// The server form filled with a host and kept, waited on until it has gone.
async function keepServer(page: Page, width: Width, host: string): Promise<void> {
  const form = serverForm(page);

  await form.getByLabel(words.mail.label.server_host).fill(host);
  await press(page, width, form.locator('button[type="submit"]'));
  await form.waitFor({ state: "detached" });
}

// The pointer rested on a dialog's head, away from any control, so no tooltip it left takes the next Esc, and the
// focus put on the dialog itself.
async function rest(page: Page, dialog: Locator): Promise<void> {
  const box = (await dialog.boundingBox())!;

  await page.mouse.move(box.x + 8, box.y + 8);
  await page.locator('[role="tooltip"]').first().waitFor({ state: "detached" });
  await dialog.locator("h2").first().click();
}

// What the stand-in keeps, waited on until it holds or the bound passes.
async function saved(holds: () => boolean): Promise<void> {
  for (const deadline = Date.now() + 10_000; !holds(); await Bun.sleep(50)) {
    if (Date.now() > deadline) throw new Error("The stand-in was never told the change.");
  }
}

for (const width of WIDTHS) {
  test(`at ${width.name}, servers are added, tested and chosen for a new account, and one an account stands on is not removed`, async () => {
    const state = standIn();

    state.moves = true;

    await onScreen(state, width, async (page) => {
      const dialog = await openAccounts(page, width);

      // The servers tab: a company's servers under its name, the platform's under its word, none changed here.
      await press(page, width, dialog.getByRole("tab", { name: words.mail.label.tab_servers }));
      await expect(serverRow(page, "imap.example.com").innerText()).resolves.toContain(`${COMPANY} · ${words.mail.label.server_accounts.replace("{n}", "4")}`);
      await expect(serverRow(page, "relay.example.net").innerText()).resolves.toContain(`${words.mail.label.holder_platform} · ${words.mail.label.server_accounts.replace("{n}", "0")}`);
      expect(await dialog.getByRole("button", { name: words.mail.action.server_edit }).count()).toBe(0);
      expect(await dialog.getByRole("button", { name: words.mail.action.server_remove }).count()).toBe(0);

      // The server form opened and left untouched closes with no question.
      const form = serverForm(page);

      await press(page, width, dialog.getByRole("button", { name: words.mail.action.server_add }));
      await form.waitFor();
      await rest(page, form);
      await page.keyboard.press("Escape");
      await form.waitFor({ state: "detached" });

      // An own server added through the form: tested where it is not reached and where it is, then kept.
      await press(page, width, dialog.getByRole("button", { name: words.mail.action.server_add }));
      await form.waitFor();
      await expect(form.getByLabel(words.mail.label.server_port).getAttribute("placeholder")).resolves.toBe(words.mail.label.default_value.replace("{value}", "993"));
      await form.getByLabel(words.mail.label.server_host).fill(UNREACHED_HOST);
      await press(page, width, form.getByRole("button", { name: words.mail.action.server_test }));
      await form.getByRole("status").filter({ hasText: words.mail.status.server_unreached }).waitFor();
      await form.getByLabel(words.mail.label.server_host).fill(OWN_HOST);
      await press(page, width, form.getByRole("button", { name: words.mail.action.server_test }));
      await form.getByRole("status").filter({ hasText: words.mail.status.server_reached }).waitFor();
      expect(state.serverTests.map((one) => [one.protocol, one.host, one.port, one.security])).toEqual([
        ["imap", UNREACHED_HOST, 993, "tls"],
        ["imap", OWN_HOST, 993, "tls"],
      ]);
      expect(await runsWide(page)).toEqual([]);
      await keepServer(page, width, OWN_HOST);

      const own = serverRow(page, OWN_HOST);

      await own.waitFor();
      await expect(own.innerText()).resolves.toContain(words.mail.label.holder_own);
      expect(await own.getByRole("button", { name: words.mail.action.server_edit }).count()).toBe(1);

      // The account form: its incoming server chosen among the company's and the owner's own, its outgoing one
      // made through the select's new-server entry, which opens the form on SMTP and chooses what it keeps.
      await press(page, width, dialog.getByRole("tab", { name: words.mail.label.tab_accounts }));
      await dialog.getByLabel(words.mail.label.account_address).fill(ADDRESS);

      const incoming = dialog.locator(`button[aria-label="${words.mail.label.incoming_server}"]`);
      const outgoing = dialog.locator(`button[aria-label="${words.mail.label.outgoing_server}"]`);
      const incomingList = await chooseIn(page, width, incoming, OWN_HOST);

      expect(incomingList.indexOf(COMPANY)).toBeLessThan(incomingList.indexOf(words.mail.label.holder_own));
      expect(incomingList.indexOf(words.mail.label.holder_own)).toBeLessThan(incomingList.indexOf(words.mail.label.server_new));

      const outgoingList = await chooseIn(page, width, outgoing, words.mail.label.server_new);

      expect(outgoingList.indexOf(COMPANY)).toBeLessThan(outgoingList.indexOf(words.mail.label.holder_platform));
      await form.waitFor();
      await expect(form.innerText()).resolves.toContain(words.mail.server_protocol.smtp);
      await keepServer(page, width, NEW_SMTP_HOST);
      await expect(outgoing.innerText()).resolves.toContain(NEW_SMTP_HOST);

      await dialog.getByLabel(words.mail.account_field.password).fill("secret");
      expect(await runsWide(page)).toEqual([]);
      await press(page, width, dialog.locator('button[type="submit"]').filter({ hasText: words.mail.action.account_add }));
      await saved(() => state.added.length === 1);

      const ownId = state.servers.find((one) => one.host === OWN_HOST)!.id;
      const smtpId = state.servers.find((one) => one.host === NEW_SMTP_HOST)!.id;

      expect(state.added[0]).toMatchObject({ kind: "imap-smtp", address: ADDRESS, incomingServer: ownId, outgoingServer: smtpId, secrets: { password: "secret" } });
      await dialog.waitFor({ state: "detached" });
      await page.waitForURL(/\/mail\/new\/inbox$/);

      // The own server the new account stands on is not removed: the refusal is said under its row.
      const reopened = await openAccounts(page, width);

      await press(page, width, reopened.getByRole("tab", { name: words.mail.label.tab_servers }));
      await expect(own.innerText()).resolves.toContain(words.mail.label.server_accounts.replace("{n}", "1"));
      await press(page, width, own.getByRole("button", { name: words.mail.action.server_remove }));
      await press(page, width, own.getByRole("button", { name: words.mail.action.server_remove_confirm, exact: true }));
      await own.getByRole("alert").filter({ hasText: words.mail.error["server-in-use"] }).waitFor();
      expect(state.servers.some((one) => one.id === ownId)).toBe(true);
      expect(await runsWide(page)).toEqual([]);

      // The account form holding an address asks before the modal closes, and staying keeps it; written back
      // to what it opened with, it closes with no question.
      await press(page, width, reopened.getByRole("tab", { name: words.mail.label.tab_accounts }));

      const address = reopened.getByLabel(words.mail.label.account_address);
      const standing = await page.getByRole("dialog").count();

      await address.fill(ADDRESS);
      await rest(page, reopened);
      await page.keyboard.press("Escape");
      await page.waitForFunction((standing) => document.querySelectorAll('[role="dialog"], [role="alertdialog"]').length > standing, standing);
      await page.keyboard.press("Escape");
      await page.waitForFunction((standing) => document.querySelectorAll('[role="dialog"], [role="alertdialog"]').length === standing, standing);
      await expect(address.inputValue()).resolves.toBe(ADDRESS);

      await address.fill("");
      await rest(page, reopened);
      await page.keyboard.press("Escape");
      await reopened.waitFor({ state: "detached" });
    });
  }, drivingBound(2));
}
