// The GPU ray marcher against the CPU reference, per pixel (Phase 25 A3b,
// docs/RAY_MARCHED_RENDERER.md §5 "Correctness under both"): crafted scenes (chunks, LOD sections,
// both world faces, 4,000 km out) rendered offscreen at 64² on headless SwiftShader, every ray
// marched again in Rust with the GPU's own direction. Ids exact, depth within 1e-4 relative.
import { expect, test } from '@playwright/test';

interface MarchReport {
  error?: string;
  adapter?: string;
  views?: number;
  pixels?: number;
  hits?: number;
  idMismatches?: number;
  recordMismatches?: number;
  tUlpsMax?: number;
  maxDepthError?: number;
  maxDirError?: number;
  levelsHit?: number;
  passes?: boolean;
  lines?: string[];
  examples?: string[];
}

test('the GPU marcher matches the CPU reference pixel for pixel', async ({ page }) => {
  test.setTimeout(120_000);
  await page.goto('./');
  const result = (await (
    await page.waitForFunction(() => (window as { __probe?: unknown }).__probe, null, {
      timeout: 110_000,
    })
  ).jsonValue()) as { march?: MarchReport; error?: string };
  const m = result.march ?? {};
  console.log(JSON.stringify({ ...m, lines: undefined }, null, 1));
  for (const line of m.lines ?? []) console.log(line);
  expect(m.error).toBeUndefined();
  // 12 views, each after three last frames: a turn, a step forward, a step back (A4c).
  expect(m.views).toBe(36);
  expect(m.idMismatches).toBe(0);
  expect(m.recordMismatches).toBe(0);
  expect(m.maxDepthError ?? 1).toBeLessThanOrEqual(1e-4);
  // Chunks and three LOD levels were hit.
  expect((m.levelsHit ?? 0) & 0b1111).toBe(0b1111);
  expect(m.passes).toBe(true);
});
