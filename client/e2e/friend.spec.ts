// Phase 5c: friend worlds (ARCHITECTURE.md §10.2). One page hosts its local world from the game
// menu and gets a join code; a second browser context joins by that code through the local master
// (WebRTC between the pages); each sees the other move, and a guest's edit; stopping hosting
// disconnects the guest with a reason.
import { expect, test, type Page } from '@playwright/test';
import { MASTER_URL } from './global-setup';

type Vec3 = [number, number, number];
interface DebugState {
  terrainReady: boolean;
  terrain: { loaded: number; generating: number };
  target: { cell: Vec3; face: number } | null;
  remotes: { playerId: number; feet: Vec3 }[];
}

const FACE_DIRS: Vec3[] = [
  [1, 0, 0],
  [-1, 0, 0],
  [0, 1, 0],
  [0, -1, 0],
  [0, 0, 1],
  [0, 0, -1],
];
const add = (a: Vec3, b: Vec3): Vec3 => [a[0] + b[0], a[1] + b[1], a[2] + b[2]];

function call<T>(page: Page, expr: string): Promise<T> {
  return page.evaluate<T>(`(() => { const d = globalThis.__dwell; return d ? ${expr} : null; })()`);
}
const state = (page: Page) => call<DebugState | null>(page, 'd.state()');
const voxel = (page: Page, c: Vec3) => call<number>(page, `d.voxel(${c.join(',')})`);

async function ready(page: Page): Promise<void> {
  await expect
    .poll(async () => (await state(page))?.terrainReady ?? false, { timeout: 30_000 })
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

/** Where the other player is, as this page sees them. */
async function other(page: Page): Promise<Vec3 | null> {
  return (await state(page))?.remotes[0]?.feet ?? null;
}

/** Walks forward for a moment and checks the other page saw the player move. */
async function seenMoving(walker: Page, watcher: Page): Promise<void> {
  await expect.poll(() => other(watcher), { timeout: 15_000 }).not.toBeNull();
  const before = await other(watcher);
  await call(walker, `d.press('KeyW', true)`);
  await walker.waitForTimeout(800);
  await call(walker, `d.press('KeyW', false)`);
  await expect
    .poll(
      async () => {
        const now = await other(watcher);
        return now && before ? Math.hypot(now[0] - before[0], now[2] - before[2]) : 0;
      },
      { timeout: 5_000 },
    )
    .toBeGreaterThan(1);
}

test('friend world: host from the game menu, join by code, play together, stop hosting', async ({
  browser,
}) => {
  test.setTimeout(150_000);
  const contexts = [await browser.newContext(), await browser.newContext()];
  const [host, guest] = await Promise.all(contexts.map((c) => c.newPage()));
  const master = `master=${encodeURIComponent(MASTER_URL)}`;

  await host.goto(`./?world=flat&seed=55&${master}`);
  await ready(host);
  await host.locator('#menu-button').click();
  await host.locator('#menu-host').click();
  await host.locator('#host-edits').selectOption({ label: 'Everyone' });
  await host.locator('#host-start').click();
  const code = (await host.locator('#host-code').textContent({ timeout: 15_000 })) ?? '';
  expect(code).toMatch(/^[A-HJKMNP-Z2-9]{3}-[A-HJKMNP-Z2-9]{3}$/);
  const link = await host.locator('#host-link').inputValue();
  expect(link).toContain(`code=${code.replace('-', '')}`);
  await host.locator('#menu-button').click(); // close the menu

  // The guest types the code on the main menu.
  await guest.goto(`./?${master}`);
  await guest.locator('#join-input').fill(code.toLowerCase());
  await guest.locator('.menu-join-form button[type=submit]').click();
  await expect(guest.locator('#net-status')).toContainText(`${code} (WebRTC) · player`, {
    timeout: 30_000,
  });
  await ready(guest);
  await expect(host.locator('#host-guests')).toHaveText('1 of 8 guests playing');

  await seenMoving(host, guest);
  await seenMoving(guest, host);

  // A block the guest places goes through the host's world and appears on both pages. Both
  // players spawned at one point and walked the same way, so the host stands just ahead of the
  // guest: place behind, where nobody is (the server refuses blocks inside a player).
  await call(guest, 'd.look(180, -45)');
  await expect.poll(async () => (await state(guest))?.target ?? null).not.toBeNull();
  const target = (await state(guest))?.target;
  if (!target) throw new Error('nothing targeted');
  const cell = add(target.cell, FACE_DIRS[target.face] ?? [0, 0, 0]);
  await guest.keyboard.press('Digit1'); // stone
  expect(await call<boolean>(guest, `d.edit('place')`)).toBe(true);
  await expect.poll(() => voxel(host, cell), { timeout: 10_000 }).toBe(2);
  await expect.poll(() => voxel(guest, cell), { timeout: 10_000 }).toBe(2);

  // Stopping hosting disconnects the guest with a reason; the host keeps playing.
  await host.locator('#menu-button').click();
  await host.locator('#host-stop').click();
  await expect(guest.locator('#net-status')).toContainText('The host stopped hosting', {
    timeout: 10_000,
  });
  await expect(host.locator('#host-status')).toContainText('Stopped hosting');
  expect((await state(host))?.terrainReady).toBe(true);
  await Promise.all(contexts.map((c) => c.close()));
});
