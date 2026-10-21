import { readFileSync } from 'node:fs';
import { expect, test, type Page } from '@playwright/test';
import { INVITE_FILE } from './global-setup';

const invite = () => readFileSync(INVITE_FILE, 'utf8');

/** Waits until the status line shows a joined session with both RTTs measured. */
async function expectJoined(page: Page, transport: string): Promise<void> {
  await expect(page.locator('#net-status')).toHaveText(
    new RegExp(`\\(${transport}\\) · player \\d+ · RTT \\d+ ms \\(datagram \\d+ ms\\)`),
    { timeout: 15_000 },
  );
}

test('joins a native server over WebTransport', async ({ page }) => {
  await page.goto(`./${invite()}`);
  await expectJoined(page, 'WebTransport');
});

test('joins a native server over the WebRTC fallback', async ({ page }) => {
  await page.goto(`./${invite()}&transport=webrtc`);
  await expectJoined(page, 'WebRTC');
});

test('runs a local world with no server (GitHub Pages mode)', async ({ page }) => {
  await page.goto('./?local=1');
  await expectJoined(page, 'local');
});

test('a second login with the same device key replaces the first', async ({ context }) => {
  const first = await context.newPage();
  await first.goto(`./${invite()}`);
  await expectJoined(first, 'WebTransport');
  const second = await context.newPage();
  await second.goto(`./${invite()}`);
  await expectJoined(second, 'WebTransport');
  await expect(first.locator('#net-status')).toContainText('Signed in from another connection');
});

interface LoadingState {
  enabled: boolean;
  shown: boolean;
  ready?: number;
  needed?: number;
  openedS?: number;
  timedOut?: boolean;
}

const loadingOf = (page: Page) =>
  page.evaluate<{ loading: LoadingState; terrainReady: boolean } | null>(
    '(() => { const s = globalThis.__dwell?.state(); return s ? { loading: s.loading, terrainReady: s.terrainReady } : null; })()',
  );

test('the loading screen covers the join until the terrain around the player is drawn', async ({
  page,
}) => {
  // The suite's storage turns the screen off (playwright.config.ts): back on for this page.
  await page.addInitScript(() => {
    localStorage.removeItem('dwell.loading');
  });
  await page.goto('./?world=flat');
  // Until it opens the player does not move: terrainReady (the prediction gate) stays false.
  await expect
    .poll(
      async () => {
        const s = await loadingOf(page);
        if (s?.loading.shown) expect(s.terrainReady).toBe(false);
        return s?.loading.openedS !== undefined;
      },
      { timeout: 60_000 },
    )
    .toBe(true);
  const s = await loadingOf(page);
  expect(s?.loading.enabled).toBe(true);
  expect(s?.loading.shown).toBe(false);
  await expect(page.locator('#loading-screen')).toBeHidden();
  await expect
    .poll(async () => (await loadingOf(page))?.terrainReady, { timeout: 30_000 })
    .toBe(true);
});

test('?loading=off skips the loading screen', async ({ page }) => {
  await page.addInitScript(() => {
    localStorage.removeItem('dwell.loading');
  });
  await page.goto('./?world=flat&loading=off');
  await expect
    .poll(async () => (await loadingOf(page))?.terrainReady, { timeout: 30_000 })
    .toBe(true);
  const s = await loadingOf(page);
  expect(s?.loading).toEqual({ enabled: false, shown: false });
  await expect(page.locator('#loading-screen')).toBeHidden();
});
