import { spawn } from "node:child_process";
import { createServer } from "node:net";
import { createInterface } from "node:readline";

import { chromium } from "playwright-core";

import { ARRIVE_MS, BROWSER_MS, KEEPER_ENDING_MS, LAUNCH_MS, PAGE_MS, READY_MS, bounded } from "./bounds";

import type { Answer, Ask } from "./screen-keeper";
import type { AddressInfo, Socket } from "node:net";
import type { Browser, BrowserContext, BrowserContextOptions } from "playwright-core";

// The mail screen standing on its own for a browser test: the client's Nuxt project served by its dev server
// on a free loopback port, the screen's page at `/mail`, and a headless Chromium to drive it. Both run under the
// screen's keeper, a process of their own outside the test file's, so a test that runs past its bound leaves
// them standing for the tests after it.

export interface Screen {
  origin: string;
  /**
   * A browser context of the screen whose every wait on the page is bounded by `PAGE_MS`. A browser that does
   * not open one within `BROWSER_MS` is put down and a new one launched in its place, which is asked once.
   */
  context: (options?: BrowserContextOptions) => Promise<BrowserContext>;
  /** A context closed, where its browser still stands; one whose browser went has nothing left to close. */
  release: (context: BrowserContext) => Promise<void>;
  /** The browser, the dev server and the keeper put down, within `CLOSING_MS`. */
  close: () => Promise<void>;
}

// The keeper's link, read as the answers to the asks this side makes one at a time.
class KeeperLink {
  private asked = 0;
  private waiting: { ask: number; resolve: (answer: Answer) => void; reject: (failure: Error) => void } | undefined;
  private readonly gone: Promise<void>;
  private ended = false;

  constructor(private readonly link: Socket) {
    this.gone = new Promise((resolve) => link.once("close", () => resolve()));
    link.on("error", () => undefined); // Its close follows.
    link.once("close", () => {
      this.ended = true;
      this.waiting?.reject(KeeperLink.endedFailure());
    });
    createInterface({ input: link }).on("line", (line) => {
      const answer = JSON.parse(line) as Answer;

      // An answer to an ask given up on is no answer to the one made since.
      if (answer.ask === this.waiting?.ask) this.waiting.resolve(answer);
    });
  }

  /** Where the screen and its browser stand, once the keeper has stood them up. */
  standing(): Promise<{ origin: string; endpoint: string }> {
    return this.answer(0, READY_MS + LAUNCH_MS);
  }

  /** The endpoint of a new browser, the old one put down. */
  async relaunched(): Promise<string> {
    this.asked += 1;

    const answer = this.answer(this.asked, BROWSER_MS + LAUNCH_MS);
    const ask: Ask = { ask: this.asked };

    this.link.write(`${JSON.stringify(ask)}\n`);

    return (await answer).endpoint;
  }

  /** The link closed, which has the keeper put down what it keeps, and the keeper's end waited for. */
  async close(): Promise<void> {
    this.link.end();
    await bounded(this.gone, KEEPER_ENDING_MS, "The screen's keeper ending");
  }

  private async answer(ask: number, ms: number): Promise<{ origin: string; endpoint: string }> {
    const answer = new Promise<Answer>((resolve, reject) => {
      if (this.ended) reject(KeeperLink.endedFailure());
      else this.waiting = { ask, resolve, reject };
    });

    try {
      const answered = await bounded(answer, ms, `The screen's keeper's answer to ask ${ask}`);

      if ("failure" in answered) throw new Error(answered.failure);

      return answered;
    } finally {
      // A test that ran out of time goes on in the background, and its ask given up on leaves a later one waiting.
      if (this.waiting?.ask === ask) this.waiting = undefined;
    }
  }

  private static endedFailure(): Error {
    return new Error("The screen's keeper ended.");
  }
}

// The keeper started through `setsid --fork`, which leaves it no child of this process, and its link once it
// reaches back to the loopback port it is handed.
async function keeper(): Promise<KeeperLink> {
  const door = createServer();

  await new Promise<void>((resolve, reject) => {
    door.once("error", reject);
    door.listen(0, "127.0.0.1", () => resolve());
  });

  try {
    const { port } = door.address() as AddressInfo;
    const program = new URL("./screen-keeper.ts", import.meta.url).pathname;
    const arrived = new Promise<Socket>((resolve, reject) => {
      door.once("connection", resolve);
      spawn("setsid", ["--fork", process.execPath, program, String(port)], { stdio: "ignore" }).once("error", reject);
    });

    return new KeeperLink(await bounded(arrived, ARRIVE_MS, "The screen's keeper reaching back"));
  } finally {
    door.close();
  }
}

async function connected(endpoint: string): Promise<Browser> {
  return chromium.connect(endpoint, { timeout: BROWSER_MS });
}

async function opened(browser: Browser, options: BrowserContextOptions | undefined): Promise<BrowserContext> {
  const context = await bounded(browser.newContext(options), BROWSER_MS, "Opening a browser context");

  context.setDefaultTimeout(PAGE_MS);
  context.setDefaultNavigationTimeout(PAGE_MS);

  return context;
}

// The screen's page loaded once in a browser of its own, which has the dev server build the modules the page
// asks for, a build it makes on a browser's first ask alone: on a loaded machine that build outlasts a test's own
// wait on its first page, so it is taken here, within the standing's bound, before any test opens one.
async function warmed(browser: Browser, origin: string): Promise<void> {
  const page = await bounded(browser.newPage(), BROWSER_MS, "Opening a page");

  try {
    await page.goto(`${origin}/mail/`, { waitUntil: "load", timeout: READY_MS });
  } finally {
    await bounded(page.close(), BROWSER_MS, "Closing a page");
  }
}

export async function standScreen(): Promise<Screen> {
  const link = await keeper();

  try {
    const { origin, endpoint } = await link.standing();
    let browser = await connected(endpoint);

    await warmed(browser, origin);

    return {
      origin,
      context: async (options) => {
        try {
          return await opened(browser, options);
        } catch {
          browser = await connected(await link.relaunched());

          return opened(browser, options);
        }
      },
      release: async (context) => {
        if (context.browser()?.isConnected() === true) await bounded(context.close(), BROWSER_MS, "Closing a browser context");
      },
      close: async () => {
        await bounded(browser.close(), BROWSER_MS, "Leaving the browser").catch(() => undefined);
        await link.close();
      },
    };
  } catch (failure) {
    await link.close();
    throw failure;
  }
}
