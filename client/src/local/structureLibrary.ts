// The player's structure library (docs/STRUCTURES.md §9): the structures store (structureStore.ts)
// as a per-player, per-device source of placements, usable in any world the player creates.
// A selection that fits one structure file (format.mjs: 32 across, 64 tall) is saved as one; a
// larger one as a grid of tiles (`kind: "tiles"`), each at most `tileSize` a side, at most
// `maxTiles` of them along each axis. The limits are data: shared/structures/library.json.
import data from '../../../shared/structures/library.json';
import {
  MAX_EDGE,
  maxEdge,
  validate,
  type ValidStructure,
} from '../../../shared/structures/format.mjs';

export interface LibraryLimits {
  /** A tile's side (cells): at most one region edit's (32). */
  tileSize: number;
  /** The most tiles along x, y and z. */
  maxTiles: [number, number, number];
  /** The longest name (characters, namespace included). */
  maxNameLength: number;
}

/** The region edit's largest side (`region_edit::MAX_SIDE`): a tile is one edit. */
const EDIT_SIDE = 32;

/** Checks the library's limits; throws `shared/structures/library.json: <entry>: <why>`. */
export function checkLimits(json: unknown): LibraryLimits {
  const file = 'shared/structures/library.json';
  const fail = (at: string, why: string): never => {
    throw new Error(`${file}: ${at}: ${why}`);
  };
  if (json === null || typeof json !== 'object' || Array.isArray(json))
    fail('(file)', 'not an object');
  const j = json as Record<string, unknown>;
  for (const k of Object.keys(j)) {
    if (!['$comment', 'tileSize', 'maxTiles', 'maxNameLength'].includes(k))
      fail(k, 'unknown field');
  }
  const whole = (v: unknown, lo: number, hi: number) =>
    typeof v === 'number' && Number.isInteger(v) && v >= lo && v <= hi;
  if (!whole(j.tileSize, 1, Math.min(EDIT_SIDE, MAX_EDGE))) {
    fail('tileSize', `must be a whole number 1..${String(Math.min(EDIT_SIDE, MAX_EDGE))}`);
  }
  const t = j.maxTiles;
  if (!Array.isArray(t) || t.length !== 3 || !t.every((n) => whole(n, 1, 64))) {
    fail('maxTiles', 'must be [x, y, z], each a whole number 1..64');
  }
  if (!whole(j.maxNameLength, 3, 256)) fail('maxNameLength', 'must be a whole number 3..256');
  return {
    tileSize: j.tileSize as number,
    maxTiles: [...(t as number[])] as [number, number, number],
    maxNameLength: j.maxNameLength as number,
  };
}

/** This build's limits. */
export const LIMITS: LibraryLimits = checkLimits(data);

/** The largest selection the library takes: tiles × tile side, per axis. */
export function maxSize(limits: LibraryLimits = LIMITS): [number, number, number] {
  return limits.maxTiles.map((n) => n * limits.tileSize) as [number, number, number];
}

/** A box that fits one structure file (no tiles needed). */
export function fitsOne(size: readonly number[]): boolean {
  return size.every((n, i) => n <= maxEdge(i));
}

/** Within the library's limit. */
export function withinLimit(size: readonly number[], limits: LibraryLimits = LIMITS): boolean {
  const most = maxSize(limits);
  return size.every((n, i) => Number.isInteger(n) && n >= 1 && n <= (most[i] ?? 0));
}

type Vec3 = [number, number, number];

/** The tiles of a box of `size`: each one's offset in the box and its size, x fastest, then z, y. */
export function tilesOf(size: Vec3, tile: number): { at: Vec3; size: Vec3 }[] {
  const out: { at: Vec3; size: Vec3 }[] = [];
  for (let y = 0; y < size[1]; y += tile) {
    for (let z = 0; z < size[2]; z += tile) {
      for (let x = 0; x < size[0]; x += tile) {
        out.push({
          at: [x, y, z],
          size: [
            Math.min(tile, size[0] - x),
            Math.min(tile, size[1] - y),
            Math.min(tile, size[2] - z),
          ],
        });
      }
    }
  }
  return out;
}

/** A tiled file's own fields (the known fields of `kind: "tiles"`). */
export const TILES_FIELDS: readonly string[] = [
  '$comment',
  'format',
  'name',
  'kind',
  'size',
  'anchor',
  'palette',
  'tileSize',
  'tiles',
];

/**
 * Splits a voxels structure file (any size; layer order, one palette) into a tiled one: tiles of
 * `tileSize` from the lowest corner, one palette for all; a tile of kept cells only is left out.
 */
export function tiledFrom(
  json: {
    name: string;
    size: Vec3;
    anchor: Vec3;
    palette: Record<string, string | null>;
    layers: string[][];
  },
  limits: LibraryLimits = LIMITS,
): Record<string, unknown> {
  const kept = Object.entries(json.palette)
    .filter(([, s]) => s === null)
    .map(([c]) => c);
  const tiles: { at: Vec3; layers: string[][] }[] = [];
  for (const t of tilesOf(json.size, limits.tileSize)) {
    const layers: string[][] = [];
    let any = false;
    for (let y = 0; y < t.size[1]; y++) {
      const layer: string[] = [];
      for (let z = 0; z < t.size[2]; z++) {
        const row = (json.layers[t.at[1] + y]?.[t.at[2] + z] ?? '').slice(
          t.at[0],
          t.at[0] + t.size[0],
        );
        if (Array.from(row).some((c) => !kept.includes(c))) any = true;
        layer.push(row);
      }
      layers.push(layer);
    }
    if (any) tiles.push({ at: t.at, layers });
  }
  return {
    format: 1,
    name: json.name,
    kind: 'tiles',
    size: [...json.size],
    anchor: [...json.anchor],
    palette: json.palette,
    tileSize: limits.tileSize,
    tiles,
  };
}

/**
 * Checks a tiled file (`kind: "tiles"`) by the format's rules — each tile is validated as a
 * voxels structure of its size with the shared palette — and the library's limits, and returns
 * it as one structure of its whole size (left-out tiles kept cells). Throws `<file>: <entry>: <why>`.
 */
export function checkTiled(
  json: Record<string, unknown>,
  file: string,
  registry: Parameters<typeof validate>[2],
  limits: LibraryLimits = LIMITS,
): ValidStructure {
  const fail = (at: string, why: string): never => {
    throw new Error(`${file}: ${at}: ${why}`);
  };
  if (json.format !== 1) fail('format', `must be 1, not ${JSON.stringify(json.format)}`);
  const size = json.size as unknown[] as Vec3;
  if (!Array.isArray(json.size) || json.size.length !== 3 || !withinLimit(size, limits)) {
    fail('size', `must be [x, y, z] within ${maxSize(limits).join(' × ')}`);
  }
  if (json.tileSize !== limits.tileSize) {
    fail('tileSize', `must be ${String(limits.tileSize)} (shared/structures/library.json)`);
  }
  const anchor = json.anchor as unknown[] as Vec3;
  if (
    !Array.isArray(json.anchor) ||
    json.anchor.length !== 3 ||
    !anchor.every((n, i) => Number.isInteger(n) && n >= 0 && n < (size[i] ?? 0))
  ) {
    fail('anchor', 'must be a cell inside size');
  }
  if (!Array.isArray(json.tiles)) fail('tiles', 'must be a list');
  if (json.palette === null || typeof json.palette !== 'object')
    fail('palette', 'must be an object');
  const palette = json.palette as Record<string, string | null>;
  // The whole box starts kept: a left-out tile keeps what is there.
  const KEPT = '\u0000';
  const cells: string[] = new Array<string>(size[0] * size[1] * size[2]).fill(KEPT);
  const states = new Map<string, string | null>([[KEPT, null]]);
  const seen = new Set<string>();
  const shapes = new Map(tilesOf(size, limits.tileSize).map((t) => [t.at.join(','), t.size]));
  (json.tiles as unknown[]).forEach((raw, i) => {
    const at = `tiles[${String(i)}]`;
    const t = raw as { at?: unknown; layers?: unknown };
    const key = Array.isArray(t.at) ? t.at.join(',') : '';
    const tsize = shapes.get(key);
    if (!tsize)
      fail(
        `${at}.at`,
        `must be a tile's corner (multiples of ${String(limits.tileSize)} inside size)`,
      );
    if (seen.has(key)) fail(`${at}.at`, 'a tile is listed twice');
    seen.add(key);
    const v = validate(
      {
        format: 1,
        name: json.name,
        kind: 'voxels',
        size: tsize,
        anchor: [0, 0, 0],
        palette,
        layers: t.layers,
      },
      `${file}: ${at}`,
      registry,
    );
    for (const [c, s] of v.palette) states.set(c, s);
    const o = (t.at as number[]).map(Number) as Vec3;
    const [sx, sy, sz] = tsize as Vec3;
    for (let y = 0; y < sy; y++) {
      for (let z = 0; z < sz; z++) {
        for (let x = 0; x < sx; x++) {
          const c = v.cells[(y * sz + z) * sx + x] ?? KEPT;
          cells[(o[1] + y) * size[2] * size[0] + (o[2] + z) * size[0] + (o[0] + x)] = c;
        }
      }
    }
  });
  return {
    name: String(json.name),
    kind: 'tiles',
    size: [...size],
    anchor: [...anchor],
    palette: states,
    cells: cells.join(''),
    variants: [],
  };
}
