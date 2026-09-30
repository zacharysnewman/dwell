// Phase 5e: the lobby list (ARCHITECTURE.md §10.3). A public native server and a public friend
// world (hosted from a browser) both appear in the server browser of a third browser, which joins
// each from the list. The new server is listed under "New servers" until players' join receipts
// verify it; the friend world is listed with its players.
import { expect, test, type Page } from '@playwright/test';
import { MASTER_URL, startServer } from './global-setup';

const master = `master=${encodeURIComponent(MASTER_URL)}`;

async function terrainReady(page: Page): Promise<void> {
  await expect
    .poll(
      () => page.evaluate<boolean>('(() => globalThis.__dwell?.state()?.terrainReady ?? false)()'),
      { timeout: 30_000 },
    )
    .toBe(true);
}

test('a public server and a public friend world are listed, and joined from the list', async ({
  browser,
}) => {
  test.setTimeout(120_000);
  const server = await startServer([
    '--port',
    '0',
    '--name',
    'E2E public',
    '--motd',
    'Lobby test',
    '--world',
    '',
    '--visibility',
    'public',
    '--tags',
    'e2e-lobby,pve',
  ]);
  const contexts = [await browser.newContext(), await browser.newContext()];
  try {
    const [host, player] = await Promise.all(contexts.map((c) => c.newPage()));

    // A browser hosts its world publicly.
    await host.goto(`./?world=flat&seed=77&${master}`);
    await terrainReady(host);
    await host.locator('#menu-button').click();
    await host.locator('#menu-host').click();
    await host.locator('#host-visibility').selectOption({ label: 'Public (server list)' });
    await host.locator('#host-start').click();
    const display = (await host.locator('#host-code').textContent({ timeout: 15_000 })) ?? '';
    const worldCode = display.replace('-', '');
    await host.locator('#menu-button').click(); // close the menu

    // A third browser's server browser lists the friend world, with its players…
    await player.goto(`./?${master}`);
    const world = player.locator(`#lobby-list [data-code="${worldCode}"]`);
    await expect(world).toContainText('friend world · 1/', { timeout: 15_000 });
    // …and the new server under "New servers", found by a tag, pinged over WebTransport.
    const serverCode = server.code.replace('-', '');
    await expect(player.locator(`#lobby-list [data-code="${serverCode}"]`)).toHaveCount(0);
    await player.locator('#lobby-new').check();
    await player.locator('#lobby-search').fill('e2e-lobby');
    const listed = player.locator(`#lobby-list [data-code="${serverCode}"]`);
    await expect(listed).toContainText('E2E public', { timeout: 10_000 });
    await expect(listed).toContainText('Lobby test');
    await expect(listed).toContainText('0/');
    await expect(listed.locator('.lobby-ping')).toHaveText(/^\d+ ms$/, { timeout: 10_000 });

    // Joining the server from the list.
    await listed.locator('.lobby-join').click();
    await expect(player).toHaveURL(new RegExp(`[?&]code=${serverCode}`));
    await expect(player.locator('#net-status')).toContainText('(WebTransport) · player', {
      timeout: 20_000,
    });

    // Back to the menu; joining the friend world from the list.
    await player.goto(`./?${master}`);
    await world.locator('.lobby-join').click();
    await expect(player).toHaveURL(new RegExp(`[?&]code=${worldCode}`));
    await expect(player.locator('#net-status')).toContainText('(WebRTC) · player', {
      timeout: 30_000,
    });
    await expect(host.locator('#host-guests')).toHaveText(/1 of \d+ guests? playing/);
  } finally {
    server.process.kill('SIGKILL');
    await Promise.all(contexts.map((c) => c.close()));
  }
});
