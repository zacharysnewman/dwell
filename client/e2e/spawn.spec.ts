// Spawn overrides (owner, 2026-10-27): a spawn link (`?spawn=b`, `portal`, `portal-b`; Play
// advanced's Spawn) puts the player at its spawn every time it opens the world, not where it left;
// and a new world's Spawn (the New world form) is where it first starts.
import { expect, test, type Page } from '@playwright/test';
import { World } from '../src/protocol/constants.gen';

type Vec3 = [number, number, number];

async function call<T>(page: Page, expr: string): Promise<T | null> {
  try {
    return await page.evaluate<T>(
      `(() => { const d = globalThis.__dwell; return d ? ${expr} : null; })()`,
    );
  } catch (e) {
    if (e instanceof Error && /Execution context was destroyed|navigat/i.test(e.message)) {
      return null;
    }
    throw e;
  }
}
const feet = (page: Page) => call<Vec3>(page, 'd.state().feet');

async function ready(page: Page): Promise<Vec3> {
  await expect
    .poll(async () => (await call<boolean>(page, 'd.state()?.terrainReady')) ?? false, {
      timeout: 60_000,
    })
    .toBe(true);
  const f = await feet(page);
  if (!f) throw new Error('no feet');
  return f;
}

/** Walks forward for `ms`. */
async function walk(page: Page, ms: number): Promise<void> {
  await call(page, "d.press('KeyW', true)");
  await page.waitForTimeout(ms);
  await call(page, "d.press('KeyW', false)");
  await page.waitForTimeout(300);
}

test('a spawn link puts the player back at its spawn when the world is opened again', async ({
  page,
}) => {
  test.setTimeout(120_000);
  // Face B (the flat world has none): under the midplane.
  await page.goto('./?seed=31&spawn=b');
  const start = await ready(page);
  expect(start[1]).toBeLessThan(World.midplaneY);
  await walk(page, 1500);
  const walked = await feet(page);
  if (!walked) throw new Error('no feet');
  expect(Math.hypot(walked[0] - start[0], walked[2] - start[2])).toBeGreaterThan(1);
  // Saved with the player where it walked to (local worlds save every few seconds).
  await page.waitForTimeout(6_000);
  await page.reload();
  const again = await ready(page);
  expect(Math.hypot(again[0] - start[0], again[2] - start[2])).toBeLessThan(0.1);
});

test('a new world starts at the spawn chosen for it', async ({ page }) => {
  test.setTimeout(120_000);
  await page.goto('./');
  await page.locator('#world-name').fill('Under');
  await page.locator('#world-spawn').selectOption('b');
  await page.locator('#create-world button[type=submit]').click();
  await expect(page).toHaveURL(/[?&]play=w[0-9a-z]{10}/);
  const start = await ready(page);
  expect(start[1]).toBeLessThan(World.midplaneY);
});
