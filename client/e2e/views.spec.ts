// The views tool (scripts/views.ts, e2e/views.ts): a named view places the camera high above the
// spawn, the distant view loads around it and settles, and the player stays on the ground.
import { expect, test } from '@playwright/test';
import { openView, view } from './views.ts';

test('a named view places the camera and the LOD settles around it', async ({ page }) => {
  test.setTimeout(180_000);
  const v = view('e2e-placed');
  const result = await openView(page, v);
  expect(
    result.settled,
    `LOD still loading after ${String(v.settleS)} s: ${JSON.stringify(result.lod)}`,
  ).toBe(true);
  expect(result.lod?.drawn.reduce((a, b) => a + b, 0) ?? 0).toBeGreaterThan(20);
  expect(result.render.calls).toBeGreaterThan(10);
  // The camera is 300 m up; the player is still standing at the spawn.
  expect(result.feet[1]).toBeLessThan(200);
  // ?debug=1 opens the F3 overlay from the start (it was lost before the game existed).
  await expect(page.locator('#debug-overlay')).toContainText('LOD ');
  expect(await page.screenshot()).toBeTruthy();
});
