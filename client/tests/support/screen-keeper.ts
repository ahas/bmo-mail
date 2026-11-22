import { spawn } from "node:child_process";
import { connect, createServer } from "node:net";
import { createInterface } from "node:readline";

import { chromium } from "playwright-core";

import { BROWSER_MS, LAUNCH_MS, READY_MS, STOP_MS, bounded } from "./bounds";

import type { ChildProcessByStdio } from "node:child_process";
import type { Socket } from "node:net";
import type { Readable } from "node:stream";
import type { BrowserServer } from "playwright-core";

// The processes a browser test file shares, the mail client's dev server and a headless Chromium, kept by a
// program of their own outside the test file's process. Once one test or hook of a file runs past its bound, bun
// puts down every process that file started, so the test file starts this keeper through `setsid --fork`, which
// leaves it no child of the test file's, and a test that runs out of time takes nothing the next one needs.
//
// The keeper is run with one argument, the loopback port the test file listens on, and reaches back to it at
// once. On that link it answers each ask with where the screen and its browser stand, and it puts everything
// down once the test file ends its side of the link, which the test file's process ending does as well however
// it ends; the keeper closes its own side last.

/** An ask of the keeper, one JSON object a line: a new browser, the old one put down; ask 0 is the standing itself. */
export interface Ask {
  ask: number;
}

/** The keeper's answer to an ask, one JSON object a line: where the screen and a browser stand, or why they do not. */
export type Answer = { ask: number; origin: string; endpoint: string } | { ask: number; failure: string };

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

// The client's Nuxt project served by its dev server on a free loopback port. The dev server runs in a process
// group of its own, so taking it down takes every process it started with it.
class DevServer {
  readonly origin: string;
  private readonly server: ChildProcessByStdio<null, Readable, Readable>;
  private readonly ended: Promise<void>;
  private exited = false;
  // What the dev server said, kept for the failure that names it; it ends at once when its output is cast aside,
  // so the output is read.
  private said = "";

  private constructor(port: number) {
    this.origin = `http://127.0.0.1:${port}`;
    this.server = spawn(process.execPath, ["x", "nuxi", "dev", "--port", String(port), "--host", "127.0.0.1"], {
      cwd: new URL("../host", import.meta.url).pathname,
      detached: true,
      stdio: ["ignore", "pipe", "pipe"],
    });
    this.ended = new Promise((resolve) => this.server.once("exit", () => resolve()));
    this.server.once("exit", () => {
      this.exited = true;
    });

    const hear = (chunk: Buffer) => {
      this.said = (this.said + chunk.toString()).slice(-4000);
    };

    this.server.stdout.on("data", hear);
    this.server.stderr.on("data", hear);
  }

  static async start(): Promise<DevServer> {
    return new DevServer(await freePort());
  }

  /** The dev server's answer to the screen's page once it gives one, polled until it does or `READY_MS` pass. */
  async ready(): Promise<void> {
    const deadline = Date.now() + READY_MS;

    while (Date.now() < deadline) {
      if (this.exited) throw this.failure("The mail client's dev server ended before it answered.");

      try {
        if ((await fetch(`${this.origin}/mail/`)).ok) return;
      } catch {
        // Not listening yet.
      }

      await Bun.sleep(250);
    }

    throw this.failure(`The mail client's dev server did not answer ${this.origin}/mail/ in time.`);
  }

  /** The dev server's whole group asked to end, killed where it does not within `STOP_MS`, and its end waited for. */
  async stop(): Promise<void> {
    this.signal("SIGTERM");

    try {
      await bounded(this.ended, STOP_MS, "The mail client's dev server ending");
    } catch {
      this.signal("SIGKILL");
      await this.ended;
    }
  }

  private signal(signal: NodeJS.Signals): void {
    if (this.exited || this.server.pid === undefined) return;

    try {
      process.kill(-this.server.pid, signal);
    } catch {
      // The group ended between the check and the signal.
    }
  }

  private failure(message: string): Error {
    return new Error(`${message}\nThe dev server said:\n${this.said}`);
  }
}

// The keeper handles its own signals, so a browser server leaves them alone.
async function launched(): Promise<BrowserServer> {
  return chromium.launchServer({ handleSIGINT: false, handleSIGTERM: false, handleSIGHUP: false, timeout: LAUNCH_MS });
}

// A browser closed in its own time where it answers, and its process put down where it does not.
async function putDown(browser: BrowserServer): Promise<void> {
  try {
    await bounded(browser.close(), BROWSER_MS, "Closing the browser");
  } catch {
    await browser.kill();
  }
}

class Keeper {
  private devServer: DevServer | undefined;
  private browser: BrowserServer | undefined;
  private ending: Promise<void> | undefined;
  // The asks answered one after another, in the order they came.
  private asks: Promise<void> = Promise.resolve();

  constructor(private readonly link: Socket) {
    // The test file ending its side asks the keeper to end, and the keeper closes its own once it has put
    // everything down, which is the end the test file waits for.
    link.on("error", () => undefined); // Its close follows, and ends the keeper.
    link.once("end", () => void this.end());
    link.once("close", () => void this.end());
    for (const signal of ["SIGTERM", "SIGINT", "SIGHUP"] as const) process.once(signal, () => void this.end());

    createInterface({ input: link }).on("line", (line) => {
      const { ask } = JSON.parse(line) as Ask;

      this.asks = this.asks.then(() => this.relaunch(ask));
    });
  }

  /** The dev server and a browser stood up and said as ask 0's answer, or the failure said and the keeper ended. */
  async stand(): Promise<void> {
    try {
      this.devServer = await DevServer.start();
      await this.devServer.ready();
      await this.launch();
      await this.answerWith(0);
    } catch (failure) {
      await this.say({ ask: 0, failure: (failure as Error).message });
      await this.end();
    }
  }

  private async relaunch(ask: number): Promise<void> {
    try {
      const old = this.browser;

      this.browser = undefined;
      if (old !== undefined) await putDown(old);
      await this.launch();
      await this.answerWith(ask);
    } catch (failure) {
      await this.say({ ask, failure: (failure as Error).message });
    }
  }

  // A browser launched, and put down at once where the keeper began to end meanwhile.
  private async launch(): Promise<void> {
    const browser = await launched();

    if (this.ending === undefined) this.browser = browser;
    else await putDown(browser);
  }

  private async answerWith(ask: number): Promise<void> {
    if (this.devServer === undefined || this.browser === undefined) return;

    await this.say({ ask, origin: this.devServer.origin, endpoint: this.browser.wsEndpoint() });
  }

  // An answer written and flushed to the link, where the link still stands.
  private say(answer: Answer): Promise<void> {
    if (this.link.destroyed) return Promise.resolve();

    return new Promise((resolve) => this.link.write(`${JSON.stringify(answer)}\n`, () => resolve()));
  }

  // The browser and the dev server put down, and the keeper's own process ended.
  private end(): Promise<void> {
    this.ending ??= (async () => {
      if (this.browser !== undefined) await putDown(this.browser);
      await this.devServer?.stop();
      this.link.destroy();
      process.exit(0);
    })();

    return this.ending;
  }
}

if (import.meta.main) {
  const link = connect({ port: Number(process.argv[2]), host: "127.0.0.1", allowHalfOpen: true });

  link.once("connect", () => void new Keeper(link).stand());
}
