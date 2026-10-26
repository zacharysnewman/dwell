// Phase 2: players move with client prediction, and see each other (native server + local mode).
import { readFileSync } from 'node:fs';
import { expect, test, type BrowserContext, type Page } from '@playwright/test';
import { INVITE_FILE } from './global-setup';

type Vec3 = [number, number, number];
interface DebugState {
  playerId: number;
  active: boolean;
  terrainReady: boolean;
  terrain: { loaded: number; generating: number; meshed: number };
  feet: Vec3;
  remotes: { playerId: number; feet: Vec3; dead: boolean }[];
}

interface Hooks {
  state(): (DebugState & { stats: { snaps: number; ticks: number } }) | null;
  press(code: string, down: boolean): void;
  look(yaw: number, pitch: number): void;
}
// The page's test hooks (window.__dwell, see src/main.ts); evaluated inside the page.
const hooks = () => (globalThis as unknown as { __dwell?: Hooks }).__dwell;

const invite = () => readFileSync(INVITE_FILE, 'utf8');

async function state(page: Page): Promise<DebugState | null> {
  return page.evaluate(`(${hooks.toString()})()?.state() ?? null`);
}

/** Waits until the player is predicted, which starts once the terrain around it has streamed in. */
async function waitActive(page: Page): Promise<DebugState> {
  await expect
    .poll(async () => (await state(page))?.terrainReady ?? false, { timeout: 20_000 })
    .toBe(true);
  const s = await state(page);
  if (!s) throw new Error('no game state');
  return s;
}

/**
 * Waits until the whole view has streamed in and been generated (about 110 chunks). CI
 * machines are small: worldgen workers still busy would starve the pages' frame loops.
 */
async function waitTerrain(page: Page): Promise<DebugState> {
  await expect
    .poll(
      async () => {
        const t = (await state(page))?.terrain;
        return t && t.generating === 0 ? t.loaded : 0;
      },
      { timeout: 30_000 },
    )
    .toBeGreaterThan(100)
    .catch(async (e: unknown) => {
      // Which wait it was: the chunks still generating, loaded and meshing.
      const s = await state(page).catch(() => null);
      console.log('terrain at the timeout', JSON.stringify(s?.terrain ?? null));
      throw e;
    });
  const s = await state(page);
  if (!s) throw new Error('no game state');
  return s;
}

/**
 * Holds W facing `yaw` (0: +Z) for `ticks` predicted ticks (60 per simulated second), and returns
 * the ticks predicted with it held. Counting ticks rather than wall time keeps the distance
 * independent of the frame rate, which CI's software renderer (SwiftShader) holds well below 60 fps
 * with a full view of streamed terrain.
 *
 * The walk runs inside the page: the count starts as the key goes down, and the key comes up
 * between frames once the count is reached. Read from the test, a slow page's frame (it runs up to
 * 60 ticks) could fall between reading the count and pressing the key, its idle ticks counted as
 * the walk's and the key released before any tick saw it: on CI a 15-tick walk moved 0 m.
 */
async function walkForward(page: Page, ticks: number, yaw = 0): Promise<number> {
  const h = hooks.toString();
  return Number(
    await page.evaluate(`new Promise((resolve, reject) => {
      const d = (${h})();
      const count = () => d?.state()?.stats.ticks ?? 0;
      const start = count();
      d?.look(${String(yaw)}, 0);
      d?.press('KeyW', true);
      const deadline = performance.now() + 20000;
      const check = () => {
        const n = count() - start;
        if (n >= ${String(ticks)} || performance.now() > deadline) {
          d?.press('KeyW', false);
          if (n >= ${String(ticks)}) resolve(n);
          else reject(new Error('walked ' + n + ' of ${String(ticks)} ticks in 20 s'));
        } else setTimeout(check, 10);
      };
      check();
    })`),
  );
}

test('local mode: the predicted player walks forward', async ({ page }) => {
  // Level ground: the walk overshoots its 60 ticks by however long releasing the key takes, which
  // on procedural terrain can reach a ledge.
  await page.goto('./?world=flat');
  await waitActive(page);
  const start = await waitTerrain(page);
  await walkForward(page, 60);
  const end = await state(page);
  expect((end?.feet[2] ?? 0) - start.feet[2]).toBeGreaterThan(3); // ~5 m/s
  expect(Math.abs((end?.feet[1] ?? 0) - start.feet[1])).toBeLessThan(0.05);
});

test('local mode streams terrain: Generated chunks, or every chunk explicitly on request', async ({
  page,
}) => {
  test.setTimeout(90_000);
  // The second visit opens the same saved world where the first walk stopped.
  for (const query of ['./?local=1', './?local=1&chunks=full']) {
    await page.goto(query);
    await waitActive(page);
    // The whole view arrives (about 110 chunks), and meshes follow.
    const s = await waitTerrain(page);
    await expect
      .poll(async () => (await state(page))?.terrain.meshed ?? 0, { timeout: 20_000 })
      .toBeGreaterThan(50);
    // The spawn sits in a valley (generator 21), so a slope may block one way: walk each way in
    // turn until one walk moves (the check is that the streamed terrain lets the player walk).
    let moved = 0;
    for (const yaw of [0, 90, 180, 270]) {
      const from = (await state(page))?.feet ?? s.feet;
      await walkForward(page, 60, yaw);
      const end = await state(page);
      moved = Math.max(
        moved,
        Math.hypot((end?.feet[0] ?? 0) - from[0], (end?.feet[2] ?? 0) - from[2]),
      );
      if (moved > 2) break;
    }
    expect(moved).toBeGreaterThan(2);
  }
});

test('two clients on a native server see each other move', async ({ browser }) => {
  // Two pages stream and generate their terrain side by side on CI's few cores.
  test.setTimeout(120_000);
  // Separate contexts: separate device keys (a shared key would replace the first session). Closed
  // at the end: left open, both pages keep generating and rendering, starving the tests after.
  const contexts: BrowserContext[] = [await browser.newContext(), await browser.newContext()];
  const [a, b] = await Promise.all(contexts.map((c) => c.newPage()));
  try {
    for (const [n, p] of [
      ['A', a],
      ['B', b],
    ] as const) {
      p.on('console', (m) => {
        if (m.type() === 'error') console.log(n, 'console', m.text());
      });
      p.on('pageerror', (e) => {
        console.log(n, 'pageerror', e.message, e.stack);
      });
    }
    await a.goto(`./${invite()}`);
    await b.goto(`./${invite()}&netsim=150,20,5`);
    const sa = await waitActive(a);
    await waitActive(b);
    await waitTerrain(a);
    await waitTerrain(b);
    await expect
      .poll(
        async () => (await state(b))?.remotes.some((r) => r.playerId === sa.playerId) ?? false,
        {
          timeout: 10_000,
        },
      )
      .toBe(true);
    const before = (await state(b))?.remotes.find((r) => r.playerId === sa.playerId)?.feet ?? [
      0, 0, 0,
    ];
    await walkForward(a, 90);
    await expect
      .poll(
        async () => {
          const seen = (await state(b))?.remotes.find((r) => r.playerId === sa.playerId)?.feet;
          return (seen?.[2] ?? 0) - before[2];
        },
        { timeout: 10_000 },
      )
      .toBeGreaterThan(3);
    // B's own prediction runs over the simulated 150 ms link without snapping. A short walk: CI's
    // software renderer runs this page at well under 60 ticks/s, and the server repeats a starved
    // client's last input, so a long walk would diverge whatever the prediction does.
    // (Relative to where B stood: the spawn lies wherever the terrain puts it, not at z = 0.)
    const startB = await state(b);
    const walked = await walkForward(b, 15);
    const sb = await state(b);
    expect(
      (sb?.feet[2] ?? 0) - (startB?.feet[2] ?? 0),
      JSON.stringify({ walked, from: startB?.feet, to: sb?.feet }),
    ).toBeGreaterThan(0.5);
    const stats = await b.evaluate(`(${hooks.toString()})()?.state()?.stats`);
    expect((stats as { snaps?: number } | undefined)?.snaps, JSON.stringify(stats)).toBe(0);
  } finally {
    for (const c of contexts) await c.close();
  }
});
