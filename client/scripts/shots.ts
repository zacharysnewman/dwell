// Screenshots of fixed views, for comparing how the game looks before and after a change to its
// colours or lighting (IMPLEMENTATION_PLAN.md Phase 7; WORLD_GENERATION.md §1.5):
//   node --experimental-strip-types scripts/shots.ts BASE_URL OUT_DIR [view…]
// BASE_URL is a served build (`npm run build && npx vite preview`, e.g. http://localhost:4173), and
// OUT_DIR gets one PNG per view plus `frame.json`, the average frame time at the spawn view. Run it
// against the build before and after, then compare the two directories.
//
// The views are fixed by seed, position and look direction, in a local world, flying to each (the
// player has no teleport). Positions come from the terrain generator's map for seed 7: a coast
// 60 m east of the spawn, a forest edge 300 m west, a 190 m mountain range 1.2 km north-west.
// Headless Chromium draws in software, slowly; each view waits for the world to finish loading.
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { chromium, type Page } from '@playwright/test';

const SEED = 7;
const WIDTH = 960;
const HEIGHT = 540;

type Vec3 = [number, number, number];

/** The page's debug hooks (main.ts `__dwell`), as far as this script uses them. */
interface Hooks {
  state(): {
    terrainReady: boolean;
    feet: number[];
    terrain: { generating: number; meshing: number };
    lod: { generating: number; meshing: number } | null;
  } | null;
}

interface View {
  name: string;
  /** Where to be (x, z), and how high above the ground (m); null: the spawn as it is. */
  at: { x: number; z: number; above: number } | null;
  yaw: number;
  pitch: number;
}

const VIEWS: readonly View[] = [
  { name: 'spawn', at: null, yaw: 0, pitch: 0 },
  // Trees and the hill behind them: the forest meets the plains between x = −320 and −288.
  { name: 'forest-edge', at: { x: -250, z: 0, above: 2 }, yaw: -90, pitch: 0 },
  // The shore at x ≈ 64: sea to the east, turquoise shallows to deep water.
  { name: 'coast', at: { x: 40, z: 0, above: 3 }, yaw: 90, pitch: -6 },
  // Mountains at the edge of the fog, from 120 m up.
  { name: 'mountains', at: { x: 0, z: 0, above: 120 }, yaw: -116, pitch: -2 },
  // The whole disc from the flight ceiling (24,000 km).
  { name: 'disc', at: null, yaw: 0, pitch: -89 },
];

const call = <T>(page: Page, expression: string): Promise<T> =>
  page.evaluate<T>(`(() => { const d = globalThis.__dwell; return d ? ${expression} : null; })()`);

const feet = (page: Page): Promise<Vec3> => call<Vec3>(page, 'd.state().feet');

/** Flies toward `target` until within `tolerance` metres (the speed grows with height). */
async function flyTo(page: Page, target: Vec3, tolerance: number, timeoutMs = 120_000) {
  await call(page, 'd.fly(true)');
  const start = Date.now();
  for (;;) {
    const f = await feet(page);
    const d = [target[0] - f[0], target[1] - f[1], target[2] - f[2]];
    const dist = Math.hypot(d[0] ?? 0, d[1] ?? 0, d[2] ?? 0);
    if (dist < tolerance || Date.now() - start > timeoutMs) break;
    const yaw = (Math.atan2(d[0] ?? 0, d[2] ?? 0) * 180) / Math.PI;
    const pitch = (Math.asin((d[1] ?? 0) / dist) * 180) / Math.PI;
    await call(page, `(d.look(${String(yaw)}, ${String(pitch)}), d.press('KeyW', true))`);
    // Close in slowly: the flight coasts.
    await page.waitForTimeout(dist < 40 ? 30 : 120);
  }
  await call(page, "d.press('KeyW', false)");
  await page.waitForTimeout(800);
}

/** The top of the first solid column below `fromY` at (x, z), from the loaded chunks. */
async function groundAt(page: Page, x: number, z: number, fromY: number): Promise<number> {
  return page.evaluate(
    `(() => { const d = globalThis.__dwell; for (let y = ${String(fromY)}; y > -200; y--) { const m = d.voxel(${String(x)}, y, ${String(z)}); if (m !== 0 && m !== 10) return y + 1; } return 0; })()`,
  );
}

/** Waits until the terrain and the distant view have nothing left to generate or mesh. */
async function settle(page: Page, extraMs = 4000) {
  await page.waitForTimeout(500);
  await page
    .waitForFunction(
      () => {
        const s = (globalThis as unknown as { __dwell: Hooks }).__dwell.state();
        return (
          s !== null &&
          s.terrain.generating === 0 &&
          s.terrain.meshing === 0 &&
          s.lod !== null &&
          s.lod.generating === 0 &&
          s.lod.meshing === 0
        );
      },
      null,
      { timeout: 30_000 },
    )
    .catch(() => {
      console.log('  (still loading; shooting anyway)');
    });
  await page.waitForTimeout(extraMs);
}

async function main(): Promise<void> {
  const [base, out, ...only] = process.argv.slice(2);
  if (!base || !out) {
    console.error('usage: shots.ts BASE_URL OUT_DIR [view…]');
    process.exit(2);
  }
  mkdirSync(out, { recursive: true });
  const browser = await chromium.launch({
    ...(process.env.DWELL_CHROMIUM ? { executablePath: process.env.DWELL_CHROMIUM } : {}),
    args: ['--enable-unsafe-swiftshader', '--use-angle=swiftshader', '--no-proxy-server'],
  });
  const page = await browser.newPage({ viewport: { width: WIDTH, height: HEIGHT } });
  page.on('pageerror', (e) => {
    console.log(`page error: ${e.message}`);
  });
  await page.goto(`${base.replace(/\/$/, '')}/dwell/?local=1&seed=${String(SEED)}`);
  await page.waitForFunction(
    () => (globalThis as unknown as { __dwell?: Hooks }).__dwell?.state()?.terrainReady,
    null,
    { timeout: 120_000 },
  );
  // Only the picture: no HUD, menus or overlays.
  // Through the CSSOM: the page's CSP refuses injected style sheets.
  await page.evaluate(
    `(() => { for (const el of document.body.children) if (el.tagName !== 'CANVAS') el.style.display = 'none'; })()`,
  );

  const spawn = await feet(page);
  const views = VIEWS.filter((v) => only.length === 0 || only.includes(v.name));
  for (const view of views) {
    console.log(view.name);
    if (view.name === 'disc') {
      // Straight up at full speed; the speed grows with height.
      await call(
        page,
        "(d.fly(true), d.look(0, -89), d.press('Space', true), d.press('ShiftLeft', true))",
      );
      await page.waitForFunction(
        () =>
          ((globalThis as unknown as { __dwell: Hooks }).__dwell.state()?.feet[1] ?? 0) >
          23_900_000,
        null,
        { timeout: 400_000 },
      );
      await call(page, "(d.press('Space', false), d.press('ShiftLeft', false))");
    } else if (view.at) {
      const { x, z, above } = view.at;
      // Climb to look at the ground there, then settle into place above it.
      await flyTo(page, [x, spawn[1] + 80, z], 20);
      await settle(page, 1500);
      const ground = await groundAt(page, x, z, Math.round(spawn[1] + 120));
      await flyTo(page, [x, ground + above, z], 1.5);
    }
    await call(page, `d.look(${String(view.yaw)}, ${String(view.pitch)})`);
    await settle(page);
    await page.screenshot({ path: join(out, `${view.name}.png`) });
    if (view.name === 'spawn') {
      // Frame time at a standing view, for the ±5 % exit check (software rendering: relative only).
      const ms = await page.evaluate<number>(`new Promise((resolve) => {
      let frames = 0; const t0 = performance.now();
      const tick = () => { frames++; if (performance.now() - t0 > 8000) resolve((performance.now() - t0) / frames); else requestAnimationFrame(tick); };
      requestAnimationFrame(tick);
    })`);
      writeFileSync(join(out, 'frame.json'), `${JSON.stringify({ frameMs: ms }, null, 2)}\n`);
      console.log(`frame time ${ms.toFixed(1)} ms`);
    }
  }

  await browser.close();
}

await main();
