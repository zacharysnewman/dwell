// Waiting until the game runs (docs/BUGS.md #49): `terrainReady` (the prediction gate) needs only
// the chunks round the player, and on the software renderer (SwiftShader, the suite's CI adapter)
// the page then stops drawing for several seconds while the rest of the streamed view (~150
// chunks) is meshed and first drawn: no frame runs and the controller ticks none (input held then
// moves the player 0 m). A test that moves the player or times the screen right after the join
// first waits that out here: the streamed chunks all meshed, then the game ticking in real time.
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
  await expect
    .poll(
      () =>
        page.evaluate<boolean>(
          '(() => { const t = globalThis.__dwell?.state()?.terrain; return !!t && t.dirty === 0 && t.meshing === 0; })()',
        ),
      { message: 'the streamed chunks meshed', timeout },
    )
    .toBe(true);
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
