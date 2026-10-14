// Terrain and the whole-world view (ARCHITECTURE.md §6.6) in local mode: the chunks and the LOD
// sections are drawn, and the F3 memory report counts what holds them.
import { expect, test, type Page } from '@playwright/test';

interface Stats {
  calls: number;
  triangles: number;
}

async function frame(page: Page): Promise<{ stats: Stats; lodDrawn: number; chunks: number }> {
  return page.evaluate(`(() => {
    const d = globalThis.__dwell;
    const lod = d?.state()?.lod;
    return {
      stats: d?.renderStats(),
      lodDrawn: lod ? lod.drawn.reduce((a, b) => a + b, 0) : 0,
      chunks: lod?.chunkSections ?? 0,
    };
  })()`);
}

test('terrain and LOD drawn, and the memory they hold reported', async ({ page }) => {
  test.setTimeout(120_000);
  await page.goto('./?local=1');
  // The streamed chunks and LOD sections beyond them.
  await expect
    .poll(
      async () => {
        const f = await frame(page);
        return f.chunks > 0 && f.lodDrawn > 20;
      },
      { timeout: 90_000 },
    )
    .toBe(true);
  const { stats } = await frame(page);
  expect(stats.triangles).toBeGreaterThan(0);
  // A draw per chunk and section (no batching, ADR 0028).
  expect(stats.calls).toBeGreaterThan(20);
  // The memory report (F3): the local world's server core, the client's, the workers', the GPU.
  const memory = await page.evaluate<{
    coreBytes: number;
    serverBytes: number | null;
    worldgenBytes: number[];
    meshBytes: number;
    screenBytes: number;
  } | null>('globalThis.__dwell?.memory()');
  expect(memory?.coreBytes).toBeGreaterThan(0);
  expect(memory?.serverBytes).toBeGreaterThan(0);
  expect(memory?.worldgenBytes.length).toBeGreaterThan(0);
  expect(memory?.worldgenBytes.every((b) => b > 0)).toBe(true);
  expect(memory?.meshBytes).toBeGreaterThan(0);
  expect(memory?.screenBytes).toBeGreaterThan(0);
});
