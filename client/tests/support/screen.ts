import { spawn } from "node:child_process";
import { createServer } from "node:net";

import { chromium } from "playwright-core";

import type { Browser, BrowserContext, BrowserContextOptions } from "playwright-core";

// The mail screen standing on its own for a browser test: the client's Nuxt project served by its dev server
// on a free loopback port, the screen's page at `/mail`, and a headless Chromium to drive it. The dev server
// runs in a process group of its own, so taking it down takes every process it started with it.

export interface Screen {
  origin: string;
  browser: Browser;
  /** A browser context of the screen whose every wait on the page is bounded by `PAGE_MS`. */
  context: (options?: BrowserContextOptions) => Promise<BrowserContext>;
  /** A context closed, where its browser still stands; one whose browser went has nothing left to close. */
  release: (context: BrowserContext) => Promise<void>;
  close: () => Promise<void>;
}

// How long a wait on the page may take, a page's load and its network going idle among them. The dev server hands
// the screen's modules over one by one, so on a loaded machine one load of the page takes well past the driver's
// own thirty seconds.
const PAGE_MS = 90_000;

// What a test spends between its loads of the page at most: the driving itself, which takes seconds.
const DRIVING_SPARE_MS = 30_000;

/**
 * What a test that loads the screen `loads` times may take at most: each load within `PAGE_MS`, and the driving
 * between them. A test past its bound has bun put down every process its file started, the dev server among them,
 * so the bound is the loads' and not a guess at one machine's speed.
 */
export function drivingBound(loads: number): number {
  return loads * PAGE_MS + DRIVING_SPARE_MS;
}

// How long the dev server may take to answer the screen's page, and to build what a browser's first load of it asks.
const READY_MS = 180_000;

function freePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const probe = createServer();

    probe.once("error", reject);
    probe.listen(0, "127.0.0.1", () => {
      const address = probe.address();
      const port = typeof address === "object" && address !== null ? address.port : 0;

      probe.close(() => resolve(port));
    });
  });
}

// The dev server's answer to the screen's page once it gives one, polled until it does or the bound passes.
async function ready(origin: string, exited: () => boolean): Promise<void> {
  const deadline = Date.now() + READY_MS;

  while (Date.now() < deadline) {
    if (exited()) throw new Error("The mail client's dev server ended before it answered.");

    try {
      if ((await fetch(`${origin}/mail/`)).ok) return;
    } catch {
      // Not listening yet.
    }

    await Bun.sleep(250);
  }

  throw new Error(`The mail client's dev server did not answer ${origin}/mail/ in time.`);
}

// The screen's page loaded once in a browser of its own, which has the dev server build the modules the page
// asks for, a build it makes on a browser's first ask alone: on a loaded machine that build outlasts a test's own
// wait on its first page, so it is taken here, within the standing's bound, before any test opens one.
async function warmed(browser: Browser, origin: string): Promise<void> {
  const page = await browser.newPage();

  try {
    await page.goto(`${origin}/mail/`, { waitUntil: "load", timeout: READY_MS });
  } finally {
    await page.close();
  }
}

export async function standScreen(): Promise<Screen> {
  const port = await freePort();
  const origin = `http://127.0.0.1:${port}`;
  const server = spawn("bun", ["x", "nuxi", "dev", "--port", String(port), "--host", "127.0.0.1"], {
    cwd: new URL("../host", import.meta.url).pathname,
    detached: true,
    stdio: ["ignore", "pipe", "pipe"],
  });
  let exited = false;
  const ended = new Promise<void>((resolve) => server.once("exit", () => resolve()));
  // What the dev server said, kept for the failure that names it; it ends at once when its output is cast
  // aside, so the output is read.
  let said = "";
  const hear = (chunk: Buffer) => {
    said = (said + chunk.toString()).slice(-4000);
  };

  server.stdout.on("data", hear);
  server.stderr.on("data", hear);
  server.once("exit", () => {
    exited = true;
  });

  // The dev server's whole group taken down, and its end waited for.
  const stop = async () => {
    if (!exited && server.pid !== undefined) process.kill(-server.pid, "SIGTERM");
    await ended;
  };

  try {
    await ready(origin, () => exited);

    const browser = await chromium.launch();

    try {
      await warmed(browser, origin);
    } catch (failure) {
      await browser.close();
      throw failure;
    }

    return {
      origin,
      browser,
      context: async (options) => {
        const context = await browser.newContext(options);

        context.setDefaultTimeout(PAGE_MS);
        context.setDefaultNavigationTimeout(PAGE_MS);

        return context;
      },
      release: async (context) => {
        if (context.browser()?.isConnected() === true) await context.close();
      },
      close: async () => {
        await browser.close();
        await stop();
      },
    };
  } catch (failure) {
    await stop();
    throw new Error(`${(failure as Error).message}\nThe dev server said:\n${said}`);
  }
}
