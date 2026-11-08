// The settings menu (ui/settingsMenu.ts, local mode): the top-left button opens the menu, clear of
// the connection status, on the Graphics pane (the Preset slider first; the fog is fixed look data,
// not a setting); the panes switch from the list; a moved slider applies and is kept after a
// reload; the Preset slider applies a preset and a changed setting reads Custom; the settings copy
// to the clipboard as JSON.
import { HUD_ELEMENTS } from '../src/ui/hud.gen';
import { expect, test, type Locator } from '@playwright/test';
import { untilTicking } from './ticking';

/** A settings row by its name, in the pane shown. */
function row(menu: Locator, name: string): Locator {
  return menu
    .locator('.gm-page:not([hidden]) .settings-row')
    .filter({ has: menu.page().locator('.settings-name', { hasText: new RegExp(`^${name}$`) }) });
}

test('settings menu: opens from the corner, in panes, without fog sliders, and is kept', async ({
  page,
}) => {
  test.setTimeout(90_000);
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
  // The panes, then the other pages.
  await expect(menu.locator('.gm-tab').first()).toHaveText('Graphics');
  await expect(menu.locator('.gm-tab').nth(1)).toHaveText('Sound');
  await expect(menu.locator('.gm-tab').nth(2)).toHaveText('Gameplay');
  await expect(menu.locator('.gm-tab').nth(3)).toHaveText('On screen');
  // Graphics opens first, at the desktop default's 50 % Stretch; the suite stores the clouds Off
  // (playwright.config.ts), so the preset reads Custom until Reset.
  await expect(menu.locator('#graphics-preset .settings-value')).toHaveText('Custom');
  await expect(row(menu, 'Render scale').locator('[aria-pressed=true]')).toHaveText('50 %');
  await expect(row(menu, 'Full detail').locator('.settings-value')).toHaveText('256 m');
  await expect(row(menu, 'Distant memory').locator('.settings-value')).toHaveText('256 MB');
  await expect(menu).not.toContainText('Fog');
  await expect(menu).not.toContainText('Density');

  // The preset slider to Ultra (its last stop) sets 100 %; one setting changed reads Custom.
  await menu.locator('#graphics-preset input[type=range]').fill('4');
  await expect(menu.locator('#graphics-preset .settings-value')).toHaveText('Ultra');
  await expect(row(menu, 'Render scale').locator('[aria-pressed=true]')).toHaveText('100 %');
  await row(menu, 'Clouds').getByRole('button', { name: 'Low' }).click();
  await expect(menu.locator('#graphics-preset .settings-value')).toHaveText('Custom');

  // The distant view's memory to the most.
  await row(menu, 'Distant memory').locator('input[type=range]').fill('1000');
  await expect(row(menu, 'Distant memory').locator('.settings-value')).toHaveText('1024 MB');
  // Sound: the four volumes.
  await menu.locator('.gm-tab', { hasText: 'Sound' }).click();
  await expect(menu.locator('.gm-page:not([hidden]) .settings-value')).toHaveText([
    '80%',
    '100%',
    '60%',
    '60%',
  ]);
  // Gameplay: the field of view to its narrowest, as a drag would.
  await menu.locator('.gm-tab', { hasText: 'Gameplay' }).click();
  await row(menu, 'Field of view').locator('input[type=range]').fill('0');
  await expect(row(menu, 'Field of view').locator('.settings-value')).toHaveText('60°');

  await page.reload();
  await untilTicking(page);
  await page.locator('#menu-button').click();
  await expect(menu.locator('#graphics-preset .settings-value')).toHaveText('Custom');
  await expect(row(menu, 'Distant memory').locator('.settings-value')).toHaveText('1024 MB');
  await menu.locator('.gm-tab', { hasText: 'Gameplay' }).click();
  await expect(row(menu, 'Field of view').locator('.settings-value')).toHaveText('60°');
  await menu.locator('.settings-reset[title="Back to the defaults"]').click();
  await expect(row(menu, 'Field of view').locator('.settings-value')).toHaveText('75°');
  await menu.locator('.gm-tab', { hasText: 'Graphics' }).click();
  await expect(menu.locator('#graphics-preset .settings-value')).toHaveText('High');
  await expect(row(menu, 'Distant memory').locator('.settings-value')).toHaveText('256 MB');

  // Copy JSON puts the settings on the clipboard, to paste elsewhere.
  await page.context().grantPermissions(['clipboard-read', 'clipboard-write']);
  await menu.locator('.settings-copy').click();
  await expect(menu.locator('.settings-copy')).toHaveText('Copied');
  const copied = await page.evaluate<string>('navigator.clipboard.readText()');
  expect(JSON.parse(copied)).toEqual({
    detail: { distanceM: 256, pixelError: 3, memoryMb: 256 },
    exposure: 1,
    audio: { master: 0.8, effects: 1, ui: 0.6, music: 0.6 },
    clouds: 'high',
    shafts: 'high',
    water: {
      quality: 'medium',
      flowRipples: true,
      flowFoam: true,
      flowParticles: true,
      waterfalls: true,
    },
    shadows: { quality: 'high', torch: 'low' },
    motion: { speedEffects: true, fovBoost: 1, plantSway: true },
    explosions: { flash: true, smoke: true, shake: true },
    fovDeg: 75,
    showArms: true,
    // Every part of the screen shows by default (shared/ui/hud.json).
    hud: {
      shown: Object.fromEntries(HUD_ELEMENTS.map((e) => [e.id, e.shown])),
      mapShading: true,
    },
    renderScale: { scale: 0.5, upscaler: 'stretch' },
  });
});
