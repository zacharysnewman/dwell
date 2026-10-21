// The settings menu (ui/settingsMenu.ts, local mode): the top-left button opens the sliders (sound,
// detail, look; the fog is fixed look data, not a setting), clear of the connection status; a moved
// slider applies and is kept after a reload, and the settings copy to the clipboard as JSON.
import { expect, test } from '@playwright/test';

test('settings menu: opens from the corner, without fog sliders, and is kept', async ({ page }) => {
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
  // Sound's four volumes first, then the detail's three (no fog sliders: the fog is fixed).
  await expect(menu.locator('.settings-value')).toHaveText([
    '80%',
    '100%',
    '60%',
    '60%',
    '256 m',
    '4.0 px',
    '256 MB',
    '75°',
    '1.00×',
    // Clouds, Sun & moon shadows and Torch shadows, then (after the FOV boost) the water
    // quality: rows of buttons, the pressed one the value.
    '',
    '',
    '',
    '100%',
    '',
  ]);

  await expect(menu).not.toContainText('Fog');
  await expect(menu).not.toContainText('Density');

  // The field of view to its narrowest, as a drag would.
  await menu.locator('input[type=range]').nth(7).fill('0');
  await expect(menu.locator('.settings-value').nth(7)).toHaveText('60°');
  // The distant view's memory to the most.
  await menu.locator('input[type=range]').nth(6).fill('1000');
  await expect(menu.locator('.settings-value').nth(6)).toHaveText('1024 MB');
  await page.reload();
  await page.locator('#menu-button').click();
  await expect(page.locator('#settings-menu .settings-value').nth(7)).toHaveText('60°');
  await expect(page.locator('#settings-menu .settings-value').nth(6)).toHaveText('1024 MB');
  await page.locator('#settings-menu .settings-reset').click();
  await expect(page.locator('#settings-menu .settings-value').nth(7)).toHaveText('75°');
  await expect(page.locator('#settings-menu .settings-value').nth(6)).toHaveText('256 MB');

  // Copy JSON puts the settings on the clipboard, to paste elsewhere.
  await page.context().grantPermissions(['clipboard-read', 'clipboard-write']);
  await page.locator('#settings-menu .settings-copy').click();
  await expect(page.locator('#settings-menu .settings-copy')).toHaveText('Copied');
  const copied = await page.evaluate<string>('navigator.clipboard.readText()');
  expect(JSON.parse(copied)).toEqual({
    detail: { distanceM: 256, pixelError: 4, memoryMb: 256 },
    exposure: 1,
    audio: { master: 0.8, effects: 1, ui: 0.6, music: 0.6 },
    clouds: 'high',
    water: { quality: 'medium' },
    shadows: { quality: 'high', torch: 'low' },
    motion: { speedEffects: true, fovBoost: 1, plantSway: true },
    fovDeg: 75,
    showArms: true,
  });
});
