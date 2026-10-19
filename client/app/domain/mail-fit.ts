import { bodyFit, FIT_ID, MIN_FRAME_HEIGHT, SCROLLBAR_ALLOWANCE } from "./mail-frame";

// Fitting a mail body to the frame it is shown in (docs/mail.md, 「Reading」). Nothing inside the
// frame is allowed to run, so the measuring is done from outside it: the console reads how wide the
// body wants to be, lays it out at that width, and scales the whole of it down to the frame's. The
// frame's height follows the scaled body, so the thread region is still the only thing that
// scrolls.

export interface FittedFrame {
  // The height the frame has to be for the body to stand in it whole.
  height: number;
  scale: number;
  // Whether the body is still wider than the frame with the shrinking at its limit, which is the
  // one case the frame scrolls sideways over the rest instead of cutting it off.
  scrolls: boolean;
}

// A fitter for one frame. It remembers the frame width the body was last laid out for: a frame that
// has since changed width is measured from nothing again, and one that has not keeps the width
// already settled on, so fitting an already-fitted body writes nothing and cannot wake whatever
// watches the frame for changes.
export function bodyFitter(frame: HTMLIFrameElement): () => FittedFrame | null {
  let laidOutFor = -1;

  return () => {
    const box = frame.contentDocument?.getElementById(FIT_ID);

    if (box === undefined || box === null) {
      return null;
    }

    const width = frame.clientWidth;

    if (width !== laidOutFor) {
      box.style.width = "";
      box.style.transform = "";
      laidOutFor = width;
    }

    const page = box.ownerDocument.documentElement;
    // What the body wants: the width it is laid out at, or the wider one its content spills to.
    const fit = bodyFit(width, Math.max(box.offsetWidth, page.scrollWidth));

    if (box.style.width !== fit.width || box.style.transform !== fit.transform) {
      box.style.width = fit.width;
      box.style.transform = fit.transform;
    }

    const scrolls = page.scrollWidth > page.clientWidth;
    // The box's own rectangle is already the scaled one, which is the height the frame has to be.
    const height = Math.max(MIN_FRAME_HEIGHT, Math.ceil(box.getBoundingClientRect().height) + (scrolls ? SCROLLBAR_ALLOWANCE : 0));

    return { height, scale: fit.scale, scrolls };
  };
}
