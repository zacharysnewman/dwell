// Debug tooling: `?at=`, `?look=` and `?fly=1` start a local world where a screenshot needs it
// (the local server moves the player: dwell_local_teleport), and `__dwell.settled()` reports when
// the view has finished loading (scripts/shot.ts waits for it).
import { expect, test, type Page } from '@playwright/test';

const call = <T>(page: Page, expr: string): Promise<T> =>
  page.evaluate<T>(`(() => { const d = globalThis.__dwell; return d ? ${expr} : null; })()`);

test('a local world starts where ?at= and ?look= say, flying', async ({ page }) => {
  test.setTimeout(120_000);
  // 2 km from the spawn: far beyond the streamed view.
  // Without the LOD (which keeps refining for minutes in software rendering), the streamed
  // chunks alone decide when the view has settled.
  await page.goto('./?world=flat&lod=0&at=2000.5,12,-1500.5&look=135,-12&fly=1');
  // Within a few cm: the body settles as flight takes over.
  const at = [2000.5, 12, -1500.5];
  await expect
    .poll(
      async () => {
        const f = await call<number[] | null>(page, 'd.state()?.feet ?? null');
        return f ? Math.hypot(...f.map((v, i) => v - (at[i] ?? 0))) : Infinity;
      },
      { timeout: 30_000 },
    )
    .toBeLessThan(0.25);
  expect(await call<{ yaw: number; pitch: number }>(page, 'd.view()')).toEqual({
    yaw: 135,
    pitch: -12,
  });
  expect(await call<boolean>(page, 'd.state()?.flying ?? false')).toBe(true);
  // The terrain there arrives, and the view settles.
  await expect
    .poll(() => call<boolean>(page, 'd.state()?.terrainReady ?? false'), { timeout: 30_000 })
    .toBe(true);
  await expect.poll(() => call<boolean>(page, 'd.settled()'), { timeout: 60_000 }).toBe(true);
});
