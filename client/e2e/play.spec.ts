// Phase 2: players move with client prediction, and see each other (native server + local mode).
import { readFileSync } from 'node:fs';
import { expect, test, type Page } from '@playwright/test';
import { INVITE_FILE } from './global-setup';

type Vec3 = [number, number, number];
interface DebugState {
  playerId: number;
  active: boolean;
  feet: Vec3;
  remotes: { playerId: number; feet: Vec3; dead: boolean }[];
}

interface Hooks {
  state(): (DebugState & { stats: { snaps: number } }) | null;
  press(code: string, down: boolean): void;
  look(yaw: number, pitch: number): void;
}
// The page's test hooks (window.__dwell, see src/main.ts); evaluated inside the page.
const hooks = () => (globalThis as unknown as { __dwell?: Hooks }).__dwell;

const invite = () => readFileSync(INVITE_FILE, 'utf8');

async function state(page: Page): Promise<DebugState | null> {
  return page.evaluate(`(${hooks.toString()})()?.state() ?? null`);
}

async function waitActive(page: Page): Promise<DebugState> {
  await expect
    .poll(async () => (await state(page))?.active ?? false, { timeout: 20_000 })
    .toBe(true);
  const s = await state(page);
  if (!s) throw new Error('no game state');
  return s;
}

/** Holds W for `ms` facing yaw 0 (+Z). */
async function walkForward(page: Page, ms: number): Promise<void> {
  const h = hooks.toString();
  await page.evaluate(`(${h})()?.look(0, 0); (${h})()?.press('KeyW', true);`);
  await page.waitForTimeout(ms);
  await page.evaluate(`(${h})()?.press('KeyW', false);`);
}

test('local mode: the predicted player walks forward', async ({ page }) => {
  await page.goto('./');
  const start = await waitActive(page);
  await walkForward(page, 1000);
  const end = await state(page);
  expect((end?.feet[2] ?? 0) - start.feet[2]).toBeGreaterThan(3); // ~5 m/s
  expect(Math.abs((end?.feet[1] ?? 0) - start.feet[1])).toBeLessThan(0.05);
});

test('two clients on a native server see each other move', async ({ browser }) => {
  // Separate contexts: separate device keys (a shared key would replace the first session).
  const a = await (await browser.newContext()).newPage();
  const b = await (await browser.newContext()).newPage();
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
  await expect
    .poll(async () => (await state(b))?.remotes.some((r) => r.playerId === sa.playerId) ?? false, {
      timeout: 10_000,
    })
    .toBe(true);
  const before = (await state(b))?.remotes.find((r) => r.playerId === sa.playerId)?.feet ?? [
    0, 0, 0,
  ];
  // Two pages share one CPU renderer (SwiftShader) in CI and may run below 60 ticks/s: walk long
  // enough to cover 3 m either way; this checks that B sees A move, not A's speed.
  await walkForward(a, 2000);
  await expect
    .poll(
      async () => {
        const seen = (await state(b))?.remotes.find((r) => r.playerId === sa.playerId)?.feet;
        return (seen?.[2] ?? 0) - before[2];
      },
      { timeout: 10_000 },
    )
    .toBeGreaterThan(3);
  // B's own prediction runs over the simulated 150 ms link without snapping.
  await walkForward(b, 1000);
  const sb = await state(b);
  expect(sb?.feet[2] ?? 0).toBeGreaterThan(1);
  const snaps = await b.evaluate(`(${hooks.toString()})()?.state()?.stats.snaps ?? 1`);
  expect(snaps).toBe(0);
});
