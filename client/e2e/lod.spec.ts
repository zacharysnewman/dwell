// Phase 4c: the whole-world view (ARCHITECTURE.md §6.6) in local mode — the streamed chunks stand
// in for level 0 around the player, who then flies (creative flight) high enough to see the whole
// disc drawn.
import { expect, test, type Page } from '@playwright/test';

interface LodStats {
  drawn: number[];
  chunkSections: number;
  generating: number;
  meshing: number;
  cacheBytes: number;
}
interface Hooks {
  state(): { terrainReady: boolean; lod: LodStats | null; feet: number[]; flying: boolean } | null;
  look(yaw: number, pitch: number): void;
  press(code: string, down: boolean): void;
  fly(on: boolean): void;
}
const hooks = () => (globalThis as unknown as { __dwell?: Hooks }).__dwell;

async function lod(page: Page): Promise<LodStats | null> {
  return page.evaluate(`(${hooks.toString()})()?.state()?.lod ?? null`);
}

async function call(page: Page, expression: string): Promise<unknown> {
  return page.evaluate(`(${hooks.toString()})()?.${expression}`);
}

test('local mode: LOD around the player, and the whole disc after flying up', async ({ page }) => {
  test.setTimeout(240_000);
  await page.goto('./?local=1');
  await expect
    .poll(async () => (await lod(page))?.chunkSections ?? 0, { timeout: 60_000 })
    .toBeGreaterThan(0);
  const ground = await lod(page);
  // LOD sections beyond the streamed chunks.
  expect(ground?.drawn.reduce((a, b) => a + b, 0) ?? 0).toBeGreaterThan(0);

  // Fly up (running doubles the climb) to the ceiling, 24,000 km, looking straight down: the disc
  // (radius 8,192 km) fills ~2/3 of the view.
  await call(page, 'fly(true)');
  await call(page, 'look(0, -89)');
  await call(page, "press('Space', true)");
  await call(page, "press('ShiftLeft', true)");
  await expect
    .poll(async () => ((await call(page, 'state()?.flying')) as boolean | undefined) ?? false)
    .toBe(true);
  const climb = Date.now();
  await expect
    .poll(async () => ((await call(page, 'state()?.feet[1]')) as number | undefined) ?? 0, {
      timeout: 150_000,
    })
    .toBeGreaterThan(23_900_000);
  console.log(`flew to the ceiling in ${((Date.now() - climb) / 1000).toFixed(1)} s`);
  const start = Date.now();
  await expect
    .poll(
      async () => {
        const s = await lod(page);
        if (!s) return false;
        // Only coarse levels, and together they cover at least the disc's area (π · 8,192 km²).
        const fine = s.drawn.slice(0, 12).reduce((a, b) => a + b, 0);
        const area = s.drawn.reduce((sum, n, level) => sum + n * (32 * 2 ** level) ** 2, 0);
        const disc = Math.PI * 8_192_000 ** 2;
        return fine === 0 && area >= disc && s.generating === 0 && s.meshing === 0;
      },
      { timeout: 30_000 },
    )
    .toBe(true);
  const seconds = (Date.now() - start) / 1000;
  console.log(`whole disc drawn ${seconds.toFixed(1)} s after reaching altitude`);
  expect(seconds).toBeLessThan(30);
  const high = await lod(page);
  expect(high?.cacheBytes ?? 0).toBeLessThan(256 * 1048576); // LOD_CACHE_MB (desktop)
});
