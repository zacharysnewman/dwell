// The main menu's background and Settings from the main menu (ui/menuBackground.ts, ui/menuPage.ts):
// the panorama of the chosen spot turns behind the menu (held still with reduced motion); the
// Settings button opens every pane, the world-only ones listed with a note; choosing another
// background in Settings shows it and keeps it after a reload.
import { expect, test, type Page } from '@playwright/test';

const SHOTS = process.env.DWELL_MENU_SHOTS;

async function strip(page: Page): Promise<{ name: string; image: string; duration: string }> {
  const panorama = page.locator('.mm-panorama.loaded');
  await expect(panorama).toBeVisible({ timeout: 15_000 });
  return page.evaluate(() => {
    const p = document.querySelector<HTMLElement>('.mm-panorama');
    const s = document.querySelector<HTMLElement>('.mm-pano-strip');
    if (!p || !s) throw new Error('no panorama');
    return {
      name: p.dataset.background ?? '',
      image: s.style.backgroundImage,
      duration: getComputedStyle(s).animationName,
    };
  });
}

test('main menu: a turning panorama, and every setting from the Settings button', async ({
  page,
}) => {
  await page.goto('./');
  const shown = await strip(page);
  expect(shown.name).toBe('beach-peaks');
  expect(shown.image).toMatch(/beach-peaks.*\.webp/);
  expect(shown.duration).toBe('mm-pano-turn');
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/desktop-beach-peaks.png` });

  await page.locator('#menu-settings').click();
  const menu = page.locator('#settings-menu');
  await expect(menu).toBeVisible();
  await expect(menu.locator('.gm-title')).toHaveText('Settings');
  await expect(menu.locator('.gm-tab')).toHaveText([
    'Graphics',
    'Sound',
    'Gameplay',
    'On screen',
    'Controller',
    'Controls',
    'Structures',
    'Debug',
    'Host',
  ]);
  for (const page_ of ['structures', 'debug', 'host']) {
    await expect(menu.locator(`.gm-tab[data-page="${page_}"]`)).toHaveClass(/gm-tab-off/);
  }
  await menu.locator('.gm-tab[data-page="host"]').click();
  await expect(menu.locator('.gm-page[data-page="host"]')).toContainText('Open one of your worlds');
  await menu.locator('.gm-tab[data-page="screen"]').click();
  const choices = menu.locator('.settings-choices[aria-label="Menu background"] button');
  await expect(choices).toHaveText(['Beach and peaks', 'River valley', 'Lake shore']);
  await choices.filter({ hasText: 'Lake shore' }).click();
  await page.keyboard.press('Escape');
  await expect(menu).toBeHidden();
  await expect.poll(async () => (await strip(page)).name).toBe('lake-shore');

  await page.reload();
  expect((await strip(page)).name).toBe('lake-shore');
});

test('main menu: the panorama holds still with reduced motion', async ({ browser }) => {
  const context = await browser.newContext({ reducedMotion: 'reduce' });
  const page = await context.newPage();
  await page.goto('./');
  expect((await strip(page)).duration).toBe('none');
  await context.close();
});

test('main menu backgrounds: screenshots of each', async ({ browser }) => {
  test.skip(!SHOTS, 'DWELL_MENU_SHOTS names a directory for the screenshots');
  test.setTimeout(90_000);
  for (const name of ['beach-peaks', 'river-bank', 'lake-shore']) {
    for (const [kind, options] of [
      ['desktop', { viewport: { width: 1440, height: 900 } }],
      [
        'phone',
        {
          viewport: { width: 390, height: 844 },
          deviceScaleFactor: 3,
          isMobile: true,
          hasTouch: true,
        },
      ],
    ] as const) {
      const context = await browser.newContext(options);
      await context.addInitScript((n) => {
        localStorage.setItem('dwell.menu', JSON.stringify({ background: n }));
      }, name);
      const page = await context.newPage();
      await page.goto('./');
      expect((await strip(page)).name).toBe(name);
      await page.waitForTimeout(1000);
      await page.screenshot({ path: `${SHOTS ?? ''}/${kind}-${name}.png` });
      await context.close();
    }
  }
});
