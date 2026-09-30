// Touch controls on a landscape phone (local mode): the stick walks, dragging looks, Jump jumps.
import { devices, expect, test, type CDPSession, type Page } from '@playwright/test';

test.use({ ...devices['iPhone 13 landscape'], browserName: 'chromium' });

interface State {
  active: boolean;
  feet: [number, number, number];
}
const read = (page: Page) =>
  page.evaluate<State | null>('(globalThis.__dwell && globalThis.__dwell.state()) || null');

const yaw = (page: Page) =>
  page.evaluate<number>('globalThis.__dwell ? globalThis.__dwell.view().yaw : 0');

type Point = { x: number; y: number; id: number };
async function touch(
  cdp: CDPSession,
  type: 'touchStart' | 'touchMove' | 'touchEnd',
  points: Point[],
) {
  await cdp.send('Input.dispatchTouchEvent', {
    type,
    touchPoints: type === 'touchEnd' ? [] : points.map((p) => ({ x: p.x, y: p.y, id: p.id })),
  });
}

test('touch controls: stick, look, and jump', async ({ page }) => {
  await page.goto('./?local=1');
  await expect
    .poll(async () => (await read(page))?.active ?? false, { timeout: 20_000 })
    .toBe(true);
  await expect(page.locator('#touch-controls')).toBeVisible();
  await expect(page.locator('#touch-jump')).toBeVisible();
  const cdp = await page.context().newCDPSession(page);
  const start = await read(page);

  // Drag the stick up (forward) from the lower left and hold for a second.
  await touch(cdp, 'touchStart', [{ x: 150, y: 300, id: 1 }]);
  for (let i = 1; i <= 5; i++) await touch(cdp, 'touchMove', [{ x: 150, y: 300 - i * 12, id: 1 }]);
  await expect(page.locator('.stick')).toBeVisible();
  await page.waitForTimeout(1000);
  await touch(cdp, 'touchEnd', []);
  const walked = await read(page);
  const moved = Math.hypot(
    (walked?.feet[0] ?? 0) - (start?.feet[0] ?? 0),
    (walked?.feet[2] ?? 0) - (start?.feet[2] ?? 0),
  );
  expect(moved).toBeGreaterThan(2.5);

  // Drag on the right half to look, while jumping with another finger.
  const yawBefore = await yaw(page);
  await touch(cdp, 'touchStart', [{ x: 600, y: 200, id: 2 }]);
  for (let i = 1; i <= 5; i++) await touch(cdp, 'touchMove', [{ x: 600 + i * 20, y: 200, id: 2 }]);
  await touch(cdp, 'touchEnd', []);
  const yawAfter = await yaw(page);
  expect(Math.abs(yawAfter - yawBefore)).toBeGreaterThan(10);

  const box = await page.locator('#touch-jump').boundingBox();
  if (!box) throw new Error('no jump button');
  const ground = (await read(page))?.feet[1] ?? 0;
  await touch(cdp, 'touchStart', [{ x: box.x + box.width / 2, y: box.y + box.height / 2, id: 3 }]);
  let peak = ground;
  for (let i = 0; i < 10; i++) {
    await page.waitForTimeout(40);
    peak = Math.max(peak, (await read(page))?.feet[1] ?? 0);
  }
  await touch(cdp, 'touchEnd', []);
  expect(peak - ground).toBeGreaterThan(0.5);
});

test('touch controls: Break/Place toggle, tapping the view edits, tapping the hotbar picks', async ({
  page,
}) => {
  await page.goto('./?world=flat');
  await expect
    .poll(
      async () =>
        page.evaluate<boolean>(
          '!!(globalThis.__dwell && globalThis.__dwell.state()?.terrainReady)',
        ),
      { timeout: 20_000 },
    )
    .toBe(true);
  const cdp = await page.context().newCDPSession(page);
  const tap = async (x: number, y: number, id: number) => {
    await touch(cdp, 'touchStart', [{ x, y, id }]);
    await touch(cdp, 'touchEnd', []);
  };
  const center = async (selector: string) => {
    const box = await page.locator(selector).boundingBox();
    if (!box) throw new Error(`no ${selector}`);
    return { x: box.x + box.width / 2, y: box.y + box.height / 2 };
  };
  // Look down at the ground ahead; the cell above the targeted one receives the block.
  await page.evaluate('globalThis.__dwell.look(0, -45)');
  await expect
    .poll(() => page.evaluate('globalThis.__dwell.state()?.target ?? null'))
    .not.toBeNull();
  const target = await page.evaluate<{ cell: [number, number, number] }>(
    'globalThis.__dwell.state().target',
  );
  const [x, y, z] = target.cell;
  const above = `globalThis.__dwell.voxel(${String(x)}, ${String(y + 1)}, ${String(z)})`;

  // Pick dirt (slot 2) on the hotbar, switch to Place, and tap the view.
  const dirt = await center('.hotbar-slot[data-slot="1"]');
  await tap(dirt.x, dirt.y, 4);
  await expect(page.locator('.hotbar-slot.selected')).toHaveAttribute('data-slot', '1');
  const edit = await center('#touch-edit');
  await tap(edit.x, edit.y, 5);
  await expect(page.locator('#touch-edit')).toHaveText('Place');
  await tap(600, 150, 6);
  await expect.poll(() => page.evaluate<number>(above), { timeout: 5_000 }).toBe(3);

  // Back to Break: a tap removes it again.
  await tap(edit.x, edit.y, 7);
  await expect(page.locator('#touch-edit')).toHaveText('Break');
  await page.waitForTimeout(150);
  await tap(600, 150, 8);
  await expect.poll(() => page.evaluate<number>(above), { timeout: 5_000 }).toBe(0);
});

test('on a phone the hotbar, connection status, debug overlay and buttons do not overlap', async ({
  page,
}) => {
  // Regression (phone playtest): the hotbar sat over both.
  await page.goto('./?world=flat&debug=1');
  await expect
    .poll(async () => (await read(page))?.active ?? false, { timeout: 20_000 })
    .toBe(true);
  const selectors = ['#hotbar', '#net-status', '#debug-overlay', '#touch-debug', '#menu-button'];
  const boxes: { x: number; y: number; width: number; height: number }[] = [];
  for (const selector of selectors) {
    await expect(page.locator(selector)).toBeVisible();
    const box = await page.locator(selector).boundingBox();
    if (!box) throw new Error(`no ${selector}`);
    boxes.push(box);
  }
  boxes.forEach((p, a) => {
    boxes.slice(a + 1).forEach((q, k) => {
      const overlap =
        p.x < q.x + q.width && q.x < p.x + p.width && p.y < q.y + q.height && q.y < p.y + p.height;
      expect(overlap, `${selectors[a]} overlaps ${selectors[a + 1 + k]}`).toBe(false);
    });
  });
});

test('on a phone a button toggles the debug overlay (no F3 key)', async ({ page }) => {
  await page.goto('./?world=flat');
  await expect
    .poll(async () => (await read(page))?.active ?? false, { timeout: 20_000 })
    .toBe(true);
  const cdp = await page.context().newCDPSession(page);
  const tapButton = async (id: number) => {
    const box = await page.locator('#touch-debug').boundingBox();
    if (!box) throw new Error('no debug button');
    const p = { x: box.x + box.width / 2, y: box.y + box.height / 2, id };
    await touch(cdp, 'touchStart', [p]);
    await touch(cdp, 'touchEnd', []);
  };
  await expect(page.locator('#debug-overlay')).toBeHidden();
  await tapButton(1);
  await expect(page.locator('#debug-overlay')).toBeVisible();
  await tapButton(2);
  await expect(page.locator('#debug-overlay')).toBeHidden();
});
