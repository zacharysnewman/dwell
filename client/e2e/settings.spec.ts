// The settings menu (ui/settingsMenu.ts, local mode): the top-left button opens the sliders (fog,
// detail), clear of the connection status; a moved slider applies and is kept after a reload, and the
// settings copy to the clipboard as JSON.
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
  await expect(menu.locator('.settings-value')).toHaveText([
    '4.0 km',
    '50%',
    '1.5 km',
    '256 m',
    '4.0 px',
    '256 MB',
    '1.00×',
  ]);

  // Density to zero (no fog), as a drag would.
  await menu.locator('input').nth(1).fill('0');
  await expect(menu.locator('.settings-value').nth(1)).toHaveText('0%');
  // The distant view's memory to the most.
  await menu.locator('input').nth(5).fill('1000');
  await expect(menu.locator('.settings-value').nth(5)).toHaveText('1024 MB');
  await page.reload();
  await page.locator('#menu-button').click();
  await expect(page.locator('#settings-menu .settings-value').nth(1)).toHaveText('0%');
  await expect(page.locator('#settings-menu .settings-value').nth(5)).toHaveText('1024 MB');
  await page.locator('#settings-menu .settings-reset').click();
  await expect(page.locator('#settings-menu .settings-value').nth(1)).toHaveText('50%');
  await expect(page.locator('#settings-menu .settings-value').nth(5)).toHaveText('256 MB');

  // Copy JSON puts the settings on the clipboard, to paste elsewhere.
  await page.context().grantPermissions(['clipboard-read', 'clipboard-write']);
  await page.locator('#settings-menu .settings-copy').click();
  await expect(page.locator('#settings-menu .settings-copy')).toHaveText('Copied');
  const copied = await page.evaluate<string>('navigator.clipboard.readText()');
  expect(JSON.parse(copied)).toEqual({
    fog: { distanceM: 4000, density: 0.5, heightM: 1500 },
    detail: { distanceM: 256, pixelError: 4, memoryMb: 256 },
    exposure: 1,
  });
});
