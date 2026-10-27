// Phase 3d: breaking and placing blocks (ARCHITECTURE.md §6.5) — every block of the palette picked
// from the hotbar in local mode, and an edit by one client showing up for another on a native
// server.
import { readFileSync } from 'node:fs';
import { expect, test, type Page } from '@playwright/test';
import { CELL_HEIGHT } from '../src/protocol/constants.gen';
import { STATE_DEFS, stateId } from '../src/world/blocks';
import { INVITE_FILE } from './global-setup';

type Vec3 = [number, number, number];
interface Target {
  cell: Vec3;
  face: number;
}
interface DebugState {
  terrainReady: boolean;
  terrain: { loaded: number; generating: number };
  target: Target | null;
  remotes: { feet: Vec3 }[];
}

const FACE_DIRS: Vec3[] = [
  [1, 0, 0],
  [-1, 0, 0],
  [0, 1, 0],
  [0, -1, 0],
  [0, 0, 1],
  [0, 0, -1],
];

/** Runs a call on the page's test hooks (window.__dwell, see src/main.ts). */
function call<T>(page: Page, expr: string): Promise<T> {
  return page.evaluate<T>(`(() => { const d = globalThis.__dwell; return d ? ${expr} : null; })()`);
}
const state = (page: Page) => call<DebugState | null>(page, 'd.state()');
const voxel = (page: Page, c: Vec3) => call<number>(page, `d.voxel(${c.join(',')})`);

async function ready(page: Page): Promise<void> {
  await expect
    .poll(async () => (await state(page))?.terrainReady ?? false, { timeout: 20_000 })
    .toBe(true);
  await expect
    .poll(
      async () => {
        const t = (await state(page))?.terrain;
        return t && t.generating === 0 ? t.loaded : 0;
      },
      { timeout: 30_000 },
    )
    .toBeGreaterThan(100)
    .catch(async (e: unknown) => {
      // Which wait it was: the chunks still generating, loaded and meshing.
      const s = await state(page).catch(() => null);
      console.log('terrain at the timeout', JSON.stringify(s?.terrain ?? null));
      throw e;
    });
}

/** Looks down ahead and waits for a block to be targeted. */
async function aim(page: Page, pitch = -45): Promise<Target> {
  await call(page, `d.look(0, ${String(pitch)})`);
  await expect.poll(async () => (await state(page))?.target ?? null).not.toBeNull();
  const t = (await state(page))?.target;
  if (!t) throw new Error('nothing targeted');
  return t;
}

const add = (a: Vec3, b: Vec3): Vec3 => [a[0] + b[0], a[1] + b[1], a[2] + b[2]];

const EYE_HEIGHT = 1.62;
const feet = (page: Page) => call<Vec3>(page, 'd.state().feet');

/**
 * Looks at a point of `cell`'s block, by the box it is aimed at by (`StateDef.selection`, the whole
 * cell for most blocks; a ladder's plate), `height` of the way up the box's side (so a ramp's low
 * part is hit too), and waits for the cell to be targeted. Cells are `CELL_HEIGHT` tall (½ m,
 * HALF_HEIGHT_GRID.md): a fixed pitch can pass over or under a block just placed. Face A (the
 * tests' worlds): the box is face-local.
 */
async function lookAt(page: Page, cell: Vec3, height = 0.5): Promise<void> {
  const f = await feet(page);
  const [x0, y0, z0, x1, y1, z1] = STATE_DEFS[await voxel(page, cell)]?.selection ?? [
    0, 0, 0, 1, 1, 1,
  ];
  const y = (cell[1] + y0 + (y1 - y0) * height) * CELL_HEIGHT;
  const d = [cell[0] + (x0 + x1) / 2 - f[0], y - f[1] - EYE_HEIGHT, cell[2] + (z0 + z1) / 2 - f[2]];
  const yaw = (Math.atan2(d[0] ?? 0, d[2] ?? 0) * 180) / Math.PI;
  const pitch = (Math.atan2(d[1] ?? 0, Math.hypot(d[0] ?? 0, d[2] ?? 0)) * 180) / Math.PI;
  await call(page, `d.look(${String(yaw)}, ${String(pitch)})`);
  await expect.poll(async () => (await state(page))?.target?.cell.join() ?? '').toBe(cell.join());
}

/**
 * The cardinal yaw (0, 90, 180, 270) whose placement, looking 45° down, lands farthest from every
 * other player the page sees (the server refuses a block placed into a player, and players from
 * earlier tests may still stand around the spawn).
 */
async function clearYaw(page: Page): Promise<number> {
  let best = 0;
  let bestDistance = -1;
  for (const yaw of [0, 90, 180, 270]) {
    await call(page, `d.look(${String(yaw)}, -45)`);
    await expect.poll(async () => (await state(page))?.target ?? null).not.toBeNull();
    const placement = await call<Placement>(page, 'd.placement()');
    const remotes = (await state(page))?.remotes ?? [];
    const [x, , z] = placement.cell;
    const distance = Math.min(
      ...remotes.map((r) => Math.hypot(x + 0.5 - r.feet[0], z + 0.5 - r.feet[2])),
    );
    if (distance > bestDistance) {
      best = yaw;
      bestDistance = distance;
    }
  }
  return best;
}

test('local mode: every palette block can be picked from the hotbar, placed, and broken', async ({
  page,
}) => {
  test.setTimeout(120_000);
  await page.goto('./?world=flat');
  await ready(page);
  // The hotbar is built once the page has taken the join, which can be a frame after the terrain.
  // One slot per item (shared/items/items.json, docs/ITEMS.md).
  await expect(page.locator('.hotbar-slot')).toHaveCount(16);
  const slots = await page.locator('.hotbar-slot').count();
  const ground = await aim(page);
  expect(ground.face).toBe(2);
  const cell = add(ground.cell, FACE_DIRS[ground.face] ?? [0, 0, 0]);
  expect(await voxel(page, cell)).toBe(0);

  for (let slot = 0; slot < slots; slot++) {
    // Number keys pick the first ten slots, a tap on the hotbar the rest.
    if (slot < 10) await page.keyboard.press(`Digit${String((slot + 1) % 10)}`);
    else await page.locator(`.hotbar-slot[data-slot="${String(slot)}"]`).click();
    await expect(page.locator('.hotbar-slot.selected')).toHaveAttribute('data-slot', String(slot));
    // A ladder hangs on a block's side, never on the ground's top (docs/ITEMS.md; the placement
    // rules' tests, dwell-world tests/placement.rs, hang it).
    const title = await page
      .locator(`.hotbar-slot[data-slot="${String(slot)}"]`)
      .getAttribute('title');
    if (title === 'Ladder') continue;

    // Back to the ground cell, and wait until it is the target: the target is the previous look's
    // (aimed at the block just broken, so past it now) until the next frame re-targets.
    await call(page, 'd.look(0, -45)');
    await expect
      .poll(async () => {
        const t = (await state(page))?.target;
        return t ? `${t.cell.join()}/${String(t.face)}` : '';
      })
      .toBe(`${ground.cell.join()}/${String(ground.face)}`);
    expect(await call<boolean>(page, `d.edit('place')`)).toBe(true);
    await expect
      .poll(() => voxel(page, cell), { message: `slot ${String(slot)} placed`, timeout: 5_000 })
      .not.toBe(0);
    const placed = await voxel(page, cell);
    // Breaking targets the new block itself: looked at, as tall grass is aimed at by a box
    // smaller than its cell.
    await lookAt(page, cell);
    await page.waitForTimeout(120); // BLOCK_EDIT_INTERVAL_MS
    expect(await call<boolean>(page, `d.edit('break')`)).toBe(true);
    await expect.poll(() => voxel(page, cell), { timeout: 5_000 }).toBe(0);
    await page.waitForTimeout(120);
    expect(placed, `slot ${String(slot)}`).toBeGreaterThan(0);
  }
});

/**
 * How long an edit may take to show on both pages. The server answers within milliseconds, but
 * every check waits for the page's main thread, and under software rendering (SwiftShader) a frame
 * of the first seconds of play has taken 3–4.7 s (measured with the CPU throttled 4×).
 */
const EDIT_ARRIVES_MS = 20_000;

interface Placement {
  cell: Vec3;
  material: number;
}

test('a placed block shows as the same state to a second client on a native server', async ({
  browser,
}) => {
  test.setTimeout(180_000);
  const invite = readFileSync(INVITE_FILE, 'utf8');
  const contexts = [await browser.newContext(), await browser.newContext()];
  try {
    const [a, b] = await Promise.all(contexts.map((c) => c.newPage()));
    await a.goto(`./${invite}`);
    await b.goto(`./${invite}`);
    await ready(a);
    await ready(b);
    await expect.poll(async () => (await state(a))?.remotes.length ?? 0).toBeGreaterThan(0);
    // Players spawn side by side: build where nobody stands.
    const yaw = await clearYaw(a);
    for (const key of ['Digit1', 'Digit5'] as const) {
      await a.keyboard.press(key); // stone, then sand
      await call(a, `d.look(${String(yaw)}, -45)`);
      await expect.poll(async () => (await state(a))?.target ?? null).not.toBeNull();
      const placement = await call<Placement>(a, 'd.placement()');
      const expected = placement.material;
      // edit() refuses (and sends nothing) without a target or within BLOCK_EDIT_INTERVAL_MS.
      await expect.poll(() => call<boolean>(a, `d.edit('place')`)).toBe(true);
      await expect
        .poll(() => voxel(a, placement.cell), { timeout: EDIT_ARRIVES_MS })
        .toBe(expected);
      await expect
        .poll(() => voxel(b, placement.cell), { timeout: EDIT_ARRIVES_MS })
        .toBe(expected);
      await lookAt(a, placement.cell);
      await expect.poll(() => call<boolean>(a, `d.edit('break')`)).toBe(true);
      await expect.poll(() => voxel(b, placement.cell), { timeout: EDIT_ARRIVES_MS }).toBe(0);
      // And on the builder's page, or its next aim still finds the block and places beside it.
      await expect.poll(() => voxel(a, placement.cell), { timeout: EDIT_ARRIVES_MS }).toBe(0);
    }
  } finally {
    await Promise.all(contexts.map((c) => c.close()));
  }
});

test('an edit by one client appears for another on a native server', async ({ browser }) => {
  test.setTimeout(120_000);
  const invite = readFileSync(INVITE_FILE, 'utf8');
  const contexts = [await browser.newContext(), await browser.newContext()];
  const [a, b] = await Promise.all(contexts.map((c) => c.newPage()));
  await a.goto(`./${invite}`);
  await b.goto(`./${invite}`);
  await ready(a);
  await ready(b);

  const target = await aim(a);
  const cell = add(target.cell, FACE_DIRS[target.face] ?? [0, 0, 0]);
  await a.keyboard.press('Digit1'); // stone
  expect(await call<boolean>(a, `d.edit('place')`)).toBe(true);
  await expect.poll(() => voxel(a, cell), { timeout: 5_000 }).toBe(stateId('dwell:stone'));
  await expect.poll(() => voxel(b, cell), { timeout: 5_000 }).toBe(stateId('dwell:stone'));

  await a.waitForTimeout(120);
  await lookAt(a, cell, 0.5);
  expect(await call<boolean>(a, `d.edit('break')`)).toBe(true);
  await expect.poll(() => voxel(b, cell), { timeout: 5_000 }).toBe(0);
  // Closed so the pages stop rendering before later tests run.
  await Promise.all(contexts.map((c) => c.close()));
});

test('local mode: an edited world is saved in the browser and survives a reload', async ({
  page,
}) => {
  test.setTimeout(90_000);
  await page.goto('./?world=flat&seed=7');
  await ready(page);
  const target = await aim(page);
  const cell = add(target.cell, FACE_DIRS[target.face] ?? [0, 0, 0]);
  await page.locator('.hotbar-slot[title="sand"]').click(); // not a fixed slot: the palette grows
  const sand = stateId('dwell:sand');
  expect(await call<boolean>(page, `d.edit('place')`)).toBe(true);
  await expect.poll(() => voxel(page, cell), { timeout: 5_000 }).toBe(sand);
  // Local worlds save every few seconds (and when the page is hidden).
  await page.waitForTimeout(6_000);
  await page.reload();
  await ready(page);
  await expect.poll(() => voxel(page, cell), { timeout: 10_000 }).toBe(sand);
  // A different seed is a different world file: untouched there.
  await page.goto('./?world=flat&seed=8');
  await ready(page);
  expect(await voxel(page, cell)).toBe(0);
});
