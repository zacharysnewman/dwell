// The flight speed slider (PLAYER_CONTROLLER.md §6.7): shown while flying; raising it flies faster
// (applied by the local server and predicted alike); − steps it down; hidden when flight ends.
import { expect, test, type Page } from '@playwright/test';

type Vec3 = [number, number, number];

function call<T>(page: Page, expr: string): Promise<T> {
  return page.evaluate<T>(`(() => { const d = globalThis.__dwell; return d ? ${expr} : null; })()`);
}
const feet = (page: Page) => call<Vec3>(page, 'd.state().feet');

/** Metres flown forward (+Z) in `ms` at the current speed level. */
async function flyForward(page: Page, ms: number): Promise<number> {
  const start = await feet(page);
  await call(page, "d.press('KeyW', true)");
  await page.waitForTimeout(ms);
  await call(page, "d.press('KeyW', false)");
  const end = await feet(page);
  // Let the flight's drag bring the player to rest before the next measurement.
  await page.waitForTimeout(500);
  return Math.hypot(end[0] - start[0], end[2] - start[2]);
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
  const control = page.locator('#flight-speed');
  await expect(control).toBeHidden();

  await call(page, 'd.fly(true)');
  await expect(control).toBeVisible();
  await expect(control.locator('.flight-speed-value')).toHaveText('Normal (faster with height)');
  const normal = await flyForward(page, 1000);

  await control.locator('input').fill('20'); // at least 11 m/s × 2^10 (capped at 400 m/s here)
  await expect(control.locator('.flight-speed-value')).toHaveText('At least 11 km/s');
  const fast = await flyForward(page, 1000);
  expect(normal).toBeLessThan(40);
  expect(fast).toBeGreaterThan(5 * normal);

  // The − key steps it down; the level is kept for the next visit.
  await page.keyboard.press('Minus');
  await expect(control.locator('.flight-speed-value')).toHaveText('At least 8.0 km/s');
  expect(await page.evaluate(() => localStorage.getItem('dwell.flySpeed'))).toBe('19');

  await call(page, 'd.fly(false)');
  await expect(control).toBeHidden();
});
