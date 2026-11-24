// Touch controls on a landscape phone (local mode): the stick walks, dragging looks, Jump jumps.
import { devices, expect, test, type CDPSession, type Page } from '@playwright/test';
import { CELL_HEIGHT } from '../src/protocol/constants.gen';
import { stateId } from '../src/world/blocks';
import { untilTicking } from './ticking';

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

test('touch controls: stick, look, and jump @gpu', async ({ page }) => {
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

  // Pick dirt on the hotbar (by its title: the slots move as items are added), switch to Place,
  // and tap the view.
  const dirt = await center('#hotbar .hotbar-slot[title="Dirt"]:visible');
  await tap(dirt.x, dirt.y, 4);
  await expect(page.locator('#hotbar .hotbar-slot.selected:visible')).toHaveAttribute(
    'title',
    'Dirt',
  );
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

// BUGS.md #174 (owner, 2026-11-14): "I can't seem to access inventory, after opening bag, nothing is
// intractable." The touch layer is the whole screen; raised over the inventory so the bag button
// stays on top, it took every tap meant for the inventory. And closing it took the pointer lock,
// which sent the fingers to the canvas: no look after.
test('on a phone the bag opens an inventory that answers taps, and closes to working controls', async ({
  page,
}) => {
  test.setTimeout(90_000);
  await page.goto('./?world=flat');
  await expect
    .poll(async () => (await read(page))?.active ?? false, { timeout: 30_000 })
    .toBe(true);
  const tap = async (selector: string) => {
    const box = await page.locator(selector).first().boundingBox();
    if (!box) throw new Error(`${selector}: not on screen`);
    // A finger at the element's centre: whatever is on top there gets the tap.
    await page.touchscreen.tap(box.x + box.width / 2, box.y + box.height / 2);
  };
  await tap('#touch-bag');
  const inv = page.locator('#creative-inventory');
  await expect(inv).toBeVisible();
  // A slot of the inventory's hotbar is selected by a tap...
  await tap('#creative-inventory .inventory-hotbar [data-slot="3"]');
  await expect(inv.locator('.inventory-hotbar [data-slot="3"]')).toHaveClass(/selected/, {
    timeout: 5_000,
  });
  // ...and an item tapped goes into it.
  const label = await inv.locator('.inventory-items [data-item]').nth(4).getAttribute('title');
  await tap('#creative-inventory .inventory-items [data-item] >> nth=4');
  await expect(inv.locator('.inventory-hotbar [data-slot="3"]')).toHaveAttribute(
    'title',
    label ?? '',
    { timeout: 5_000 },
  );
  // The bag closes it again, and the controls are back and answer.
  await tap('#touch-bag');
  await expect(inv).toBeHidden();
  await expect(page.locator('#touch-jump')).toBeVisible();
  const before = await yaw(page);
  const cdp = await page.context().newCDPSession(page);
  await touch(cdp, 'touchStart', [{ x: 600, y: 200, id: 2 }]);
  for (let i = 1; i <= 8; i++) await touch(cdp, 'touchMove', [{ x: 600 - i * 15, y: 200, id: 2 }]);
  await touch(cdp, 'touchEnd', []);
  await expect
    .poll(async () => Math.abs((await yaw(page)) - before), { timeout: 5_000 })
    .toBeGreaterThan(1);
});

// Owner, 2026-11-15: "I'd love to copy a structure in mobile but there's no selection or copy
// control for it. Perhaps we could add a mobile button for it, but only when you're flying
// perhaps?" The region bar (ui/regionBar.ts, shared/input/touchRegion.json): Select shows while
// flying in creative; its corners, Copy, Paste (Place) and Undo run the region tools.
test('on a phone, flying, the region bar selects, copies, pastes and undoes', async ({ page }) => {
  test.setTimeout(120_000);
  await page.goto('./?world=flat');
  await expect
    .poll(
      () =>
        page.evaluate<boolean>(
          '!!(globalThis.__dwell && globalThis.__dwell.state()?.terrainReady)',
        ),
      { timeout: 30_000 },
    )
    .toBe(true);
  await untilTicking(page);
  const shots = process.env.DWELL_SHOTS;
  const shot = async (name: string) => {
    if (shots) await page.screenshot({ path: `${shots}/${name}.png` });
  };
  const tap = async (selector: string) => {
    const box = await page.locator(selector).boundingBox();
    if (!box) throw new Error(`${selector}: not on screen`);
    await page.touchscreen.tap(box.x + box.width / 2, box.y + box.height / 2);
  };
  type Cell = [number, number, number];
  const target = () =>
    page.evaluate<Cell | null>('globalThis.__dwell.state()?.target?.cell ?? null');
  const voxel = ([x, y, z]: Cell) =>
    page.evaluate<number>(`globalThis.__dwell.voxel(${String(x)}, ${String(y)}, ${String(z)})`);
  // The tools' line (RegionTools::status), as the bar shows it.
  const status = async () =>
    (await page.locator('#touch-region-bar .region-status').textContent()) ?? '';
  /** Turns the view and waits for the crosshair to land on a block other than `not`. */
  const aim = async (yawDeg: number, pitchDeg: number, not?: Cell): Promise<Cell> => {
    await page.evaluate(`globalThis.__dwell.look(${String(yawDeg)}, ${String(pitchDeg)})`);
    await expect
      .poll(async () => {
        const t = await target();
        return t !== null && t.join() !== not?.join();
      })
      .toBe(true);
    return (await target()) as Cell;
  };
  const cellsOf = (min: Cell, size: Cell): Cell[] => {
    const out: Cell[] = [];
    for (let y = 0; y < size[1]; y++)
      for (let z = 0; z < size[2]; z++)
        for (let x = 0; x < size[0]; x++) out.push([min[0] + x, min[1] + y, min[2] + z]);
    return out;
  };
  const read = (cells: Cell[]) => Promise.all(cells.map(voxel));

  // Walking: no Select. Flying (creative): Select, and its bar.
  await expect(page.locator('#touch-region')).toBeHidden();
  await page.evaluate('globalThis.__dwell.fly(true)');
  await expect(page.locator('#touch-region')).toBeVisible();
  const feet = await page.evaluate<Cell>('globalThis.__dwell.state().feet');
  /** Hovers `dx` m along x from the start, 1.5 m up: clear of the boxes edited, the ground in reach. */
  const hover = async (dx: number) => {
    const to: Cell = [feet[0] + dx, feet[1] + 1.5, feet[2]];
    await page.evaluate(`globalThis.__dwell.teleport([${to.join(', ')}])`);
    // On a slow page (CI's software renderer, since the near ring shrank and the screen opens
    // sooner) the player can drop from the point before its flight is back (BUGS.md #200's
    // follow-up): flight again once placed, and the ground below the point will do.
    await expect
      .poll(
        async () => {
          const s = await page.evaluate<{ feet: Cell; terrainReady: boolean } | null>(
            'globalThis.__dwell.state()',
          );
          if (s?.terrainReady) await page.evaluate('globalThis.__dwell.fly(true)');
          // At the point, or dropped from it to the ground below (still clear of the boxes, the
          // ground in reach).
          const dy = s ? s.feet[1] - to[1] : 9;
          return !!s?.terrainReady && Math.abs(s.feet[0] - to[0]) < 0.3 && dy > -1.6 && dy < 0.3;
        },
        { timeout: 30_000 },
      )
      .toBe(true);
  };
  await hover(0);
  await tap('#touch-region');
  await expect(page.locator('#touch-region-bar')).toBeVisible();
  await expect(page.locator('#region-place')).toBeHidden();
  await shot('region-bar-open');

  // Every button shown is on top where it is (nothing over it: the flight speed panel was), and
  // clear of the play buttons and the bag.
  const onTop = (id: string) =>
    page.evaluate<boolean>(`(() => {
      const b = document.getElementById('${id}').getBoundingClientRect();
      return document.elementFromPoint(b.x + b.width / 2, b.y + b.height / 2)?.id === '${id}';
    })()`);
  for (const id of ['touch-region', 'region-corner-a', 'region-corner-b', 'region-copy']) {
    expect(await onTop(id), id).toBe(true);
  }
  const barBox = await page.locator('#touch-region-bar').boundingBox();
  for (const other of ['.touch-buttons', '#touch-bag', '#flight-speed']) {
    const o = await page.locator(other).boundingBox();
    if (!barBox || !o) continue;
    const overlap =
      barBox.x < o.x + o.width &&
      o.x < barBox.x + barBox.width &&
      barBox.y < o.y + o.height &&
      o.y < barBox.y + barBox.height;
    expect(overlap, `the region bar overlaps ${other}`).toBe(false);
  }

  // Two corners on the ground ahead, then Copy.
  const a = await aim(0, -65);
  await tap('#region-corner-a');
  await expect.poll(status).toContain('Selection 1×1×1');
  const b = await aim(60, -50, a);
  await tap('#region-corner-b');
  const min: Cell = [Math.min(a[0], b[0]), Math.min(a[1], b[1]), Math.min(a[2], b[2])];
  const size: Cell = [
    Math.abs(a[0] - b[0]) + 1,
    Math.abs(a[1] - b[1]) + 1,
    Math.abs(a[2] - b[2]) + 1,
  ];
  await expect.poll(status).toContain(`Selection ${size.join('×')}`);
  const source = await read(cellsOf(min, size));
  expect(source.some((s) => s !== 0)).toBe(true);
  await shot('region-bar-select');
  await tap('#region-copy');
  await expect.poll(status).toContain('Copied');

  // Fly on 8 m, Paste: the ghost at the crosshair, then Place.
  await hover(8);
  await aim(0, -65);
  await tap('#region-paste');
  await expect(page.locator('#region-place')).toBeVisible();
  await expect(page.locator('#region-corner-a')).toBeHidden();
  const at = await page.evaluate<{ cell: Cell } | null>('globalThis.__dwell.placement()');
  if (!at) throw new Error('no placement');
  const p = at.cell;
  // The box stands on the placement cell, its bottom centre there (region_tools.rs `paste_box`).
  const pasteMin: Cell = [p[0] - Math.trunc(size[0] / 2), p[1], p[2] - Math.trunc(size[2] / 2)];
  const pasted = cellsOf(pasteMin, size);
  const before = await read(pasted);
  await shot('region-bar-paste');
  await tap('#region-place');
  await expect.poll(() => read(pasted), { timeout: 10_000 }).toEqual(source);

  // Cancel leaves paste mode; Undo takes the paste back.
  await tap('#region-cancel');
  await expect(page.locator('#region-corner-a')).toBeVisible();
  await tap('#region-undo');
  await expect.poll(() => read(pasted), { timeout: 10_000 }).toEqual(before);
  // Landing hides Select and closes the bar.
  await page.evaluate('globalThis.__dwell.fly(false)');
  await expect(page.locator('#touch-region')).toBeHidden();
  await expect(page.locator('#touch-region-bar')).toBeHidden();
});
