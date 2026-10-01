// Batched terrain (?batch=1, ARCHITECTURE.md §6.6) in local mode: the chunks and the LOD sections
// draw in a handful of draw calls, where separate meshes take one each.
import { expect, test, type Page } from '@playwright/test';

interface Stats {
  calls: number;
  triangles: number;
  batched: boolean;
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

for (const batched of [false, true]) {
  test(`terrain and LOD drawn ${batched ? 'in batches' : 'as meshes'}`, async ({ page }) => {
    test.setTimeout(120_000);
    await page.goto(batched ? './?local=1&batch=1' : './?local=1');
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
    expect(stats.batched).toBe(batched);
    expect(stats.triangles).toBeGreaterThan(0);
    // Batched: a draw call per batch and pass (chunks, chunk water, LOD, LOD water, the outline).
    if (batched) expect(stats.calls).toBeLessThanOrEqual(12);
    else expect(stats.calls).toBeGreaterThan(20);
  });
}
