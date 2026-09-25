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
  await page.goto('./');
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
