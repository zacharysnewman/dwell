// Phase 3d: breaking and placing blocks (ARCHITECTURE.md §6.5) — every block of the palette picked
// from the hotbar in local mode, and an edit by one client showing up for another on a native
// server.
import { readFileSync } from 'node:fs';
import { expect, test, type Page } from '@playwright/test';
import { PIECES, facingToward, pieceState } from '../src/interact/shapes';
import { stateId } from '../src/world/blocks';
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
    .toBeGreaterThan(100);
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
 * Looks at a point of `cell` (`height` up its side, so a slab or a slope's low part is hit too) and
 * waits for the cell to be targeted. On sloped ground a fixed pitch can pass under a block just
 * placed and target the slope below it instead.
 */
async function lookAt(page: Page, cell: Vec3, height = 0.25): Promise<void> {
  const f = await feet(page);
  const d = [cell[0] + 0.5 - f[0], cell[1] + height - f[1] - EYE_HEIGHT, cell[2] + 0.5 - f[2]];
  const yaw = (Math.atan2(d[0] ?? 0, d[2] ?? 0) * 180) / Math.PI;
  const pitch = (Math.atan2(d[1] ?? 0, Math.hypot(d[0] ?? 0, d[2] ?? 0)) * 180) / Math.PI;
  await call(page, `d.look(${String(yaw)}, ${String(pitch)})`);
  await expect.poll(async () => (await state(page))?.target?.cell.join() ?? '').toBe(cell.join());
}

/** The cardinal yaw (0, 90, 180, 270) facing most directly away from `other`. */
async function yawAwayFrom(page: Page, other: Page): Promise<number> {
  const [a, b] = await Promise.all([feet(page), feet(other)]);
  const yaw = (Math.atan2(a[0] - b[0], a[2] - b[2]) * 180) / Math.PI;
  return (((Math.round(yaw / 90) * 90) % 360) + 360) % 360;
}

test('local mode: every palette block can be picked from the hotbar, placed, and broken', async ({
  page,
}) => {
  test.setTimeout(120_000);
  await page.goto('./?world=flat');
  await ready(page);
  const slots = await page.locator('.hotbar-slot').count();
  expect(slots).toBe(13);
  const ground = await aim(page);
  expect(ground.face).toBe(2);
  const cell = add(ground.cell, FACE_DIRS[ground.face] ?? [0, 0, 0]);
  expect(await voxel(page, cell)).toBe(0);

  for (let slot = 0; slot < slots; slot++) {
    // Number keys pick the first ten slots, a tap on the hotbar the rest.
    if (slot < 10) await page.keyboard.press(`Digit${String((slot + 1) % 10)}`);
    else await page.locator(`.hotbar-slot[data-slot="${String(slot)}"]`).click();
    await expect(page.locator('.hotbar-slot.selected')).toHaveAttribute('data-slot', String(slot));

    await aim(page);
    expect(await call<boolean>(page, `d.edit('place')`)).toBe(true);
    await expect.poll(() => voxel(page, cell), { timeout: 5_000 }).not.toBe(0);
    const placed = await voxel(page, cell);
    // Breaking targets the new block itself.
    await expect.poll(async () => (await state(page))?.target?.cell.join() ?? '').toBe(cell.join());
    await page.waitForTimeout(120); // BLOCK_EDIT_INTERVAL_MS
    expect(await call<boolean>(page, `d.edit('break')`)).toBe(true);
    await expect.poll(() => voxel(page, cell), { timeout: 5_000 }).toBe(0);
    await page.waitForTimeout(120);
    expect(placed, `slot ${String(slot)}`).toBeGreaterThan(1);
  }
});

interface Placement {
  cell: Vec3;
  material: number;
}

/** Places and breaks every shape piece facing each way, checking the state the palette promised. */
async function placeEveryPiece(page: Page): Promise<void> {
  await page.keyboard.press('Digit1'); // stone: comes in every shape
  for (const piece of PIECES.slice(1)) {
    await call(page, `d.piece('${piece}')`);
    for (const yaw of [0, 90, 180, 270]) {
      const label = `${piece} looking ${String(yaw)}°`;
      await call(page, `d.look(${String(yaw)}, -45)`);
      await expect.poll(async () => (await state(page))?.target ?? null).not.toBeNull();
      // The slope rises away from the player, upright on the ground.
      const expected = pieceState('dwell:stone', piece, facingToward(yaw), 'bottom');
      await expect
        .poll(async () => (await call<Placement | null>(page, 'd.placement()'))?.material ?? -1, {
          message: label,
        })
        .toBe(expected);
      const placement = await call<Placement>(page, 'd.placement()');
      await page.waitForTimeout(120); // BLOCK_EDIT_INTERVAL_MS
      expect(await call<boolean>(page, `d.edit('place')`), label).toBe(true);
      await expect.poll(() => voxel(page, placement.cell), { message: label }).toBe(expected);
      // Breaking targets the new shape itself, and leaves air behind.
      await expect
        .poll(async () => (await state(page))?.target?.cell.join() ?? '', { message: label })
        .toBe(placement.cell.join());
      await page.waitForTimeout(120);
      expect(await call<boolean>(page, `d.edit('break')`), label).toBe(true);
      await expect.poll(() => voxel(page, placement.cell), { message: label }).toBe(0);
    }
  }
}

test('local mode: every shape in every facing is placed as promised, then broken', async ({
  page,
}) => {
  test.setTimeout(180_000);
  await page.goto('./?world=flat');
  await ready(page);
  await placeEveryPiece(page);
});

test('a placed slope shows as the same state to a second client on a native server', async ({
  browser,
}) => {
  test.setTimeout(120_000);
  const invite = readFileSync(INVITE_FILE, 'utf8');
  const contexts = [await browser.newContext(), await browser.newContext()];
  const [a, b] = await Promise.all(contexts.map((c) => c.newPage()));
  await a.goto(`./${invite}`);
  await b.goto(`./${invite}`);
  await ready(a);
  await ready(b);
  await a.keyboard.press('Digit1');
  // Players spawn side by side: build away from the other one (the server refuses a block placed
  // into a player).
  const yaw = await yawAwayFrom(a, b);
  for (const piece of ['wedge', 'inner', 'gentle_outer_high', 'slab'] as const) {
    await call(a, `d.piece('${piece}')`);
    await call(a, `d.look(${String(yaw)}, -45)`);
    await expect.poll(async () => (await state(a))?.target ?? null).not.toBeNull();
    const expected = pieceState('dwell:stone', piece, facingToward(yaw), 'bottom');
    await expect
      .poll(async () => (await call<Placement | null>(a, 'd.placement()'))?.material ?? -1)
      .toBe(expected);
    const placement = await call<Placement>(a, 'd.placement()');
    await a.waitForTimeout(120);
    expect(await call<boolean>(a, `d.edit('place')`)).toBe(true);
    await expect.poll(() => voxel(a, placement.cell), { timeout: 5_000 }).toBe(expected);
    await expect.poll(() => voxel(b, placement.cell), { timeout: 5_000 }).toBe(expected);
    await lookAt(a, placement.cell);
    await a.waitForTimeout(120);
    expect(await call<boolean>(a, `d.edit('break')`)).toBe(true);
    await expect.poll(() => voxel(b, placement.cell), { timeout: 5_000 }).toBe(0);
  }
  await Promise.all(contexts.map((c) => c.close()));
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
  await page.locator('.hotbar-slot[title="sandstone"]').click(); // not a fixed slot: the palette grows
  const sandstone = stateId('dwell:sandstone');
  expect(await call<boolean>(page, `d.edit('place')`)).toBe(true);
  await expect.poll(() => voxel(page, cell), { timeout: 5_000 }).toBe(sandstone);
  // Local worlds save every few seconds (and when the page is hidden).
  await page.waitForTimeout(6_000);
  await page.reload();
  await ready(page);
  await expect.poll(() => voxel(page, cell), { timeout: 10_000 }).toBe(sandstone);
  // A different seed is a different world file: untouched there.
  await page.goto('./?world=flat&seed=8');
  await ready(page);
  expect(await voxel(page, cell)).toBe(0);
});
