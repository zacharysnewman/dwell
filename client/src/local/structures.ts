// Structures in the client (docs/STRUCTURES.md §3–§4, ARCHITECTURE.md §6.4): the format's rules
// are shared/structures/format.mjs — the same code the generator (gen.mjs) runs in CI — over this
// build's block registry, so a structure saved here is exactly what `shared/structures/` accepts.
// This module turns voxels read from the world into a structure file, and prints files the way
// the committed ones look.
import {
  MAX_EDGE,
  registryFromBlockDefs,
  TOP_LEVEL,
  validate,
  type Registry,
  type ValidStructure,
} from '../../../shared/structures/format.mjs';
import { BLOCK_DEFS, STATE_DEFS, stateId } from '../world/blocks';

export { MAX_EDGE, type ValidStructure };

export type Vec3 = [number, number, number];

/** A structure file as stored and exported: the known fields, and any a newer build added. */
export type StructureJson = Record<string, unknown>;

/** This build's registry, in the form the format's rules read. */
export const REGISTRY: Registry = registryFromBlockDefs(BLOCK_DEFS);

/** Kept by every save (never part of a structure): empty cells, and the sandbox's ground. */
export const KEPT_ON_SAVE: readonly string[] = ['dwell:air', 'dwell:sandbox_floor'];

/** The fields this build knows (format.mjs `TOP_LEVEL`); others are a newer build's. */
export function knownFields(json: StructureJson): StructureJson {
  return Object.fromEntries(Object.entries(json).filter(([k]) => TOP_LEVEL.includes(k)));
}

/** The fields this build doesn't know, kept on every rewrite (the store is append-only). */
export function unknownFields(json: StructureJson): StructureJson {
  return Object.fromEntries(Object.entries(json).filter(([k]) => !TOP_LEVEL.includes(k)));
}

/**
 * Checks a structure by the generator's rules (format.mjs `validate`), ignoring fields a newer
 * build added (gen.mjs itself rejects unknown fields: the repository has one version). Throws
 * `<file>: <entry>: <why>`.
 */
export function checkStructure(json: unknown, file: string): ValidStructure {
  if (json === null || typeof json !== 'object' || Array.isArray(json)) {
    throw new Error(`${file}: (file): not a JSON object`);
  }
  return validate(knownFields(json as StructureJson), file, REGISTRY);
}

/** Parses JSON text and checks it; the parsed object (all fields) and the checked structure. */
export function parseStructure(
  text: string,
  file: string,
): { json: StructureJson; structure: ValidStructure } {
  let json: unknown;
  try {
    json = JSON.parse(text);
  } catch (e) {
    throw new Error(`${file}: (file): ${e instanceof Error ? e.message : String(e)}`, { cause: e });
  }
  const structure = checkStructure(json, file);
  return { json: json as StructureJson, structure };
}

const NAME_PART = /^[a-z0-9_]+$/;

/**
 * A structure's name from what the player typed: lower case, runs of other characters as `_`,
 * in the `dwell` namespace unless one is given (`ns:name`). `test` is the fixtures' namespace.
 */
export function structureName(text: string): { name: string } | { error: string } {
  const clean = (s: string) =>
    s
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9_]+/g, '_')
      .replace(/^_+|_+$/g, '');
  const colon = text.indexOf(':');
  const ns = colon >= 0 ? clean(text.slice(0, colon)) : 'dwell';
  const local = clean(colon >= 0 ? text.slice(colon + 1) : text);
  if (!local || !NAME_PART.test(local)) return { error: 'Give the structure a name.' };
  if (!ns || !NAME_PART.test(ns)) return { error: 'The namespace (before ":") is empty.' };
  if (ns === 'test') return { error: '"test" is for the repository’s test fixtures.' };
  return { name: `${ns}:${local}` };
}

/** The file a structure is kept in and exported as: `<name>.json`, the name without namespace. */
export function structureFile(name: string): string {
  return `${name.slice(name.indexOf(':') + 1)}.json`;
}

/** "5 × 4 × 5". */
export function sizeLabel(size: readonly number[]): string {
  return size.join(' × ');
}

// ---- Saving from the world ------------------------------------------------------------------------

/** The box from two corners (inclusive, any order): its lowest corner and size. */
export function boxOf(a: Vec3, b: Vec3): { min: Vec3; size: Vec3 } {
  const min: Vec3 = [Math.min(a[0], b[0]), Math.min(a[1], b[1]), Math.min(a[2], b[2])];
  const size: Vec3 = [
    Math.abs(a[0] - b[0]) + 1,
    Math.abs(a[1] - b[1]) + 1,
    Math.abs(a[2] - b[2]) + 1,
  ];
  return { min, size };
}

/**
 * The smallest box (in the box's own coordinates) holding every cell `built` accepts; null if
 * none. `cells` is in layer order (y-slices, z-rows, x fastest), as `Client.voxels` reads them.
 */
export function builtBounds(
  cells: ArrayLike<number>,
  size: Vec3,
  built: (state: number) => boolean,
): { min: Vec3; size: Vec3 } | null {
  const [sx, sy, sz] = size;
  const lo: Vec3 = [Infinity, Infinity, Infinity];
  const hi: Vec3 = [-Infinity, -Infinity, -Infinity];
  for (let y = 0; y < sy; y++) {
    for (let z = 0; z < sz; z++) {
      for (let x = 0; x < sx; x++) {
        if (!built(cells[(y * sz + z) * sx + x] ?? 0)) continue;
        const p = [x, y, z];
        for (let i = 0; i < 3; i++) {
          lo[i] = Math.min(lo[i] ?? 0, p[i] ?? 0);
          hi[i] = Math.max(hi[i] ?? 0, p[i] ?? 0);
        }
      }
    }
  }
  if (lo[0] === Infinity) return null;
  return boxOf(lo, hi);
}

/** Reads a box's states in layer order; empty where a chunk of it isn't loaded (`Client.voxels`). */
export type ReadBox = (min: Vec3, size: Vec3) => ArrayLike<number>;

/** The edge of the tiles `scanBuilt` reads: a chunk, so a tile is loaded or not as a whole. */
const TILE = 32;

/**
 * The box (world coordinates) of every built cell — a state not in `kept` — in the region from
 * `min` of `size`, read tile by tile along the chunk grid; tiles not loaded are passed over.
 * Null if nothing is built there.
 */
export function scanBuilt(
  read: ReadBox,
  min: Vec3,
  size: Vec3,
  kept: readonly string[] = KEPT_ON_SAVE,
): { min: Vec3; size: Vec3 } | null {
  const keptIds = new Set(kept.map((k) => stateId(k)));
  const built = (s: number) => !keptIds.has(s);
  let lo: Vec3 | null = null;
  let hi: Vec3 | null = null;
  const start = min.map((v) => Math.floor(v / TILE) * TILE) as Vec3;
  for (let ty = start[1]; ty < min[1] + size[1]; ty += TILE) {
    for (let tz = start[2]; tz < min[2] + size[2]; tz += TILE) {
      for (let tx = start[0]; tx < min[0] + size[0]; tx += TILE) {
        // The tile clipped to the region.
        const a: Vec3 = [Math.max(tx, min[0]), Math.max(ty, min[1]), Math.max(tz, min[2])];
        const b: Vec3 = [
          Math.min(tx + TILE, min[0] + size[0]),
          Math.min(ty + TILE, min[1] + size[1]),
          Math.min(tz + TILE, min[2] + size[2]),
        ];
        const tileSize: Vec3 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        const cells = read(a, tileSize);
        if (cells.length === 0) continue;
        const box = builtBounds(cells, tileSize, built);
        if (!box) continue;
        const bmin: Vec3 = [a[0] + box.min[0], a[1] + box.min[1], a[2] + box.min[2]];
        const bmax: Vec3 = [
          bmin[0] + box.size[0] - 1,
          bmin[1] + box.size[1] - 1,
          bmin[2] + box.size[2] - 1,
        ];
        lo = lo
          ? [Math.min(lo[0], bmin[0]), Math.min(lo[1], bmin[1]), Math.min(lo[2], bmin[2])]
          : bmin;
        hi = hi
          ? [Math.max(hi[0], bmax[0]), Math.max(hi[1], bmax[1]), Math.max(hi[2], bmax[2])]
          : bmax;
      }
    }
  }
  return lo && hi ? boxOf(lo, hi) : null;
}

/**
 * A structure as the region tools' clipboard takes it (`Client.loadStructure`): runtime state
 * ids in layer order, and a mask of the cells it keeps (`null` in its palette).
 */
export function clipboardOf(s: ValidStructure): { cells: Uint16Array; keep: Uint8Array } {
  const ids = new Map<string, number | null>();
  for (const [c, state] of s.palette) ids.set(c, state === null ? null : stateId(state));
  const cells = new Uint16Array(s.cells.length);
  const keep = new Uint8Array(s.cells.length);
  for (let i = 0; i < s.cells.length; i++) {
    const id = ids.get(s.cells[i] ?? '');
    if (id === null || id === undefined) keep[i] = 1;
    else cells[i] = id;
  }
  return { cells, keep };
}

/** The default anchor: the bottom centre (the cell `place` puts on the first air above ground). */
export function bottomCentre(size: Vec3): Vec3 {
  return [Math.floor(size[0] / 2), 0, Math.floor(size[2] / 2)];
}

/** Palette characters in the order they are tried after a state's own initial. */
const PALETTE_POOL =
  "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789#$%&*+-=@^~!?<>/|:;,()[]{}`'_";
/** The palette character of kept cells (`null`), as in the committed files. */
const KEEP = '.';

/**
 * A structure file from the states of a box (layer order, as `Client.voxels` reads them): states
 * named in `kept` (air, the sandbox floor) become kept cells (`null`, "."), every other state a
 * palette character — its block's initial where free. Throws over the size cap or with more
 * distinct states than palette characters; the result passes `checkStructure`.
 */
export function structureFromVoxels(options: {
  name: string;
  size: Vec3;
  cells: ArrayLike<number>;
  anchor?: Vec3;
  kept?: readonly string[];
}): StructureJson {
  const { name, size, cells } = options;
  const [sx, sy, sz] = size;
  if (size.some((n) => !Number.isInteger(n) || n < 1 || n > MAX_EDGE)) {
    throw new Error(
      `A structure is at most ${String(MAX_EDGE)} blocks on each side, not ${sizeLabel(size)}.`,
    );
  }
  if (cells.length !== sx * sy * sz) throw new Error('The cells do not fill the box.');
  const kept = new Set(options.kept ?? KEPT_ON_SAVE);
  const charOf = new Map<number, string>();
  const palette: Record<string, string | null> = {};
  let anyKept = false;
  const used = new Set<string>([KEEP]);
  const pick = (state: string): string => {
    const local = state.slice(state.indexOf(':') + 1);
    const initial = local[0] ?? 'a';
    for (const c of [initial, initial.toUpperCase(), ...PALETTE_POOL.split('')]) {
      if (!used.has(c)) return c;
    }
    throw new Error('Too many different blocks for one structure.');
  };
  const layers: string[][] = [];
  for (let y = 0; y < sy; y++) {
    const layer: string[] = [];
    for (let z = 0; z < sz; z++) {
      let row = '';
      for (let x = 0; x < sx; x++) {
        const id = cells[(y * sz + z) * sx + x] ?? 0;
        let c = charOf.get(id);
        if (c === undefined) {
          const state = STATE_DEFS[id]?.state;
          if (state === undefined) throw new Error(`Unknown block state id ${String(id)}.`);
          if (kept.has(state)) {
            c = KEEP;
            anyKept = true;
          } else {
            c = pick(state);
            used.add(c);
            palette[c] = state;
          }
          charOf.set(id, c);
        }
        if (c === KEEP) anyKept = true;
        row += c;
      }
      layer.push(row);
    }
    layers.push(layer);
  }
  const sorted: Record<string, string | null> = anyKept ? { [KEEP]: null } : {};
  for (const c of Object.keys(palette).sort()) sorted[c] = palette[c] ?? null;
  return {
    format: 1,
    name,
    kind: 'voxels',
    size: [...size],
    anchor: [...(options.anchor ?? bottomCentre(size))],
    palette: sorted,
    layers,
  };
}

// ---- Printing -------------------------------------------------------------------------------------

/** The line width the printer fills before breaking a value over lines. */
const WIDTH = 100;

function flat(v: unknown): string {
  if (Array.isArray(v)) return `[${v.map(flat).join(', ')}]`;
  if (v !== null && typeof v === 'object') {
    const entries = Object.entries(v).filter(([, x]) => x !== undefined);
    if (entries.length === 0) return '{}';
    return `{ ${entries.map(([k, x]) => `${JSON.stringify(k)}: ${flat(x)}`).join(', ')} }`;
  }
  // Parsed JSON holds no undefined, functions or symbols, for which this would be undefined.
  return JSON.stringify(v);
}

function pretty(v: unknown, indent: string, room: number, top = false): string {
  const one = flat(v);
  if (!top && (one.length <= room || v === null || typeof v !== 'object')) return one;
  const inner = `${indent}  `;
  const width = WIDTH - inner.length;
  if (Array.isArray(v)) {
    const items = v.map((x) => `${inner}${pretty(x, inner, width - 1)}`);
    return `[\n${items.join(',\n')}\n${indent}]`;
  }
  const entries = Object.entries(v as Record<string, unknown>).filter(([, x]) => x !== undefined);
  const items = entries.map(([k, x]) => {
    const key = `${JSON.stringify(k)}: `;
    // The layers always one per line, as the committed files have them.
    const room = top && k === 'layers' && Array.isArray(x) ? -1 : width - key.length - 1;
    return `${inner}${key}${room < 0 ? pretty(x, inner, 0, true) : pretty(x, inner, room)}`;
  });
  return `{\n${items.join(',\n')}\n${indent}}`;
}

/**
 * A structure file's text, as the committed files look: the known fields in the format's order,
 * then any a newer build added; a value on one line where it fits, rows of a layer one per line
 * where they don't. Parsing it gives back the same object.
 */
export function structureText(json: StructureJson): string {
  const ordered: StructureJson = {};
  for (const k of TOP_LEVEL) if (k in json) ordered[k] = json[k];
  for (const [k, v] of Object.entries(json)) if (!(k in ordered)) ordered[k] = v;
  return `${pretty(ordered, '', WIDTH, true)}\n`;
}
