// Waiting until the game runs (docs/BUGS.md #49): `terrainReady` (the prediction gate) needs only
// the chunks round the player, and on the software renderer (SwiftShader, the suite's CI adapter)
// the page then stops drawing for several seconds while the rest of the streamed view (~150
// chunks) is meshed and first drawn: no frame runs and the controller ticks none (input held then
// moves the player 0 m). A test that moves the player or times the screen right after the join
// first waits that out here: most of the streamed view meshed, then the game ticking in real time.
import type { Page } from '@playwright/test';

/** Chunks left to mesh that count as "most of the view meshed". */
const MOST_MESHED = 10;
/** The longest wait for the view to mesh before checking the ticks alone (ms). */
const MESH_WAIT_MS = 20_000;

/** The controller's ticks so far (`__dwell.state().stats.ticks`), or null before the game. */
function ticks(page: Page): Promise<number | null> {
  return page.evaluate<number | null>(
    '(() => globalThis.__dwell?.state()?.stats?.ticks ?? null)()',
  );
}

/** The streamed chunks still to mesh (`state().terrain`'s dirty and meshing), or null. */
function unmeshed(page: Page): Promise<number | null> {
  return page.evaluate<number | null>(
    '(() => { const t = globalThis.__dwell?.state()?.terrain; return t ? t.dirty + t.meshing : null; })()',
  );
}

/**
 * Resolves once most of the streamed view is meshed (at most `MOST_MESHED` chunks left; on CI's
 * runners the count need not reach 0 while light and water keep re-meshing, so this stops waiting
 * for it after `MESH_WAIT_MS`), and then the game has ticked at least `minPerS` ticks a second
 * over each of two consecutive half-second windows (60 a second is real time; a page drawing a
 * few frames a second still catches up its ticks, a stalled one ticks none; CI's generated-terrain
 * pages never sustain 30, so the bar is 10).
 */
export async function untilTicking(page: Page, minPerS = 10, timeout = 60_000): Promise<boolean> {
  const meshWait = Date.now() + MESH_WAIT_MS;
  while (Date.now() < meshWait) {
    const n = await unmeshed(page);
    if (n !== null && n <= MOST_MESHED) break;
    await page.waitForTimeout(250);
  }
  // Best effort: on CI's generated-terrain pages the tick rate can stay under any bar for a minute,
  // so this waits up to `timeout` and then lets the test go on; tests that need movement poll for
  // it themselves (spawn.spec's `walk`, lod.spec's flight toggle).
  let good = 0;
  let last = await ticks(page);
  const end = Date.now() + timeout;
  while (Date.now() < end) {
    await page.waitForTimeout(500);
    const now = await ticks(page);
    good = last !== null && now !== null && now - last >= minPerS / 2 ? good + 1 : 0;
    last = now;
    if (good >= 2) return true;
  }
  return false;
}
