/// <reference lib="dom" />
// Worlds are locked to their compatibility line (RELEASES.md §6) in the browser too: the local-mode
// core refuses a world file last saved by a version this build may not open, and says which.
import { expect, test, type Page } from '@playwright/test';

interface WorldFileHooks {
  worldFileVersion(bytes: Uint8Array): string | undefined;
  setWorldFileVersion(bytes: Uint8Array, version: string): Uint8Array;
}

/**
 * Rewrites the world file to say version 9.9.9 last saved it (`app_version_last` in its meta),
 * through the client's own world-file code (window.__dwellTools, on the main menu).
 */
async function claimSavedByNewerVersion(page: Page, id: string): Promise<string | undefined> {
  await page.waitForFunction(() => '__dwellTools' in globalThis);
  return page.evaluate(async (worldId) => {
    const hooks = (globalThis as { __dwellTools?: WorldFileHooks }).__dwellTools;
    const root = await navigator.storage.getDirectory();
    const dir = await (await root.getDirectoryHandle('dwell')).getDirectoryHandle('worlds');
    const handle = await dir.getFileHandle(`${worldId}.dwellworld`);
    const bytes = new Uint8Array(await (await handle.getFile()).arrayBuffer());
    const rewritten = hooks?.setWorldFileVersion(bytes, '9.9.9');
    if (!rewritten) throw new Error('no world-file hooks');
    // The page that saved it may still hold the file for a moment.
    for (let attempt = 0; ; attempt++) {
      try {
        const writable = await handle.createWritable();
        await writable.write(new Uint8Array(rewritten));
        await writable.close();
        return hooks?.worldFileVersion(rewritten);
      } catch (err) {
        if (attempt >= 50) throw err;
        await new Promise((resolve) => setTimeout(resolve, 100));
      }
    }
  }, id);
}

/** The app version the world file records as its last saver (read only). */
async function recordedVersion(page: Page, id: string): Promise<string | undefined> {
  await page.waitForFunction(() => '__dwellTools' in globalThis);
  return page.evaluate(async (worldId) => {
    const hooks = (globalThis as { __dwellTools?: WorldFileHooks }).__dwellTools;
    const root = await navigator.storage.getDirectory();
    const dir = await (await root.getDirectoryHandle('dwell')).getDirectoryHandle('worlds');
    const handle = await dir.getFileHandle(`${worldId}.dwellworld`);
    return hooks?.worldFileVersion(new Uint8Array(await (await handle.getFile()).arrayBuffer()));
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

  expect(await claimSavedByNewerVersion(page, id)).toBe('9.9.9');

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
  // The refusal wrote nothing: the file still says 9.9.9.
  expect(await recordedVersion(page, id)).toBe('9.9.9');
});
