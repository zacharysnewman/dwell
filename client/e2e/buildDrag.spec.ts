// Drag building (docs/ITEMS.md §5): holding Place after the first block keeps building as the aim
// moves — a line straight out of the face first clicked, or (Flat) the cells the aim sweeps over at
// the first block's level. Local mode on the flat world, held through the test hook `holdPlace`
// (the right button needs the pointer locked, which a test page can't count on).
import { expect, test, type Page } from '@playwright/test';
import { CELL_HEIGHT } from '../src/protocol/constants.gen';
import { stateId } from '../src/world/blocks';

type Vec3 = [number, number, number];
interface DebugState {
  terrainReady: boolean;
  feet: Vec3;
  target: { cell: Vec3; face: number } | null;
}

const EYE_HEIGHT = 1.62;

function call<T>(page: Page, expr: string): Promise<T> {
  return page.evaluate<T>(`(() => { const d = globalThis.__dwell; return d ? ${expr} : null; })()`);
}
const state = (page: Page) => call<DebugState | null>(page, 'd.state()');
const voxel = (page: Page, c: Vec3) => call<number>(page, `d.voxel(${c.join(',')})`);

/** Looks at point `p` (m) from the player's eye. */
async function lookAtPoint(page: Page, p: Vec3): Promise<void> {
  const f = (await state(page))?.feet ?? [0, 0, 0];
  const d = [p[0] - f[0], p[1] - f[1] - EYE_HEIGHT, p[2] - f[2]] as const;
  const yaw = (Math.atan2(d[0], d[2]) * 180) / Math.PI;
  const pitch = (Math.atan2(d[1], Math.hypot(d[0], d[2])) * 180) / Math.PI;
  await call(page, `d.look(${String(yaw)}, ${String(pitch)})`);
}

/** The flat world, ready, with stone (a ½ m block, one cell) selected; the ground cell ahead. */
async function start(page: Page): Promise<Vec3> {
  await page.goto('./?world=flat');
  await expect
    .poll(async () => (await state(page))?.terrainReady ?? false, { timeout: 30_000 })
    .toBe(true);
  await call(page, 'd.select(0)');
  await call(page, 'd.look(0, -50)');
  await expect.poll(async () => (await state(page))?.target?.face ?? -1).toBe(2);
  const t = (await state(page))?.target;
  if (!t) throw new Error('nothing targeted');
  return t.cell;
}

test('held Place builds a line straight up out of the top face, and stops when let go', async ({
  page,
}) => {
  test.setTimeout(90_000);
  const [x, y, z] = await start(page);
  const stone = stateId('dwell:stone');
  expect(await call<boolean>(page, 'd.holdPlace(true)')).toBe(true);
  // The first piece is row y+1; looking up the column to the fifth piece's middle builds the
  // second to the fifth.
  const top = (y + 1) * CELL_HEIGHT;
  await lookAtPoint(page, [x + 0.5, top + 2.25, z + 0.5]);
  for (const row of [y + 2, y + 3, y + 4, y + 5])
    await expect.poll(() => voxel(page, [x, row, z]), { timeout: 10_000 }).toBe(stone);
  // Let go: aiming further up builds nothing more.
  await call(page, 'd.holdPlace(false)');
  await lookAtPoint(page, [x + 0.5, top + 3.25, z + 0.5]);
  await page.waitForTimeout(1_000);
  expect(await voxel(page, [x, y + 6, z])).toBe(0);
});

test('in Flat mode held Place paints the cells the aim sweeps over at the first level', async ({
  page,
}) => {
  test.setTimeout(90_000);
  const [x, y, z] = await start(page);
  const stone = stateId('dwell:stone');
  expect(await call<string>(page, 'd.buildMode()')).toBe('Flat');
  await expect(page.locator('#hotbar-build')).toHaveText('Flat');
  expect(await call<boolean>(page, 'd.holdPlace(true)')).toBe(true);
  // The level's plane is the first piece's middle, 0.25 m over the ground: sweep the aim along it
  // two cells to each side.
  const plane = (y + 1) * CELL_HEIGHT + 0.25;
  for (const dx of [1, 2, -1, -2]) {
    await lookAtPoint(page, [x + dx + 0.5, plane, z + 0.5]);
    await expect.poll(() => voxel(page, [x + dx, y + 1, z]), { timeout: 10_000 }).toBe(stone);
  }
  // At the level only: nothing above it.
  expect(await voxel(page, [x + 2, y + 2, z])).toBe(0);
  await call(page, 'd.holdPlace(false)');
});

/** A jump: Space held briefly, then the time to rise and land. */
async function jump(page: Page): Promise<void> {
  await call(page, 'd.press("Space", true)');
  await page.waitForTimeout(150);
  await call(page, 'd.press("Space", false)');
  await page.waitForTimeout(750);
}

test('towering: held Place while standing on the spot builds a block under the feet each jump', async ({
  page,
}) => {
  test.setTimeout(90_000);
  await start(page);
  await call(page, 'd.look(0, -89)');
  // The ground cell under the feet (the look before aimed at the ground ahead).
  const f = (await state(page))?.feet ?? [0, 0, 0];
  const under = [Math.floor(f[0]), Math.round(f[1] / CELL_HEIGHT) - 1, Math.floor(f[2])].join();
  await expect.poll(async () => (await state(page))?.target?.cell.join() ?? '').toBe(under);
  const feet0 = f[1];
  // On the ground the block would go where the body is: the press waits, held, for the jump.
  expect(await call<boolean>(page, 'd.holdPlace(true)')).toBe(true);
  for (let i = 0; i < 4; i++) await jump(page);
  // A metre a jump (the jump clears 1.25 m: two ½ m blocks), stood on each time.
  await expect.poll(async () => (await state(page))?.feet[1] ?? 0).toBeGreaterThan(feet0 + 3.99);
  await call(page, 'd.holdPlace(false)');
  await jump(page);
  expect((await state(page))?.feet[1] ?? 0).toBeLessThan(feet0 + 4.5);
});

test('walking back while holding Place in Line mode builds the line toward the player', async ({
  page,
}) => {
  test.setTimeout(90_000);
  const [x, y, z] = await start(page);
  const stone = stateId('dwell:stone');
  expect(await call<boolean>(page, `d.edit('place')`)).toBe(true);
  await expect.poll(() => voxel(page, [x, y + 1, z])).toBe(stone);
  // Aimed at the block's near side (−Z, the player's), low on its lower cell, and held: the line
  // runs out of that face toward the player as they walk back, the body kept clear of it.
  await lookAtPoint(page, [x + 0.5, (y + 1) * CELL_HEIGHT + 0.25, z + 0.02]);
  await expect.poll(async () => (await state(page))?.target?.face ?? -1).toBe(5);
  await page.waitForTimeout(150); // BLOCK_EDIT_INTERVAL_MS since the first place
  expect(await call<boolean>(page, 'd.holdPlace(true)')).toBe(true);
  await call(page, 'd.press("KeyS", true)');
  await page.waitForTimeout(1_500);
  await call(page, 'd.press("KeyS", false)');
  for (const dz of [1, 2, 3, 4])
    await expect.poll(() => voxel(page, [x, y + 1, z - dz]), { timeout: 10_000 }).toBe(stone);
  await call(page, 'd.holdPlace(false)');
});

/** Builds a tower 4 blocks high under the player (towering) and stands on it. */
async function tower(page: Page): Promise<number> {
  await start(page);
  await call(page, 'd.look(0, -89)');
  const f = (await state(page))?.feet ?? [0, 0, 0];
  const under = [Math.floor(f[0]), Math.round(f[1] / CELL_HEIGHT) - 1, Math.floor(f[2])].join();
  await expect.poll(async () => (await state(page))?.target?.cell.join() ?? '').toBe(under);
  expect(await call<boolean>(page, 'd.holdPlace(true)')).toBe(true);
  for (let i = 0; i < 4; i++) await jump(page);
  await call(page, 'd.holdPlace(false)');
  await expect.poll(async () => (await state(page))?.feet[1] ?? 0).toBeGreaterThan(f[1] + 3.99);
  return (await state(page))?.feet[1] ?? 0;
}

test('bridging help: Place over the edge with nothing targeted builds on ahead as the player walks', async ({
  page,
}) => {
  test.setTimeout(120_000);
  const top = await tower(page);
  const f = (await state(page))?.feet ?? [0, 0, 0];
  const [x, z] = [Math.floor(f[0]), Math.floor(f[2])];
  const row = Math.round(top / CELL_HEIGHT) - 1; // the tower's top cell, underfoot
  const stone = stateId('dwell:stone');
  // Looking ahead (+Z), at the horizon: nothing in reach is targeted.
  await call(page, 'd.look(0, 0)');
  await expect.poll(async () => (await state(page))?.target ?? null).toBeNull();
  expect(await call<boolean>(page, 'd.holdPlace(true)')).toBe(true);
  await expect.poll(() => voxel(page, [x, row, z + 1])).toBe(stone);
  // Walking ahead along it: it keeps a block ahead of the feet.
  await call(page, 'd.press("KeyW", true)');
  await page.waitForTimeout(1_200);
  await call(page, 'd.press("KeyW", false)');
  await call(page, 'd.holdPlace(false)');
  for (const dz of [2, 3, 4])
    await expect.poll(() => voxel(page, [x, row, z + dz]), { timeout: 10_000 }).toBe(stone);
  // Still up on it: walked along the bridge, not off it.
  expect((await state(page))?.feet[1] ?? 0).toBeGreaterThan(top - 0.1);
});

test('bridging help in Flat mode paints the floor at the level stood on, joined to it', async ({
  page,
}) => {
  test.setTimeout(120_000);
  const top = await tower(page);
  const f = (await state(page))?.feet ?? [0, 0, 0];
  const [x, z] = [Math.floor(f[0]), Math.floor(f[2])];
  const row = Math.round(top / CELL_HEIGHT) - 1;
  const stone = stateId('dwell:stone');
  expect(await call<string>(page, 'd.buildMode()')).toBe('Flat');
  // Down ahead past the tower's top, the ground far below out of reach: nothing targeted.
  await call(page, 'd.look(0, -30)');
  await expect.poll(async () => (await state(page))?.target ?? null).toBeNull();
  expect(await call<boolean>(page, 'd.holdPlace(true)')).toBe(true);
  // Swept along the level a few cells out, side to side.
  // The level's plane: the middle of the ½ m piece, level with the cell stood on.
  const level = (row + 0.5) * CELL_HEIGHT;
  for (const [dx, dz] of [
    [0, 3],
    [1, 3],
    [-1, 3],
  ] as const) {
    await lookAtPoint(page, [x + dx + 0.5, level, z + dz + 0.5]);
    await page.waitForTimeout(300); // frames to aim and build
  }
  await call(page, 'd.holdPlace(false)');
  for (const [dx, dz] of [
    [0, 1],
    [0, 2],
    [0, 3],
    [1, 3],
    [-1, 3],
  ] as const)
    await expect
      .poll(() => voxel(page, [x + dx, row, z + dz]), {
        message: `floor at ${String(dx)}, ${String(dz)}`,
        timeout: 10_000,
      })
      .toBe(stone);
});

test('a held ladder climbs the wall it is hung on, and stops where the wall ends', async ({
  page,
}) => {
  test.setTimeout(120_000);
  const [x, y, z] = await start(page);
  const stone = stateId('dwell:stone');
  // A stone pillar five cells up (Line out of the ground's top), then a step back from it.
  expect(await call<boolean>(page, 'd.holdPlace(true)')).toBe(true);
  const top = (y + 1) * CELL_HEIGHT;
  await lookAtPoint(page, [x + 0.5, top + 2.25, z + 0.5]);
  await expect.poll(() => voxel(page, [x, y + 5, z]), { timeout: 10_000 }).toBe(stone);
  await call(page, 'd.holdPlace(false)');
  await call(page, 'd.press("KeyS", true)');
  await page.waitForTimeout(300);
  await call(page, 'd.press("KeyS", false)');
  await page.waitForTimeout(300);
  // The ladder (its own drag: up or down only, whatever the mode) on the pillar's near side.
  await page.locator('.hotbar-slot[title="Ladder"]').click();
  await lookAtPoint(page, [x + 0.5, (y + 2.5) * CELL_HEIGHT, z + 0.02]);
  await expect.poll(async () => (await state(page))?.target?.face ?? -1).toBe(5);
  await page.waitForTimeout(150); // BLOCK_EDIT_INTERVAL_MS
  expect(await call<boolean>(page, 'd.holdPlace(true)')).toBe(true);
  const isLadder = async (row: number) => {
    const m = await voxel(page, [x, row, z - 1]);
    return m !== 0 && m !== stone;
  };
  await expect.poll(() => isLadder(y + 2)).toBe(true);
  // Up the ladder's column, past the pillar's top.
  for (const h of [3.5, 4.5, 5.5, 7.5]) {
    await lookAtPoint(page, [x + 0.5, (y + h) * CELL_HEIGHT, z - 0.5]);
    await page.waitForTimeout(300);
  }
  await call(page, 'd.holdPlace(false)');
  for (const row of [y + 3, y + 4, y + 5])
    await expect.poll(() => isLadder(row), { message: `ladder at ${String(row)}` }).toBe(true);
  // No wall behind the cell over the pillar: none there.
  await page.waitForTimeout(500);
  expect(await isLadder(y + 6)).toBe(false);
});
