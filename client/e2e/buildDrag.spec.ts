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

/** The flat world, ready, with stone (a 1 m block, two cells) selected; the ground cell ahead. */
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
  // The first piece is rows y+1, y+2; looking up the column to the third piece's middle builds
  // the second and third.
  const top = (y + 1) * CELL_HEIGHT;
  await lookAtPoint(page, [x + 0.5, top + 2.5, z + 0.5]);
  for (const row of [y + 3, y + 4, y + 5, y + 6])
    await expect.poll(() => voxel(page, [x, row, z]), { timeout: 10_000 }).toBe(stone);
  // Let go: aiming further up builds nothing more.
  await call(page, 'd.holdPlace(false)');
  await lookAtPoint(page, [x + 0.5, top + 3.5, z + 0.5]);
  await page.waitForTimeout(1_000);
  expect(await voxel(page, [x, y + 7, z])).toBe(0);
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
  // The level's plane is the first piece's middle, 0.5 m over the ground: sweep the aim along it
  // two cells to each side.
  const plane = (y + 1) * CELL_HEIGHT + 0.5;
  for (const dx of [1, 2, -1, -2]) {
    await lookAtPoint(page, [x + dx + 0.5, plane, z + 0.5]);
    for (const row of [y + 1, y + 2])
      await expect.poll(() => voxel(page, [x + dx, row, z]), { timeout: 10_000 }).toBe(stone);
  }
  // At the level only: nothing above it.
  expect(await voxel(page, [x + 2, y + 3, z])).toBe(0);
  await call(page, 'd.holdPlace(false)');
});
