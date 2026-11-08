// The flight speed slider (PLAYER_CONTROLLER.md §6.7): shown while flying; raising it flies faster
// (applied by the local server and predicted alike); − steps it down; hidden when flight ends.
import { expect, test, type Page } from '@playwright/test';
import { untilTicking } from './ticking';

type Vec3 = [number, number, number];

function call<T>(page: Page, expr: string): Promise<T> {
  return page.evaluate<T>(`(() => { const d = globalThis.__dwell; return d ? ${expr} : null; })()`);
}
const feet = (page: Page) => call<Vec3>(page, 'd.state().feet');

const ticks = (page: Page) => call<number>(page, 'd.state().stats.ticks');

/**
 * Metres climbed in `n` controller ticks (60 a second) at the current speed level (Space). Up into
 * the air, not along the ground: at the top level a second's flight crosses 362 m, past the
 * terrain streamed round the player, and on CI's slow runners the player is held at its edge (0 m
 * flown, 0.38.0's main). Held for a count of ticks, not of wall-clock time: on the software
 * renderer the page can draw no frame (and tick nothing) for longer than a second (BUGS.md #49),
 * and a timed hold then flew 0 m (0.50.30's main).
 */
async function flyUp(page: Page, n: number): Promise<number> {
  const start = await feet(page);
  const t0 = await ticks(page);
  await call(page, "d.press('Space', true)");
  await expect.poll(async () => (await ticks(page)) - t0, { timeout: 20_000 }).toBeGreaterThan(n);
  await call(page, "d.press('Space', false)");
  const end = await feet(page);
  // Let the flight's drag bring the player to rest before the next measurement.
  await page.waitForTimeout(500);
  return end[1] - start[1];
}

test('flight speed: the slider shows while flying and raises the speed', async ({ page }) => {
  test.setTimeout(60_000);
  await page.goto('./?world=flat');
  await expect
    .poll(
      async () => (await call<{ active: boolean } | null>(page, 'd.state()'))?.active ?? false,
      {
        timeout: 20_000,
      },
    )
    .toBe(true);
  await untilTicking(page);
  const control = page.locator('#flight-speed');
  await expect(control).toBeHidden();

  await call(page, 'd.fly(true)');
  await expect(control).toBeVisible();
  await expect(control.locator('.flight-speed-value')).toHaveText('8 m/s');
  const normal = await flyUp(page, 60);

  await control.locator('input').fill('11'); // the top level: 8 m/s × 2^5.5
  await expect(control.locator('.flight-speed-value')).toHaveText('362 m/s');
  const fast = await flyUp(page, 60);
  expect(normal).toBeLessThan(40);
  expect(fast).toBeGreaterThan(5 * normal);

  // The − key steps it down; the level is kept for the next visit.
  await page.keyboard.press('Minus');
  await expect(control.locator('.flight-speed-value')).toHaveText('256 m/s');
  expect(await page.evaluate(() => localStorage.getItem('dwell.flySpeed'))).toBe('10');

  await call(page, 'd.fly(false)');
  await expect(control).toBeHidden();
});
