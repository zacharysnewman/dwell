// The launcher (RELEASES.md §5, §9) against a locally assembled site with 0.1.0, 0.1.1 and 0.2.0:
// which build each address opens, that worlds open in their own line, and the menu's version
// badges. Run with `npm run e2e:site` (playwright.site.config.ts).
import { expect, test, type Page } from '@playwright/test';

const CERT = 'ab'.repeat(32);

function world(id: string, name: string, appVersion?: string) {
  return {
    id,
    name,
    type: 'flat',
    seed: 1,
    generatorVersion: 0,
    createdAt: 1,
    lastPlayedAt: 1,
    ...(appVersion ? { appVersion } : {}),
  };
}

/** Opens `/dwell/`: the launcher's main menu (RELEASES.md §5). */
async function openLauncher(page: Page): Promise<void> {
  await page.goto('./');
  await expect(page.locator('#main-menu')).toBeVisible();
}

/** Puts worlds in the browser's index (the launcher and every version share this origin). */
async function seedWorlds(page: Page, worlds: unknown[]): Promise<void> {
  await openLauncher(page);
  await page.evaluate((w) => {
    localStorage.setItem('dwell.worlds', JSON.stringify(w));
  }, worlds);
}

/** The version directory of the page's address. */
const versionOf = (page: Page) => /\/dwell\/v\/([^/]+)\//.exec(new URL(page.url()).pathname)?.[1];

test('the launcher is the main menu, for the latest stable build', async ({ page }) => {
  await openLauncher(page);
  expect(versionOf(page)).toBeUndefined();
  await expect(page.locator('#app-version')).toContainText('Dwell 0.2.0 (stable)');
  // A new world is made by the latest build, which then plays it; ?debug=1 is kept.
  await page.goto('./?debug=1');
  await page.locator('#world-name').fill('Fresh');
  await page.locator('#create-world button[type=submit]').click();
  await expect.poll(() => versionOf(page)).toBe('0.2.0');
  // The build makes the world once it has started, then replaces ?new=… with ?play=<id>.
  const param = (name: string) => new URL(page.url()).searchParams.get(name);
  await expect.poll(() => param('play'), { timeout: 15_000 }).toMatch(/^w[0-9a-z]{10}$/);
  expect(param('debug')).toBe('1');
});

test('opens a world in the newest build of its own compatibility line', async ({ page }) => {
  await seedWorlds(page, [
    world('wold0000001', 'Old line', '0.1.0'),
    world('wnew0000001', 'New line', '0.2.0'),
  ]);
  await page.goto('./?play=wold0000001');
  await expect.poll(() => versionOf(page)).toBe('0.1.1'); // 0.1.0's line: the newest, not 0.2.0
  expect(new URL(page.url()).searchParams.get('play')).toBe('wold0000001');
  await page.goto('./?play=wnew0000001');
  await expect.poll(() => versionOf(page)).toBe('0.2.0');
});

test("opens an invite in a build on the host's line", async ({ page }) => {
  await page.goto(`./?join=127.0.0.1:4433&cert=${CERT}&v=0.1.0`);
  await expect.poll(() => versionOf(page)).toBe('0.1.1');
  const params = new URL(page.url()).searchParams;
  expect(params.get('join')).toBe('127.0.0.1:4433');
  expect(params.get('v')).toBe('0.1.0');
  // An invite that names no version opens the latest.
  await page.goto(`./?join=127.0.0.1:4433&cert=${CERT}`);
  await expect.poll(() => versionOf(page)).toBe('0.2.0');
});

test("the latest menu lists every line's worlds with version badges, and plays them in their line", async ({
  page,
}) => {
  await seedWorlds(page, [
    world('wold0000001', 'Old line', '0.1.0'),
    world('wnew0000001', 'New line', '0.2.0'),
    world('wprev0000001', 'Before versions'),
  ]);
  await openLauncher(page);
  await expect(page.locator('#app-version')).toContainText('Dwell 0.2.0');
  const item = (name: string) => page.locator('#world-list .world-item', { hasText: name });
  await expect(item('Old line')).toContainText('v0.1.0');
  await expect(item('New line')).toContainText('v0.2.0');
  // A world from before versioned releases is listed apart, to delete.
  await expect(page.locator('#legacy-worlds')).toContainText('Before versions');
  await expect(page.locator('#world-list')).not.toContainText('Before versions');
  // Playing the old world goes through the launcher to its own line.
  await item('Old line').locator('.world-play').click();
  await expect.poll(() => versionOf(page)).toBe('0.1.1');
  expect(new URL(page.url()).searchParams.get('play')).toBe('wold0000001');
});

test('a build opened with nothing to play hands over to the launcher', async ({ page }) => {
  await page.goto('./v/0.1.0/');
  await expect(page.locator('#main-menu')).toBeVisible();
  expect(versionOf(page)).toBeUndefined();
});

test('says so when no published build can open a world, and offers the latest', async ({
  page,
}) => {
  await seedWorlds(page, [world('wfut0000001', 'From the future', '0.3.0')]);
  await page.goto('./?play=wfut0000001');
  await expect(page.locator('#launcher-status')).toContainText('0.3.0');
  expect(versionOf(page)).toBeUndefined();
  await page.getByRole('button', { name: 'Open Dwell 0.2.0' }).click();
  await expect.poll(() => versionOf(page)).toBe('0.2.0');
});

test('says so for a version that is not published', async ({ page }) => {
  await page.goto('./?version=9.9.9');
  await expect(page.locator('#launcher-status')).toContainText('9.9.9');
  await expect(page.getByRole('button', { name: 'Open Dwell 0.2.0' })).toBeVisible();
});

test('falls back to the stable build when the dev channel has none', async ({ page }) => {
  await openLauncher(page);
  await page.evaluate(() => {
    localStorage.setItem('dwell.channel', 'dev');
  });
  await openLauncher(page);
  await expect(page.locator('#app-version')).toContainText('Dwell 0.2.0');
});

test('the menu links to the version page, and a chosen older build still returns to the latest', async ({
  page,
}) => {
  await seedWorlds(page, [world('wold0000001', 'Old line', '0.1.0')]);
  await openLauncher(page);
  await expect(page.locator('#app-version')).toContainText('Dwell 0.2.0');
  await page.locator('#versions-link').click();
  await expect(page.locator('#version-list')).toBeVisible();
  expect(versionOf(page)).toBeUndefined();
  const row = (v: string) => page.locator(`.version-row[data-version="${v}"]`);
  await expect(page.locator('.version-row')).toHaveCount(3);
  await expect(row('0.2.0')).toContainText('recommended');
  await expect(row('0.1.1')).toContainText('Old line');
  await expect(row('0.2.0')).toContainText('Opens none of your worlds yet');
  // Playing the older build pins the menu to it for this visit: a new world is that build's...
  await row('0.1.0').locator('.version-open').click();
  await expect(page.locator('#app-version')).toContainText('Dwell 0.1.0');
  // A world is listed, so the New world form starts closed.
  await page.locator('#new-world').click();
  await page.locator('#world-name').fill('Pinned');
  await page.locator('#create-world button[type=submit]').click();
  await expect.poll(() => versionOf(page)).toBe('0.1.0');
  // ...and a fresh visit to /dwell/ is still the latest: the choice is not remembered.
  await openLauncher(page);
  await expect(page.locator('#app-version')).toContainText('Dwell 0.2.0');
});

test('the version page opens by address, with dev builds hidden until asked', async ({ page }) => {
  await page.goto('./?versions');
  await expect(page.locator('#version-list')).toBeVisible();
  await expect(page.locator('.version-row')).toHaveCount(3);
  // Retired patches are off the list; the page says where they are (RELEASES.md §4).
  await expect(page.locator('#version-retired a')).toHaveAttribute('href', /\/releases$/);
  await page.locator('#use-dev-builds').check();
  expect(await page.evaluate(() => localStorage.getItem('dwell.channel'))).toBe('dev');
  await page.locator('#use-dev-builds').uncheck();
  await expect(page.locator('.version-row')).toHaveCount(3);
});

test('a link that names a world, game or build does not stop on the menu', async ({ page }) => {
  await page.goto('./?world=flat');
  await expect.poll(() => versionOf(page)).toBe('0.2.0');
});
