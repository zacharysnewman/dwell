import { describe, expect, it } from 'vitest';
import { stateId } from '../world/blocks';
import { FACE_DIRS, progressText, reachTo, StructureStamp, type StampWorld } from './structureStamp';
import { checkStructure, type Vec3 } from './structures';

const air = stateId('dwell:air');
const floor = stateId('dwell:sandbox_floor');
const stone = stateId('dwell:stone');
const sand = stateId('dwell:sand');

/**
 * A world with a floor at y < 0 that applies edits as the server would for these tests: a place
 * needs a targetable cell against an empty one, a break a breakable one; `refuse` drops edits.
 */
function fakeWorld(eye: Vec3 | null): StampWorld & {
  cells: Map<string, number>;
  sent: { action: string; cell: Vec3; face: number; material: number }[];
  eyeAt: Vec3 | null;
  refuse: boolean;
} {
  const key = (x: number, y: number, z: number) => `${String(x)},${String(y)},${String(z)}`;
  const w = {
    cells: new Map<string, number>(),
    sent: [] as { action: string; cell: Vec3; face: number; material: number }[],
    eyeAt: eye,
    refuse: false,
    last: -Infinity,
    voxel(x: number, y: number, z: number) {
      return w.cells.get(key(x, y, z)) ?? (y < 0 ? floor : air);
    },
    eye: () => w.eyeAt,
    editAt(action: 'break' | 'place', cell: Vec3, face: number, material: number, now: number) {
      if (now - w.last < 100) return false;
      w.last = now;
      w.sent.push({ action, cell, face, material });
      if (w.refuse) return true;
      const v = w.voxel(...cell);
      if (action === 'break') {
        if (v !== floor) w.cells.set(key(...cell), air);
      } else {
        const d = FACE_DIRS[face] ?? [0, 0, 0];
        const to: Vec3 = [cell[0] + d[0], cell[1] + d[1], cell[2] + d[2]];
        if (v !== air && w.voxel(...to) === air) w.cells.set(key(...to), material);
      }
      return true;
    },
  };
  return w;
}

/** A 1 × 3 × 1 column: stone, sand, explicit air at the top; anchored at its bottom. */
const column = checkStructure(
  {
    format: 1,
    name: 'dwell:column',
    kind: 'voxels',
    size: [1, 3, 1],
    anchor: [0, 0, 0],
    palette: { s: 'dwell:stone', a: 'dwell:sand', _: 'dwell:air' },
    layers: [['s'], ['a'], ['_']],
  },
  'column.json',
);

function run(stamp: StructureStamp, from: number, to: number) {
  let p = stamp.step(from);
  for (let t = from + 20; t <= to; t += 20) p = stamp.step(t);
  return p;
}

describe('structure stamp', () => {
  it('builds bottom up against blocks already there, at the edit rate', () => {
    const world = fakeWorld([2.5, 3, 0.5]);
    world.cells.set('0,2,0', stone); // in the way of the air cell: broken
    const stamp = new StructureStamp(column, [0, 0, 0], world);
    const p = run(stamp, 0, 1000);
    expect(world.voxel(0, 0, 0)).toBe(stone);
    expect(world.voxel(0, 1, 0)).toBe(sand);
    expect(world.voxel(0, 2, 0)).toBe(air);
    expect(p).toMatchObject({ total: 3, done: 3, failed: 0, waiting: 0, settled: true });
    expect(progressText(p)).toBe('Loaded: 3 of 3 blocks');
    // The first place is against the floor's top face (+Y), the next against the stone.
    expect(world.sent[0]).toMatchObject({ action: 'break', cell: [0, 2, 0] });
    expect(world.sent).toContainEqual({ action: 'place', cell: [0, -1, 0], face: 2, material: stone });
    expect(world.sent).toContainEqual({ action: 'place', cell: [0, 0, 0], face: 2, material: sand });
    expect(world.sent).toHaveLength(3);
  });

  it('waits for cells out of reach, then goes on once the player is near', () => {
    const world = fakeWorld([40.5, 3, 0.5]);
    const stamp = new StructureStamp(column, [0, 0, 0], world);
    let p = run(stamp, 0, 500);
    expect(world.sent).toHaveLength(0);
    expect(p).toMatchObject({ done: 1, waiting: 2, settled: true }); // the air cell already is
    expect(progressText(p)).toContain('move closer');
    world.eyeAt = [2.5, 3, 0.5];
    p = run(stamp, 600, 1500);
    expect(p).toMatchObject({ done: 3, waiting: 0 });
  });

  it('gives a cell up after the server refuses it a few times, and skips unplaceable blocks', () => {
    const world = fakeWorld([2.5, 3, 0.5]);
    world.refuse = true;
    const s = checkStructure(
      {
        format: 1,
        name: 'dwell:odd',
        kind: 'voxels',
        size: [2, 1, 1],
        anchor: [0, 0, 0],
        palette: { s: 'dwell:stone', f: 'dwell:sandbox_floor' },
        layers: [['sf']],
      },
      'odd.json',
    );
    const stamp = new StructureStamp(s, [0, 0, 0], world, { confirmMs: 200, attempts: 2 });
    const p = run(stamp, 0, 2000);
    expect(p).toMatchObject({ total: 2, done: 0, failed: 1, skipped: 1, settled: true });
    expect(world.sent).toHaveLength(2);
    expect(progressText(p)).toBe("Loaded: 0 of 2 blocks · 1 refused · 1 can't be placed by hand");
  });

  it('sends nothing once cancelled or while not playing', () => {
    const world = fakeWorld(null);
    const stamp = new StructureStamp(column, [0, 0, 0], world);
    run(stamp, 0, 500);
    world.eyeAt = [2.5, 3, 0.5];
    stamp.cancel();
    run(stamp, 600, 1500);
    expect(world.sent).toHaveLength(0);
  });

  it('measures reach to a cell’s box', () => {
    expect(reachTo([0.5, 0.5, 0.5], [0, 0, 0])).toBe(0);
    expect(reachTo([3, 0.5, 0.5], [0, 0, 0])).toBe(2);
    expect(reachTo([4, 5, 0.5], [0, 0, 0])).toBe(5);
  });
});
