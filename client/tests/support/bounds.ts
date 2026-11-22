// How long each step of standing, driving and putting down the mail screen may take, the budget a browser test's
// bounds are made of. A test or a hook past its bound fails, so each bound is the sum of the waits under it and
// never a guess at one machine's speed, and each wait under a test stays well inside the test's own bound.

/**
 * A wait on the page, a page's load and its network going idle among them. The dev server hands the screen's
 * modules over one by one, so on a loaded machine one load of the page takes well past the driver's own thirty
 * seconds.
 */
export const PAGE_MS = 90_000;

/** A browser opening a context or a page, closing one, or closing itself: moments even on a loaded machine. */
export const BROWSER_MS = 20_000;

/** A new browser launched and answering on its endpoint. */
export const LAUNCH_MS = 30_000;

/** The dev server answering the screen's page, and building what a browser's first load of it asks. */
export const READY_MS = 180_000;

/** The dev server ending once it is asked to, before its process group is killed outright. */
export const STOP_MS = 15_000;

/** The keeper reaching back to the test file once started, the first thing it does. */
export const ARRIVE_MS = 30_000;

/** The keeper ended once its link closes: its browser closed or killed, its dev server ended or killed. */
export const KEEPER_ENDING_MS = BROWSER_MS + STOP_MS + 5_000;

// What a test spends between its loads of the page at most: the driving itself, which takes seconds.
const DRIVING_SPARE_MS = 30_000;

// A context opened and closed at most: asked of a browser that does not answer, which the keeper puts down and
// replaces with a new one, asked in turn, and closed once the test is done with it.
const CONTEXT_MS = BROWSER_MS + BROWSER_MS + LAUNCH_MS + BROWSER_MS + BROWSER_MS + BROWSER_MS;

// What the slack of a hook's bound covers: the moments between the waits it counts.
const HOOK_SPARE_MS = 10_000;

/** What a test that loads the screen `loads` times may take at most: its context, each load and the driving. */
export function drivingBound(loads: number): number {
  return CONTEXT_MS + loads * PAGE_MS + DRIVING_SPARE_MS;
}

/** What standing the screen may take at most: the keeper reached, the dev server and the browser, the first load. */
export const STANDING_MS = ARRIVE_MS + READY_MS + LAUNCH_MS + BROWSER_MS + READY_MS + HOOK_SPARE_MS;

/** What putting the screen down may take at most: the browser closed, the dev server ended, the keeper gone. */
export const CLOSING_MS = BROWSER_MS + KEEPER_ENDING_MS + HOOK_SPARE_MS;

/** What `work` answers, or a failure naming `what` once `ms` pass without an answer. */
export async function bounded<T>(work: Promise<T>, ms: number, what: string): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const late = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new Error(`${what} did not answer within ${ms} ms.`)), ms);
  });

  try {
    return await Promise.race([work, late]);
  } finally {
    clearTimeout(timer);
  }
}
