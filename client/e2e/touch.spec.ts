// Touch controls on a landscape phone (local mode): the stick walks, dragging looks, Jump jumps.
import { devices, expect, test, type CDPSession, type Page } from '@playwright/test';
import { CELL_HEIGHT } from '../src/protocol/constants.gen';
import { stateId } from '../src/world/blocks';

test.use({ ...devices['iPhone 13 landscape'], browserName: 'chromium' });

interface State {
  active: boolean;
  feet: [number, number, number];
  stats: { ticks: number };
}
const read = (page: Page) =>
  page.evaluate<State | null>('(globalThis.__dwell && globalThis.__dwell.state()) || null');

/** The player controller's kept ticks (telemetry.rs `TickSample`), oldest first. */
const ticks = (page: Page) =>
  page.evaluate<{ seq: number; feet: [number, number, number] }[]>(
    'globalThis.__dwell ? globalThis.__dwell.ticks() : []',
  );

const yaw = (page: Page) =>
  page.evaluate<number>('globalThis.__dwell ? globalThis.__dwell.view().yaw : 0');

type Point = { x: number; y: number; id: number };
/** `timestamp`: the event's time (s since the epoch); unset, the browser stamps it on arrival. */
async function touch(
  cdp: CDPSession,
  type: 'touchStart' | 'touchMove' | 'touchEnd',
  points: Point[],
  timestamp?: number,
) {
  await cdp.send('Input.dispatchTouchEvent', {
    type,
    touchPoints: type === 'touchEnd' ? [] : points.map((p) => ({ x: p.x, y: p.y, id: p.id })),
    ...(timestamp === undefined ? {} : { timestamp }),
  });
}

test('touch controls: stick, look, and jump', async ({ page }) => {
  // CI's software rendering is slow: the steps before the jump alone take ~20 s there. The flat
  // world, as the other touch tests use: on the generated terrain CI drew this page at ~1 fps by
  // the jump (frames 0.8-1.7 s), at the client's 1 s catch-up (game.rs MAX_TICKS_PER_FRAME), and a
  // frame past it drops time the server fills with the held jump. The flat world drew at
  // ~250 ms (p95 ~570 ms) a frame.
  test.setTimeout(90_000);
  await page.goto('./?world=flat');
  await expect
    .poll(async () => (await read(page))?.active ?? false, { timeout: 20_000 })
    .toBe(true);
  await expect(page.locator('#touch-controls')).toBeVisible();
  await expect(page.locator('#touch-jump')).toBeVisible();
  const cdp = await page.context().newCDPSession(page);
  const start = await read(page);
  const startedAt = Date.now();

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
  const lastSeq = (await ticks(page)).at(-1)?.seq ?? 0;
  await touch(cdp, 'touchStart', [{ x: box.x + box.width / 2, y: box.y + box.height / 2, id: 3 }]);
  // The jump's height from the controller's per-tick trace, not the drawn position: at CI's
  // 300-800 ms frames the jump's whole arc (~0.7 s) can fall between two frames' positions (the
  // drawn peak read 0, 0.11 or 0.22 m in half of six CI runs; the trace read 1.19 m). Watch until
  // the jump has clearly left the ground (or a deadline), not for a fixed number of frames.
  const jumpPeak = async () =>
    Math.max(ground, ...(await ticks(page)).filter((t) => t.seq > lastSeq).map((t) => t.feet[1]));
  let peak = ground;
  const deadline = Date.now() + 10_000;
  while (peak - ground <= 0.5 && Date.now() < deadline) {
    await page.waitForTimeout(100);
    peak = await jumpPeak();
  }
  await touch(cdp, 'touchEnd', []);
  // A page that predicts far fewer than 60 ticks a second drops time, and the server, filling the
  // missing ticks with the held input, flies and lands the jump without it (game.rs
  // MAX_TICKS_PER_FRAME): the rate and frame times say whether that is what happened.
  const predicted = ((await read(page))?.stats.ticks ?? 0) - (start?.stats.ticks ?? 0);
  const rate = (predicted * 1000) / (Date.now() - startedAt);
  const frames = await page.evaluate<{ p50: number; max: number } | undefined>(
    'globalThis.__dwell.telemetry().frames.interval',
  );
  const context = `jump peak, predicting ${rate.toFixed(1)} ticks/s, frames p50 ${(frames?.p50 ?? 0).toFixed(0)} ms, max ${(frames?.max ?? 0).toFixed(0)} ms`;
  expect(peak - ground, context).toBeGreaterThan(0.5);
});

test('touch controls: Break/Place toggle, tapping the view edits, tapping the hotbar picks', async ({
  page,
}) => {
  // Each tap waits for a frame of CI's software renderer: 30 s ran out there.
  test.setTimeout(90_000);
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
  // A tap lifts 50 ms after it lands. Stamped on arrival instead, a slow frame between the two
  // events could stretch it past TAP_MS, and the game would rightly take it for a look drag.
  const tap = async (x: number, y: number, id: number) => {
    const t = Date.now() / 1000;
    await touch(cdp, 'touchStart', [{ x, y, id }], t);
    await touch(cdp, 'touchEnd', [], t + 0.05);
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
  await expect
    .poll(() => page.evaluate<number>(above), { timeout: 5_000 })
    .toBe(stateId('dwell:dirt'));

  // Back to Break: a tap removes the cell aimed at, once the new block is the one targeted (on a
  // slow runner the target could still be the ground below, which the tap would break instead).
  // Dirt is a ½ m block, one cell (CELL_HEIGHT; docs/ITEMS.md): the crosshair is turned onto it.
  await tap(edit.x, edit.y, 7);
  await expect(page.locator('#touch-edit')).toHaveText('Break');
  const feet = (await read(page))?.feet ?? [0, 0, 0];
  const d = [
    x + 0.5 - feet[0],
    (y + 1.5) * CELL_HEIGHT - feet[1] - 1.62,
    z + 0.5 - feet[2],
  ] as const; // eye 1.62 m
  const lookYaw = (Math.atan2(d[0], d[2]) * 180) / Math.PI;
  const lookPitch = (Math.atan2(d[1], Math.hypot(d[0], d[2])) * 180) / Math.PI;
  await page.evaluate(`globalThis.__dwell.look(${String(lookYaw)}, ${String(lookPitch)})`);
  await expect
    .poll(() => page.evaluate<string>('globalThis.__dwell.state()?.target?.cell.join() ?? ""'))
    .toBe([x, y + 1, z].join());
  await page.waitForTimeout(150); // BLOCK_EDIT_INTERVAL_MS
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
