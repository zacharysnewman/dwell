// Phase 5d: dedicated servers on the master (ARCHITECTURE.md §10.1, §10.3). The suite's native
// server registers with the local master; a browser joins it by typing its address and by its
// join code, and sees it under "On your network"; a server that stops disappears within two
// heartbeat periods.
import { readFileSync } from 'node:fs';
import { expect, test, type Page } from '@playwright/test';
import { CODE_FILE, INVITE_FILE, MASTER_URL, startServer } from './global-setup';

const master = `master=${encodeURIComponent(MASTER_URL)}`;

async function expectJoined(page: Page): Promise<void> {
  await expect(page.locator('#net-status')).toContainText('(WebTransport) · player', {
    timeout: 20_000,
  });
}

test('joins a dedicated server by typing its address in the Join box', async ({ page }) => {
  const query = readFileSync(INVITE_FILE, 'utf8');
  const port = /join=[^&]*:(\d+)/.exec(query)?.[1];
  expect(port).toBeTruthy();
  await page.goto(`./?${master}`);
  await page.locator('#join-input').fill(`127.0.0.1:${String(port)}`);
  await page.locator('.menu-join-form button[type=submit]').click();
  await expect(page).toHaveURL(/[?&]join=/);
  await expectJoined(page);
});

test('joins a dedicated server by its join code', async ({ page }) => {
  const code = readFileSync(CODE_FILE, 'utf8');
  expect(code).toMatch(/^[A-HJKMNP-Z2-9]{3}-[A-HJKMNP-Z2-9]{3}$/);
  await page.goto(`./?code=${code.replace('-', '')}&${master}`);
  await expectJoined(page);
});

test('lists servers on the same network, and drops one that stops within two heartbeats', async ({
  page,
}) => {
  test.setTimeout(60_000);
  const temp = await startServer(['--port', '0', '--name', 'E2E temp', '--world', '']);
  try {
    await page.goto(`./?${master}`);
    const list = page.locator('#nearby-list');
    await expect(list).toContainText('E2E temp · 0/', { timeout: 10_000 });
    await expect(list).toContainText('E2E · ');
    // A crash (no goodbye to the master): gone after two missed heartbeats (2 × 2 s here).
    temp.process.kill('SIGKILL');
    const stoppedAt = Date.now();
    await expect
      .poll(
        async () => {
          await page.reload();
          await expect(page.locator('#nearby')).not.toContainText('Looking for games');
          return page.locator('#nearby-list').textContent();
        },
        { timeout: 15_000, intervals: [1000] },
      )
      .not.toContain('E2E temp');
    expect(Date.now() - stoppedAt).toBeLessThan(2 * 2000 + 3000);
    await expect(list).toContainText('E2E · ');
  } finally {
    temp.process.kill('SIGKILL');
  }
});
