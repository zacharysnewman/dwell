import { describe, expect, it } from 'vitest';
import data from '../../../shared/structures/library.json';
import { stateId } from '../world/blocks';
import { checkLimits, LIMITS, maxSize, tilesOf } from './structureLibrary';
import { StructureStore, type StructureDir } from './structureStore';
import {
  checkStructure,
  clipboardOf,
  readTiled,
  structureFromVoxels,
  structureText,
  type StructureJson,
  type Vec3,
} from './structures';

function memoryDir(): StructureDir & { files: Map<string, string> } {
  const files = new Map<string, string>();
  return {
    files,
    names: () => Promise.resolve([...files.keys()]),
    read: (f) => Promise.resolve(files.get(f) ?? null),
    write: (f, text) => {
      files.set(f, text);
      return Promise.resolve();
    },
    remove: (f) => {
      files.delete(f);
      return Promise.resolve();
    },
  };
}

const stone = stateId('dwell:stone');
const sand = stateId('dwell:sand');
const air = stateId('dwell:air');

/** A 70 × 40 × 33 box: stone floor, sand at the far corner, air elsewhere. */
function bigCells(size: Vec3): Uint16Array {
  const [sx, sy, sz] = size;
  const cells = new Uint16Array(sx * sy * sz).fill(air);
  for (let z = 0; z < sz; z++) for (let x = 0; x < sx; x++) cells[z * sx + x] = stone;
  cells[((sy - 1) * sz + (sz - 1)) * sx + (sx - 1)] = sand;
  return cells;
}

describe('the structure library (STRUCTURES.md §9)', () => {
  it('its limits are data, checked with named errors', () => {
    expect(LIMITS).toEqual(checkLimits(data));
    expect(LIMITS.tileSize).toBeLessThanOrEqual(32);
    expect(() => checkLimits({ ...data, tileSize: 33 })).toThrow('library.json: tileSize');
    expect(() => checkLimits({ ...data, maxTiles: [1, 2] })).toThrow('maxTiles');
    expect(() => checkLimits({ ...data, extra: 1 })).toThrow('extra: unknown field');
    // A fixture limit by data alone: 2 tiles of 8 → 16 across.
    const fixture = checkLimits({ tileSize: 8, maxTiles: [2, 1, 2], maxNameLength: 20 });
    expect(maxSize(fixture)).toEqual([16, 8, 16]);
  });

  it('cuts a box into tiles of at most the tile side from its lowest corner', () => {
    const tiles = tilesOf([70, 40, 33], 32);
    // 3 across × 2 up × 2 deep.
    expect(tiles).toHaveLength(12);
    expect(tiles[0]).toEqual({ at: [0, 0, 0], size: [32, 32, 32] });
    expect(tiles[2]).toEqual({ at: [64, 0, 0], size: [6, 32, 32] });
    expect(tiles.at(-1)).toEqual({ at: [64, 32, 32], size: [6, 8, 1] });
    for (const t of tiles) expect(Math.max(...t.size)).toBeLessThanOrEqual(32);
  });

  it('saves a selection larger than 32³ as tiles and loads it back cell for cell', () => {
    const size: Vec3 = [70, 40, 33];
    const cells = bigCells(size);
    const json = structureFromVoxels({ name: 'dwell:hall', size, cells });
    expect(json.kind).toBe('tiles');
    expect(json.tileSize).toBe(32);
    // Tiles of air only are left out: the floor's 6 tiles (y 0..31) and the sand's one.
    const tiles = json.tiles as { at: Vec3 }[];
    expect(tiles.map((t) => t.at)).toContainEqual([64, 32, 32]);
    expect(tiles).toHaveLength(7);
    const back = checkStructure(JSON.parse(structureText(json)), 'hall.json');
    expect(back.size).toEqual(size);
    const { cells: ids, keep } = clipboardOf(back);
    for (let i = 0; i < cells.length; i++) {
      if (cells[i] === air) expect(keep[i], `cell ${String(i)}`).toBe(1);
      else expect(ids[i], `cell ${String(i)}`).toBe(cells[i]);
    }
  });

  it('a selection that fits one file stays one voxels structure (older builds read it)', () => {
    const json = structureFromVoxels({
      name: 'dwell:post',
      size: [1, 2, 1],
      cells: [stone, stone],
    });
    expect(json.kind).toBe('voxels');
  });

  it('refuses a selection over the limit, and a bad tile by name', () => {
    const most = maxSize();
    expect(() =>
      structureFromVoxels({
        name: 'dwell:huge',
        size: [most[0] + 1, 1, 1],
        cells: new Uint16Array(most[0] + 1).fill(stone),
      }),
    ).toThrow('at most');
    const json = structureFromVoxels({
      name: 'dwell:hall',
      size: [40, 1, 1],
      cells: bigCells([40, 1, 1]),
    });
    const tiles = json.tiles as { at: Vec3; layers: string[][] }[];
    expect(() => checkStructure({ ...json, tiles: [...tiles, tiles[0]] }, 'hall.json')).toThrow(
      'listed twice',
    );
    expect(() =>
      checkStructure({ ...json, tiles: [{ ...tiles[0], at: [5, 0, 0] }] }, 'hall.json'),
    ).toThrow("hall.json: tiles[0].at: must be a tile's corner");
    expect(() => checkStructure({ ...json, tileSize: 16 }, 'hall.json')).toThrow('tileSize');
  });

  // The store is shared by every app version (RELEASES.md §6): append-only.
  it('the store round-trips a tiled structure and keeps fields it does not know', async () => {
    const dir = memoryDir();
    const store = new StructureStore(dir);
    const size: Vec3 = [40, 3, 2];
    const json: StructureJson = {
      ...structureFromVoxels({ name: 'dwell:wall', size, cells: bigCells(size) }),
      tags: ['wall'],
    };
    expect(await store.save(json)).toBe('wall.json');
    const [entry] = await store.list();
    expect(entry?.status).toBe('ok');
    if (entry?.status !== 'ok') return;
    expect(entry.structure.size).toEqual(size);
    expect(entry.json.tags).toEqual(['wall']);
    // Saving over it (a new selection) keeps the newer build's field.
    const again = structureFromVoxels({ name: 'dwell:wall', size, cells: bigCells(size) });
    await store.save(again, { overwrite: true });
    expect(JSON.parse(dir.files.get('wall.json') ?? '{}')).toMatchObject({ tags: ['wall'] });
    // A rename keeps every field.
    await store.rename('wall.json', 'dwell:rampart');
    expect(JSON.parse(dir.files.get('rampart.json') ?? '{}')).toMatchObject({
      kind: 'tiles',
      tags: ['wall'],
    });
    // A tiled file with a newer tile side is kept as it is, never rewritten or deleted.
    const newer = JSON.stringify({ ...again, name: 'dwell:keep', tileSize: 64 });
    dir.files.set('keep.json', newer);
    const kept = await store.get('keep.json');
    expect(kept?.status).toBe('unreadable');
    await expect(store.remove('keep.json')).rejects.toThrow('kept');
    await expect(store.save({ ...again, name: 'dwell:keep' }, { overwrite: true })).rejects.toThrow(
      'kept as it is',
    );
    expect(dir.files.get('keep.json')).toBe(newer);
  });

  it('reads a box larger than one read in tiles', () => {
    const size: Vec3 = [40, 3, 2];
    const world = bigCells(size);
    const reads: Vec3[] = [];
    const read = (min: Vec3, s: Vec3) => {
      reads.push(s);
      const out = new Uint16Array(s[0] * s[1] * s[2]);
      for (let y = 0; y < s[1]; y++)
        for (let z = 0; z < s[2]; z++)
          for (let x = 0; x < s[0]; x++)
            out[(y * s[2] + z) * s[0] + x] =
              world[((min[1] + y) * size[2] + min[2] + z) * size[0] + min[0] + x] ?? 0;
      return out;
    };
    expect([...readTiled(read, [0, 0, 0], size)]).toEqual([...world]);
    expect(reads).toEqual([
      [32, 3, 2],
      [8, 3, 2],
    ]);
    expect(readTiled(() => new Uint16Array(0), [0, 0, 0], size)).toHaveLength(0);
  });
});
