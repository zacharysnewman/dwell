/// <reference lib="dom" />
// Worlds are locked to their compatibility line (RELEASES.md §6) in the browser too: the local-mode
// core refuses a world file last saved by a version this build may not open, and says which.
import { expect, test, type Page } from '@playwright/test';

/**
 * Rewrites the app version a saved world file records as its last saver (`app_version_last` in the
 * `meta` table) to 9.9.9, in place: the same length, so the SQLite file stays valid. Every copy of
 * the row is rewritten (a page may keep a freed older one).
 */
async function claimSavedByNewerVersion(page: Page, id: string): Promise<number> {
  return page.evaluate(async (worldId) => {
    const root = await navigator.storage.getDirectory();
    const dir = await (await root.getDirectoryHandle('dwell')).getDirectoryHandle('worlds');
    const handle = await dir.getFileHandle(`${worldId}.dwellworld`);
    const bytes = new Uint8Array(await (await handle.getFile()).arrayBuffer());
    const key = new TextEncoder().encode('app_version_last');
    let rewritten = 0;
    for (let i = 0; i + key.length + 5 <= bytes.length; i++) {
      if (!key.every((b, j) => bytes[i + j] === b)) continue;
      const at = i + key.length;
      const value = new TextDecoder().decode(bytes.subarray(at, at + 5));
      if (!/^\d\.\d\.\d$/.test(value)) continue;
      bytes.set(new TextEncoder().encode('9.9.9'), at);
      rewritten++;
    }
    // The page that saved it may still hold the file for a moment.
    for (let attempt = 0; ; attempt++) {
      try {
        const writable = await handle.createWritable();
        await writable.write(bytes);
        await writable.close();
        return rewritten;
      } catch (err) {
        if (attempt >= 50) throw err;
        await new Promise((resolve) => setTimeout(resolve, 100));
      }
    }
  }, id);
}

/** The first five characters of each `app_version_last` value in the world file (read only). */
async function recordedVersions(page: Page, id: string): Promise<string[]> {
  return page.evaluate(async (worldId) => {
    const root = await navigator.storage.getDirectory();
    const dir = await (await root.getDirectoryHandle('dwell')).getDirectoryHandle('worlds');
    const handle = await dir.getFileHandle(`${worldId}.dwellworld`);
    const bytes = new Uint8Array(await (await handle.getFile()).arrayBuffer());
    const key = new TextEncoder().encode('app_version_last');
    const found: string[] = [];
    for (let i = 0; i + key.length + 5 <= bytes.length; i++) {
      if (!key.every((b, j) => bytes[i + j] === b)) continue;
      found.push(new TextDecoder().decode(bytes.subarray(i + key.length, i + key.length + 5)));
    }
    return found;
  }, id);
}

test('a world saved by a newer version is refused, with the version to use, and left alone', async ({
  page,
}) => {
  test.setTimeout(90_000);
  await page.goto('./');
  await page.locator('#world-name').fill('Locked');
  await page.locator('#world-type').selectOption('flat');
  await page.locator('#create-world button[type=submit]').click();
  await expect(page).toHaveURL(/[?&]play=(w[0-9a-z]{10})/);
  const id = /[?&]play=(w[0-9a-z]{10})/.exec(page.url())?.[1] ?? '';
  await expect
    .poll(
      async () =>
        page.evaluate(
          () =>
            (
              globalThis as { __dwell?: { state(): { terrainReady: boolean } | null } }
            ).__dwell?.state()?.terrainReady ?? false,
        ),
      { timeout: 20_000 },
    )
    .toBe(true);
  await page.waitForTimeout(6_000); // the autosave: the file records this build's version
  await page.goto('./'); // leave the game (the world file is released)
  await expect(page.locator('#main-menu')).toBeVisible();

  expect(await claimSavedByNewerVersion(page, id)).toBeGreaterThan(0);

  // The index still says this build played it; the file itself says otherwise.
  await page.goto(`./?play=${id}`);
  await expect(page.locator('#net-status')).toContainText('9.9.9', { timeout: 15_000 });
  await expect(page.locator('#net-status')).toContainText('Could not connect');
  // No game started: nothing was loaded, and nothing was written to the file.
  expect(
    await page.evaluate(
      () =>
        (globalThis as { __dwell?: { state(): { terrainReady: boolean } | null } }).__dwell?.state()
          ?.terrainReady ?? false,
    ),
  ).toBe(false);
  await page.waitForTimeout(1_500);
  // The refusal wrote nothing: every copy of the row still reads 9.9.9.
  const recorded = await recordedVersions(page, id);
  expect(recorded.length).toBeGreaterThan(0);
  expect(recorded.every((v) => v === '9.9.9')).toBe(true);
});
