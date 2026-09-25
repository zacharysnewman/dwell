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
  await page.goto('./');
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
