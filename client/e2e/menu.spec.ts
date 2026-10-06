// Phase 5a: the main menu and world management (ARCHITECTURE.md §2.1) — create a world with a seed,
// play it, quit to the menu (saving), reopen it, regenerate it and delete it; and a world saved
// before versioned releases is ignored and can be deleted.
import { expect, test, type Page } from '@playwright/test';

type Vec3 = [number, number, number];
interface DebugState {
  terrainReady: boolean;
  terrain: { loaded: number; generating: number };
  target: { cell: Vec3; face: number } | null;
}

/**
 * Runs a call on the page's test hooks (window.__dwell, see src/main.ts); null while the page is
 * navigating (the menu opens and leaves the game by navigation, and polls run across it).
 */
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
const state = (page: Page) => call<DebugState | null>(page, 'd.state()');
const voxel = (page: Page, c: Vec3) => call<number>(page, `d.voxel(${c.join(',')})`);

async function ready(page: Page): Promise<void> {
  await expect
    .poll(async () => (await state(page))?.terrainReady ?? false, { timeout: 20_000 })
    .toBe(true);
  await expect
    .poll(
      async () => {
        const t = (await state(page))?.terrain;
        return t && t.generating === 0 ? t.loaded : 0;
      },
      { timeout: 30_000 },
    )
    .toBeGreaterThan(100);
}

/** The block under the crosshair, looking down ahead. */
async function aim(page: Page): Promise<Vec3> {
  await call(page, 'd.look(0, -45)');
  await expect.poll(async () => (await state(page))?.target ?? null).not.toBeNull();
  const t = (await state(page))?.target;
  if (!t) throw new Error('nothing targeted');
  return t.cell;
}

/** Quits through the ☰ game menu and waits for the main menu. */
async function quit(page: Page): Promise<void> {
  await page.locator('#menu-button').click();
  await page.locator('#menu-quit').click();
  await expect(page.locator('#main-menu')).toBeVisible({ timeout: 10_000 });
  expect(new URL(page.url()).searchParams.has('play')).toBe(false);
}

const world = (page: Page, name: string) => page.locator('.world-item', { hasText: name });

test('main menu: create a world with a seed, quit, reopen, regenerate and delete it', async ({
  page,
}) => {
  test.setTimeout(120_000);
  await page.goto('./');
  await expect(page.locator('#main-menu')).toBeVisible();
  await expect(page.locator('.menu-empty')).toBeVisible();

  // Create: the form is open when there are no worlds.
  await page.locator('#world-name').fill('Test world');
  await page.locator('#world-seed').fill('1234');
  await page.locator('#world-type').selectOption('flat');
  await page.locator('#create-world button[type=submit]').click();
  await expect(page).toHaveURL(/[?&]play=w[0-9a-z]{10}/);
  await expect(page.locator('#net-status')).toContainText('Test world');
  await ready(page);

  // An edit, saved by quitting (no waiting for the autosave).
  const cell = await aim(page);
  const before = await voxel(page, cell);
  expect(before).not.toBe(0);
  expect(await call<boolean>(page, `d.edit('break')`)).toBe(true);
  await expect.poll(() => voxel(page, cell), { timeout: 5_000 }).toBe(0);
  await quit(page);
  await expect(world(page, 'Test world')).toContainText('Flat · seed 1234 · played');

  // Reopen: the edit is there.
  await world(page, 'Test world').locator('.world-play').click();
  await ready(page);
  await expect.poll(() => voxel(page, cell), { timeout: 10_000 }).toBe(0);
  await quit(page);

  // Regenerate (asks first): same seed, the edit is gone.
  const regenerate = world(page, 'Test world').getByRole('button', { name: 'Regenerate' });
  await regenerate.click();
  await world(page, 'Test world').getByRole('button', { name: 'Lose all changes?' }).click();
  await ready(page);
  expect(await voxel(page, cell)).toBe(before);
  await quit(page);

  // Delete (asks first).
  await world(page, 'Test world').getByRole('button', { name: 'Delete' }).click();
  await world(page, 'Test world').getByRole('button', { name: 'Delete forever?' }).click();
  await expect(page.locator('#menu-message')).toHaveText('Deleted "Test world".');
  await expect(world(page, 'Test world')).toHaveCount(0);
  await expect(page.locator('.menu-empty')).toBeVisible();
});

test('main menu: a world file without a versioned entry is ignored, and can be deleted', async ({
  page,
}) => {
  test.setTimeout(90_000);
  // A world opened by link is saved per generator and seed.
  await page.goto('./?world=flat&seed=77');
  await ready(page);
  await page.waitForTimeout(6_000); // the autosave: the file exists
  // Forget the list, as a browser from before versioned releases (RELEASES.md §6) has none that
  // records an app version for it.
  await page.evaluate(() => {
    localStorage.removeItem('dwell.worlds');
  });

  await page.goto('./');
  // Not a world to play: listed apart, to delete.
  await expect(page.locator('.menu-empty')).toBeVisible();
  const old = page.locator('#legacy-worlds .world-item', { hasText: 'Flat world 77' });
  await expect(old).toBeVisible();
  await expect(old.locator('.world-play')).toHaveCount(0);
  await old.getByRole('button', { name: 'Delete' }).click();
  await old.getByRole('button', { name: 'Delete forever?' }).click();
  await expect(page.locator('#legacy-worlds')).toBeHidden();
  await expect(page.locator('#menu-message')).toHaveText('Deleted "Flat world 77".');
});

test('main menu: shows the app version, and the licence notices from About', async ({
  page,
  request,
}) => {
  await page.goto('./');
  await expect(page.locator('#app-version')).toContainText(/Dwell \d+\.\d+\.\d+/);
  await page.getByRole('button', { name: 'About' }).click();
  const notices = page.getByRole('link', { name: 'Third-party notices' });
  await expect(notices).toBeVisible();
  const href = await notices.getAttribute('href');
  const response = await request.get(new URL(href ?? '', page.url()).href);
  expect(response.ok()).toBe(true);
  expect(await response.text()).toContain('THIRD-PARTY NOTICES');
});

test('main menu: a world from before versioned releases is listed to delete, never played', async ({
  page,
}) => {
  await page.goto('./');
  await page.evaluate(() => {
    localStorage.setItem(
      'dwell.worlds',
      JSON.stringify([
        {
          id: 'wold0000000',
          name: 'Ancient',
          type: 'terrain',
          seed: 5,
          generatorVersion: 4,
          createdAt: 1,
          lastPlayedAt: 1,
        },
      ]),
    );
  });
  await page.reload();
  await expect(page.locator('#legacy-worlds')).toContainText('Ancient');
  await expect(page.locator('#legacy-worlds .world-play')).toHaveCount(0);
  // Opening it by link is refused with the reason.
  await page.goto('./?play=wold0000000');
  await expect(page.locator('#main-menu')).toBeVisible();
  await expect(page.locator('#menu-message')).toContainText('before versioned releases');
});

test('main menu: joining asks for a join code, an address or an invite link', async ({ page }) => {
  await page.goto('./?debug=1');
  await page.locator('#join-input').fill('not a link');
  await page.locator('.menu-join-form button').click();
  await expect(page.locator('#menu-message')).toContainText(
    'not a join code, a server address or an invite link',
  );
  await expect(page).toHaveURL(/\?debug=1$/);
});
