// Waiting until the game runs (docs/BUGS.md #49): on the software renderer (SwiftShader, the
// suite's CI adapter) the page stops drawing for several seconds shortly after the terrain is
// ready, while the GPU process works through the first frames that draw it; no frame runs and the
// controller ticks none (input held then moves the player 0 m). A test that moves the player,
// times the screen or clicks the menu right after the join first waits that out here.
import { expect, type Page } from '@playwright/test';

/** The controller's ticks so far (`__dwell.state().stats.ticks`), or null before the game. */
function ticks(page: Page): Promise<number | null> {
  return page.evaluate<number | null>(
    '(() => globalThis.__dwell?.state()?.stats?.ticks ?? null)()',
  );
}

/**
 * Resolves once the game has ticked at least `minPerS` ticks a second over each of two
 * consecutive half-second windows (60 a second is real time; a page drawing a few frames a second
 * still catches up its ticks, a stalled one ticks none).
 */
export async function untilTicking(page: Page, minPerS = 30, timeout = 60_000): Promise<void> {
  let good = 0;
  let last = await ticks(page);
  await expect
    .poll(
      async () => {
        await page.waitForTimeout(500);
        const now = await ticks(page);
        good = last !== null && now !== null && now - last >= minPerS / 2 ? good + 1 : 0;
        last = now;
        return good >= 2;
      },
      { message: 'the game ticking in real time', timeout, intervals: [0] },
    )
    .toBe(true);
}
