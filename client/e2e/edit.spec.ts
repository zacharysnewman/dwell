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
    // 60 s: on CI's runners the native server's suite world (the generated terrain, its cliffs
    // ~4x slower to generate since 0.35.0; CLAUDE.md) has taken over 20 s to stream in.
    .poll(async () => (await state(page))?.terrainReady ?? false, { timeout: 60_000 })
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
/** The creative items (shared/items/items.json), in its order. */
const ITEMS = (
  JSON.parse(readFileSync(new URL('../../shared/items/items.json', import.meta.url), 'utf8')) as {
    items: { name: string; label: string; tool?: string }[];
  }
).items;
/** The hotbar's tools (shared/items/items.json `tool`): they place nothing. */
const TOOL_TITLES = new Set(ITEMS.filter((i) => i.tool !== undefined).map((i) => i.label));

const EYE_HEIGHT = 1.62;

/**
 * How long the page may take to show a look, a key or an edit. The game answers within a frame,
 * but on the software renderer (SwiftShader, the suite's CI adapter) the page has drawn no frame
 * for 6 s at a time mid-test (measured locally with the CPU throttled 6×: just after the first
 * torch is placed; BUGS.md #49 is the same stall after a join), and a look, a hotbar key (taken in
 * the frame) or a re-target waits for the next frame; the expect default of 5 s failed there on CI.
 */
const FRAME_STALL_MS = 20_000;
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
  const [x, z] = [cell[0] + (x0 + x1) / 2, cell[2] + (z0 + z1) / 2];
  const d = [x - f[0], y - f[1] - EYE_HEIGHT, z - f[2]];
  const yaw = (Math.atan2(d[0] ?? 0, d[2] ?? 0) * 180) / Math.PI;
  const pitch = (Math.atan2(d[1] ?? 0, Math.hypot(d[0] ?? 0, d[2] ?? 0)) * 180) / Math.PI;
  await call(page, `d.look(${String(yaw)}, ${String(pitch)})`);
  await expect
    .poll(async () => (await state(page))?.target?.cell.join() ?? '', { timeout: FRAME_STALL_MS })
    .toBe(cell.join());
}

/**
 * How long an edit may take to show on both pages. The server answers within milliseconds, but
 * every check waits for the page's main thread, and under software rendering (SwiftShader) a frame
 * of the first seconds of play has taken 3–4.7 s (measured with the CPU throttled 4×).
 */
const EDIT_ARRIVES_MS = 20_000;

/**
 * Breaks the block placed at `cell`: a break takes one ½ m cell (docs/ITEMS.md §2), so a taller
 * piece (an item of two cells, none on the hotbar today) goes in two breaks, its upper cell first
 * (a look from above meets it first), each hit until it breaks (block damage). Each waits until
 * `pages` all show the cell empty.
 */
async function breakBlock(page: Page, cell: Vec3, pages: Page[] = [page]): Promise<void> {
  const upper: Vec3 = [cell[0], cell[1] + 1, cell[2]];
  const cells = (await voxel(page, upper)) === (await voxel(page, cell)) ? [upper, cell] : [cell];
  for (const c of cells) {
    await lookAt(page, c);
    // Blocks take several hits by the tool held (docs/BLOCK_DAMAGE.md: 11 by hand for soil):
    // hit until it breaks. edit() refuses (and sends nothing) within BLOCK_EDIT_INTERVAL_MS of
    // the last edit.
    await expect
      .poll(
        async () => {
          await call<boolean>(page, `d.edit('break')`);
          return voxel(page, c);
        },
        { timeout: EDIT_ARRIVES_MS, intervals: [150] },
      )
      .toBe(0);
    for (const p of pages) {
      await expect.poll(() => voxel(p, c), { timeout: EDIT_ARRIVES_MS }).toBe(0);
    }
  }
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

test('local mode: the creative inventory view puts an item on the 6-slot hotbar', async ({
  page,
}) => {
  test.setTimeout(90_000);
  await page.goto('./?world=flat&fresh=1');
  await ready(page);
  // Six slots (shared/inventory/inventory.json hotbarSlots), a new world's picks first.
  await expect(page.locator('#hotbar .hotbar-slot:visible')).toHaveCount(6);
  await page.keyboard.press('Digit3');
  await expect(page.locator('#hotbar .hotbar-slot.selected:visible')).toHaveAttribute(
    'data-slot',
    '2',
    { timeout: FRAME_STALL_MS },
  );
  await page.keyboard.press('KeyE');
  const view = page.locator('#creative-inventory');
  await expect(view).toBeVisible();
  // Every creative item is in the catalogue.
  await expect(view.locator('.inventory-items [data-item]')).toHaveCount(ITEMS.length);
  await view.locator('[data-item="dwell:sand"]').click();
  await page.keyboard.press('Escape');
  await expect(view).toBeHidden();
  await expect(page.locator('#hotbar .hotbar-slot[data-slot="2"]:visible')).toHaveAttribute(
    'title',
    'Sand',
  );
});

test('local mode: every palette block can be picked from the hotbar, placed, and broken', async ({
  page,
}) => {
  // 24 items (the six tools place nothing), each put on the hotbar, placed and broken: 66-114 s on CI's software renderer when it passed.
  test.setTimeout(180_000);
  await page.goto('./?world=flat');
  await ready(page);
  // The hotbar is built once the page has taken the join, which can be a frame after the terrain.
  // Six slots (shared/inventory/inventory.json); every item reaches one by the inventory view.
  await expect(page.locator('#hotbar .hotbar-slot:visible')).toHaveCount(6);
  const ground = await aim(page);
  expect(ground.face).toBe(2);
  const cell = add(ground.cell, FACE_DIRS[ground.face] ?? [0, 0, 0]);
  expect(await voxel(page, cell)).toBe(0);

  for (const [i, item] of ITEMS.entries()) {
    // Number keys pick the six slots; the item goes in the selected one (docs/ITEMS.md §6a).
    const slot = i % 6;
    await page.keyboard.press(`Digit${String(slot + 1)}`);
    await expect(page.locator('#hotbar .hotbar-slot.selected:visible')).toHaveAttribute(
      'data-slot',
      String(slot),
      { timeout: FRAME_STALL_MS },
    );
    expect(await call<boolean>(page, `d.pick(${JSON.stringify(item.name)})`)).toBe(true);
    // A ladder hangs on a block's side, never on the ground's top (docs/ITEMS.md; the placement
    // rules' tests, dwell-world tests/placement.rs, hang it).
    const title = await page
      .locator(`#hotbar .hotbar-slot[data-slot="${String(slot)}"]:visible`)
      .getAttribute('title');
    if (title === 'Ladder') continue;
    // A tool places nothing: held, Place turns the axe's swing (docs/adr/0067-stone-axe-and-chopping.md).
    if (title !== null && TOOL_TITLES.has(title)) continue;
    // Leaves placed on the ground stay there (docs/FELLING.md §2: loose leaves fall only to land).

    // Back to the ground cell, and wait until it is the target: the target is the previous look's
    // (aimed at the block just broken, so past it now) until the next frame re-targets.
    await call(page, 'd.look(0, -45)');
    await expect
      .poll(
        async () => {
          const t = (await state(page))?.target;
          return t ? `${t.cell.join()}/${String(t.face)}` : '';
        },
        { timeout: FRAME_STALL_MS },
      )
      .toBe(`${ground.cell.join()}/${String(ground.face)}`);
    expect(await call<boolean>(page, `d.edit('place')`)).toBe(true);
    await expect
      .poll(() => voxel(page, cell), {
        message: `slot ${String(slot)} placed`,
        timeout: FRAME_STALL_MS,
      })
      .not.toBe(0);
    const placed = await voxel(page, cell);
    // Breaking targets the new block itself: looked at, as tall grass is aimed at by a box
    // smaller than its cell. Stone can't be broken by hand (docs/BLOCK_DAMAGE.md): a pickaxe.
    if (item.name.startsWith('dwell:stone')) {
      expect(await call<boolean>(page, 'd.pick("dwell:stone_pickaxe")')).toBe(true);
    }
    await breakBlock(page, cell);
    await page.waitForTimeout(120); // BLOCK_EDIT_INTERVAL_MS
    // A steep slope's upper half (a piece of two cells) is broken on its own.
    const above = add(cell, [0, 1, 0]);
    if ((await voxel(page, above)) !== 0) {
      await breakBlock(page, above);
      await page.waitForTimeout(120);
    }
    expect(placed, `slot ${String(slot)}`).toBeGreaterThan(0);
  }
});

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
      // Gone on both pages: on the builder's too, or its next aim still finds the block and
      // places beside it.
      await breakBlock(a, placement.cell, [b, a]);
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

  await breakBlock(a, cell, [b]);
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
  expect(await call<boolean>(page, 'd.pick("dwell:sand")')).toBe(true); // into the selected slot (docs/ITEMS.md §6a)
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
