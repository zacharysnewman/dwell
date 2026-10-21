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

interface CoverSample {
  overlay: boolean;
  fading: boolean;
  hud: string[];
}

test('the loading screen is up before anything of the game, then fades in from white', async ({
  page,
}) => {
  // Owner (2026-10-21): "you see ingame UI briefly before loading screen appears". Every frame
  // from the page's start is sampled: until the fade begins no part of the HUD is visible, and
  // from the moment any of it exists the screen covers it.
  await page.addInitScript(() => {
    localStorage.removeItem('dwell.loading');
    const samples: CoverSample[] = [];
    (globalThis as unknown as { __cover: CoverSample[] }).__cover = samples;
    const ids = ['hud', 'hotbar', 'touch-controls', 'flight-speed', 'net-status'];
    const sample = () => {
      const el = document.getElementById('loading-screen');
      samples.push({
        overlay: !!el && !el.hidden,
        fading: !!el?.classList.contains('ls-fading'),
        hud: ids.filter((id) => {
          const e = document.getElementById(id);
          return !!e && e.checkVisibility({ visibilityProperty: true, opacityProperty: true });
        }),
      });
      requestAnimationFrame(sample);
    };
    requestAnimationFrame(sample);
  });
  await page.goto('./?world=flat');
  await expect
    .poll(async () => (await loadingOf(page))?.loading.openedS !== undefined, { timeout: 60_000 })
    .toBe(true);
  const samples = await page.evaluate(
    () => (globalThis as unknown as { __cover: CoverSample[] }).__cover,
  );
  // Frames before the page's script runs have no screen, and nothing of the game either.
  const up = samples.findIndex((s) => s.overlay);
  expect(up, 'the screen showed').toBeGreaterThanOrEqual(0);
  const fade = samples.findIndex((s, i) => i > up && (s.fading || !s.overlay));
  expect(fade, 'the screen opened').toBeGreaterThan(up);
  const before = samples.slice(0, fade);
  expect(samples.slice(up, fade).every((s) => s.overlay && !s.fading)).toBe(true);
  const flashed = before.filter((s) => s.hud.length > 0);
  expect(flashed, `HUD shown before the screen opened: ${JSON.stringify(flashed[0])}`).toEqual([]);
  // It turned white and faded (not a cut), then the HUD shows.
  expect(samples[fade]?.fading).toBe(true);
  await expect(page.locator('#loading-screen')).toBeHidden();
  await expect(page.locator('#hotbar')).toBeVisible();
});
