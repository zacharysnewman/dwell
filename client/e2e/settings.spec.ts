// The settings menu (ui/settingsMenu.ts, local mode): the top-left button opens the fog sliders,
// clear of the connection status, and a moved slider applies and is kept after a reload.
import { expect, test } from '@playwright/test';

test('settings menu: fog sliders open from the corner and are kept', async ({ page }) => {
  await page.goto('./?world=flat');
  await expect(page.locator('#net-status')).toContainText('player', { timeout: 20_000 });
  const button = page.locator('#menu-button');
  const status = await page.locator('#net-status').boundingBox();
  const corner = await button.boundingBox();
  if (!status || !corner) throw new Error('no status or menu button');
  expect(corner.x + corner.width).toBeLessThanOrEqual(status.x);
  expect(corner.x).toBeLessThan(20);
  expect(corner.y).toBeLessThan(20);

  const menu = page.locator('#settings-menu');
  await expect(menu).toBeHidden();
  await button.click();
  await expect(menu).toBeVisible();
  await expect(menu.locator('.settings-value')).toHaveText(['100 km', '60%', '1.5 km']);

  // Density to zero (no fog), as a drag would.
  await menu.locator('input').nth(1).fill('0');
  await expect(menu.locator('.settings-value').nth(1)).toHaveText('0%');
  await page.reload();
  await page.locator('#menu-button').click();
  await expect(page.locator('#settings-menu .settings-value').nth(1)).toHaveText('0%');
  await page.locator('#settings-menu .settings-reset').click();
  await expect(page.locator('#settings-menu .settings-value').nth(1)).toHaveText('60%');
});
