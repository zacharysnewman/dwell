import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { loadDir, registryFromVector, validate } from '../../../shared/structures/gen.mjs';
import { stateId } from '../world/blocks';
import {
  boxOf,
  bottomCentre,
  builtBounds,
  checkStructure,
  clipboardOf,
  parseStructure,
  scanBuilt,
  REGISTRY,
  structureFile,
  structureFromVoxels,
  structureName,
  structureText,
  type Vec3,
} from './structures';

const shared = (path: string) => new URL(`../../../shared/${path}`, import.meta.url);
/** The registry as CI's generator reads it (shared/blocks/vectors.txt). */
const vectorRegistry = registryFromVector(readFileSync(shared('blocks/vectors.txt'), 'utf8'));

const air = stateId('dwell:air');
const floor = stateId('dwell:sandbox_floor');
const stone = stateId('dwell:stone');
const sand = stateId('dwell:sand');
const ladder = stateId('dwell:ladder[facing=east,flooded=false]');

/** A 3 × 2 × 2 box: a stone post with a ladder, sand beside it, on the floor's top row. */
function sample(): { size: Vec3; cells: number[] } {
  // Layer order: y-slices, z-rows, x fastest.
  const size: Vec3 = [3, 2, 2];
  const cells = [
    // y = 0
    stone,
    ladder,
    floor,
    sand,
    air,
    air,
    // y = 1
    stone,
    air,
    air,
    air,
    air,
    air,
  ];
  return { size, cells };
}

/** Writes `files` into a fresh directory and validates it as gen.mjs does for shared/structures/. */
function generatorAccepts(files: Record<string, string>) {
  const dir = mkdtempSync(join(tmpdir(), 'dwell-structures-'));
  try {
    for (const [name, text] of Object.entries(files)) writeFileSync(join(dir, name), text);
    return loadDir(dir, vectorRegistry, { test: false });
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

describe('structures in the client', () => {
  it('reads the registry as the generator does', () => {
    expect(REGISTRY).toEqual(vectorRegistry);
  });

  it('turns voxels into a structure: kept air and floor, a character per state', () => {
    const { size, cells } = sample();
    const json = structureFromVoxels({ name: 'dwell:post', size, cells });
    expect(json).toEqual({
      format: 1,
      name: 'dwell:post',
      kind: 'voxels',
      size: [3, 2, 2],
      anchor: [1, 0, 1],
      palette: {
        '.': null,
        l: 'dwell:ladder[facing=east,flooded=false]',
        s: 'dwell:stone[flooded=false]',
        S: 'dwell:sand[flooded=false]',
      },
      layers: [
        ['sl.', 'S..'],
        ['s..', '...'],
      ],
    });
    const s = checkStructure(json, 'post.json');
    expect(s.size).toEqual([3, 2, 2]);
    expect(s.palette.get('l')).toBe('dwell:ladder[facing=east,flooded=false]');
  });

  it('exports files the generator accepts unchanged, and reads them back', () => {
    const { size, cells } = sample();
    const json = structureFromVoxels({ name: 'dwell:post', size, cells, anchor: [0, 0, 0] });
    const text = structureText(json);
    const [generated] = generatorAccepts({ [structureFile('dwell:post')]: text });
    expect(generated?.name).toBe('dwell:post');
    expect(generated?.cells).toBe('sl.S..s.....');
    expect(parseStructure(text, 'post.json').json).toEqual(json);
    // The committed structures print the same way they parse.
    for (const path of ['structures/playground.json', 'structures/test/ladder_post.json']) {
      const committed = JSON.parse(readFileSync(shared(path), 'utf8')) as Record<string, unknown>;
      expect(JSON.parse(structureText(committed))).toEqual(committed);
      expect(() => checkStructure(committed, path)).not.toThrow();
    }
  });

  it('prints long layers one row per line', () => {
    const size: Vec3 = [32, 1, 3];
    const json = structureFromVoxels({ name: 'dwell:wide', size, cells: Array(96).fill(stone) });
    const text = structureText(json);
    const row = `"${'s'.repeat(32)}"`;
    expect(text).toContain(
      `  "layers": [\n    [\n      ${row},\n      ${row},\n      ${row}\n    ]\n  ]`,
    );
    expect(generatorAccepts({ 'wide.json': text })).toHaveLength(1);
  });

  it('fails as the generator does on the same bad entries', () => {
    const { size, cells } = sample();
    const good = structureFromVoxels({ name: 'dwell:post', size, cells });
    const bad: [string, Record<string, unknown>][] = [
      [
        'palette."s": unknown block "dwell:nope"',
        { ...good, palette: { '.': null, s: 'dwell:nope', l: 'dwell:ladder', S: 'dwell:sand' } },
      ],
      ['size: must be [x, y, z], x and z 1..32, y 1..64', { ...good, size: [33, 2, 2] }],
      ['anchor: must be [x, y, z] inside the size', { ...good, anchor: [3, 0, 0] }],
      [
        'layers[0][1][0]: "?" is not in the palette',
        {
          ...good,
          layers: [
            ['sl.', '?..'],
            ['s..', '...'],
          ],
        },
      ],
      ['kind: unknown kind', { ...good, kind: 'curves' }],
    ];
    for (const [why, json] of bad) {
      expect(() => checkStructure(json, 'post.json')).toThrow(why);
      expect(() => validate(json, 'post.json', vectorRegistry)).toThrow(why);
    }
    expect(() =>
      structureFromVoxels({ name: 'dwell:big', size: [33, 1, 1], cells: Array(33).fill(stone) }),
    ).toThrow('at most 32');
  });

  it('ignores fields a newer build added, which the generator rejects', () => {
    const { size, cells } = sample();
    const json = { ...structureFromVoxels({ name: 'dwell:post', size, cells }), tags: ['tree'] };
    expect(() => checkStructure(json, 'post.json')).not.toThrow();
    expect(() => validate(json, 'post.json', vectorRegistry)).toThrow('tags: unknown field');
  });

  it('names structures in the dwell namespace by default', () => {
    expect(structureName('Oak Tree 2')).toEqual({ name: 'dwell:oak_tree_2' });
    expect(structureName('mine:Big-Rock')).toEqual({ name: 'mine:big_rock' });
    expect(structureName('  ')).toHaveProperty('error');
    expect(structureName('test:x')).toHaveProperty('error');
    expect(structureFile('mine:big_rock')).toBe('big_rock.json');
  });

  it('scans a region tile by tile for what was built, passing over tiles not loaded', () => {
    // A world: the floor below y = 0, stone at (-3, 0, 2) and (40, 5, 2), sand at (70, 0, 0) in a
    // chunk that isn't loaded.
    const at = (x: number, y: number, z: number) => {
      if (x === -3 && y === 0 && z === 2) return stone;
      if (x === 40 && y === 5 && z === 2) return stone;
      if (x === 70 && y === 0 && z === 0) return sand;
      return y < 0 ? floor : air;
    };
    const reads: Vec3[] = [];
    const read = (min: Vec3, size: Vec3) => {
      reads.push(size);
      if (min[0] >= 64) return [];
      const out: number[] = [];
      for (let y = 0; y < size[1]; y++)
        for (let z = 0; z < size[2]; z++)
          for (let x = 0; x < size[0]; x++) out.push(at(min[0] + x, min[1] + y, min[2] + z));
      return out;
    };
    expect(scanBuilt(read, [-10, -4, -4], [90, 20, 20])).toEqual({
      min: [-3, 0, 2],
      size: [44, 6, 1],
    });
    expect(reads.every((s) => s.every((n) => n >= 1 && n <= 32))).toBe(true);
    expect(scanBuilt(read, [-10, -4, -4], [5, 3, 3])).toBeNull();
  });

  it('turns a structure into clipboard cells and a keep mask', () => {
    const s = checkStructure(structureFromVoxels({ name: 'dwell:post', ...sample() }), 'post.json');
    const { cells, keep } = clipboardOf(s);
    expect([...cells]).toEqual([stone, ladder, 0, sand, 0, 0, stone, 0, 0, 0, 0, 0]);
    expect([...keep]).toEqual([0, 0, 1, 0, 1, 1, 0, 1, 1, 1, 1, 1]);
  });

  it('finds the box of what was built, and boxes between corners', () => {
    const size: Vec3 = [4, 3, 4];
    const cells = Array(48).fill(air);
    cells[(0 * 4 + 1) * 4 + 1] = stone; // (1, 0, 1)
    cells[(2 * 4 + 3) * 4 + 2] = sand; // (2, 2, 3)
    cells[0] = floor;
    const built = (s: number) => s !== air && s !== floor;
    expect(builtBounds(cells, size, built)).toEqual({ min: [1, 0, 1], size: [2, 3, 3] });
    expect(builtBounds(Array(48).fill(air), size, built)).toBeNull();
    expect(boxOf([5, 2, -1], [3, 4, -3])).toEqual({ min: [3, 2, -3], size: [3, 3, 3] });
    expect(bottomCentre([5, 4, 6])).toEqual([2, 0, 3]);
  });
});
