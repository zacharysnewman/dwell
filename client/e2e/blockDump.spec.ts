// The block dump (debug tooling): the debug overlay's button copies the targeted block and its
// neighbours as JSON, with the world they came from.
import { expect, test, type Page } from '@playwright/test';
import { stateId } from '../src/world/blocks';

type Vec3 = [number, number, number];
const call = <T>(page: Page, expr: string): Promise<T> =>
  page.evaluate<T>(`(() => { const d = globalThis.__dwell; return d ? ${expr} : null; })()`);

test('the debug overlay copies the targeted block and its neighbours as JSON', async ({
  page,
  context,
}) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  await page.goto('./?world=flat&debug=1');
  await expect
    .poll(() => call<boolean>(page, 'd.state()?.terrainReady ?? false'), { timeout: 20_000 })
    .toBe(true);
  // A known block to find in the dump: stone, placed on the ground ahead, then targeted.
  await call(page, 'd.look(0, -45)');
  await expect.poll(() => call(page, 'd.state()?.target ?? null')).not.toBeNull();
  await page.keyboard.press('Digit1');
  expect(await call<boolean>(page, `d.edit('place')`)).toBe(true);
  const ground = await call<{ cell: Vec3 }>(page, 'd.state().target');
  const placed: Vec3 = [ground.cell[0], ground.cell[1] + 1, ground.cell[2]];
  await expect
    .poll(() => call<number>(page, `d.voxel(${placed.join(',')})`))
    .toBe(stateId('dwell:stone'));
  await expect
    .poll(async () => (await call<{ cell: Vec3 } | null>(page, 'd.state().target'))?.cell.join())
    .toBe(placed.join());

  await page.locator('#debug-copy').click();
  await expect(page.locator('#hud-note')).toHaveText('Block info copied');
  const dump = JSON.parse(await page.evaluate<string>('navigator.clipboard.readText()')) as {
    kind: string;
    world: { seed: string; generatorVersion: number };
    target: { cell: Vec3 };
    origin: Vec3;
    size: number;
    palette: string[];
    layers: number[][][];
  };
  expect(dump.kind).toBe('dwell-block-dump');
  expect(dump.world.generatorVersion).toBe(0); // the flat world
  expect(dump.target.cell).toEqual(placed);
  expect(dump.size).toBe(5);
  const centre = dump.layers[2]?.[2]?.[2] ?? -1;
  expect(dump.palette[centre]).toBe('dwell:stone');
});
