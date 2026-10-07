// The level-of-detail grid (ARCHITECTURE.md §6.6, ADR 0012), mirroring server/core/include/dwell/
// core/lod.h: a 3D octree of sections whose level-L cells are 2^L m cubes, 32³ cells per section,
// counted from the corner (−2²³, WORLD_MIN_Y, −2²³). Level 0 is the chunk grid; one root section
// at LOD_MAX_LEVEL holds the whole disc. A section's content is 34³ cells (with a one-cell apron)
// in layer order — the chunk mesher's padded layout.
import { CHUNK_SIZE, Lod, World } from '../protocol/constants.gen';
import type { ChunkCoord, Vec3 } from '../protocol/messages';

export const SECTION_CELLS = Lod.sectionCells;
export const MAX_LEVEL = Lod.maxLevel;
export const INDEX_LEVEL = Lod.indexLevel;
export const LOD_PAD = SECTION_CELLS + 2;
export const LOD_VOLUME = LOD_PAD * LOD_PAD * LOD_PAD;
/**
 * Floats per column of a generated section's surface (C++ core::LodSurface, worldgen_api.cpp):
 * height, material, flags (1 valid, 2 wet), where wet the water's level, and the column's grass and
 * foliage tints (0xRRGGBB in 1/64 of a multiplier of 1; 0: none).
 */
export const SURFACE_STRIDE = 6;
export const ORIGIN: Vec3 = [-(2 ** 23), World.worldMinY, -(2 ** 23)];

/** [level, i, j, k]. */
export type LodCoord = readonly [number, number, number, number];

/** What a generated section is: all air, buried (solid, no exposed face), or content to mesh. */
export const LodKind = { Empty: 0, Buried: 1, Content: 2 } as const;
export type LodKind = (typeof LodKind)[keyof typeof LodKind];

export const lodKey = (c: LodCoord): string =>
  `${String(c[0])}:${String(c[1])},${String(c[2])},${String(c[3])}`;

/**
 * A section as one number (exact in a double: 5 + 19 + 8 + 19 bits), for maps on hot paths.
 * Coordinates outside the octree (a neighbour past its edge) map to −1.
 */
const ACROSS = Array.from({ length: MAX_LEVEL + 1 }, (_, level) => 2 ** (MAX_LEVEL - level));
export function lodId(level: number, i: number, j: number, k: number): number {
  const across = ACROSS[level] ?? 0;
  if (i < 0 || k < 0 || j < 0 || i >= across || k >= across || j >= 256) return -1;
  return ((level * 524288 + i) * 256 + j) * 524288 + k;
}

/** Index of cell (x, y, z), each −1..32, in a section's content. */
export const lodCell = (x: number, y: number, z: number): number =>
  x + 1 + LOD_PAD * (z + 1 + LOD_PAD * (y + 1));

export const cellSize = (level: number): number => 2 ** level;
export const sectionSize = (level: number): number => SECTION_CELLS * 2 ** level;
export const sectionsAcross = (level: number): number => 2 ** (MAX_LEVEL - level);
/** Rows of sections holding part of the world's height (one from level 8). */
export const lodRows = (level: number): number =>
  Math.max(1, (World.worldMaxY - World.worldMinY) / sectionSize(level));

/** World metres of the section's min corner. */
export function sectionOrigin(c: LodCoord): Vec3 {
  const s = sectionSize(c[0]);
  return [ORIGIN[0] + c[1] * s, ORIGIN[1] + c[2] * s, ORIGIN[2] + c[3] * s];
}

/** Octant bits: 1 = +x half, 2 = +y half, 4 = +z half. */
export function lodChild(c: LodCoord, octant: number): LodCoord {
  return [
    c[0] - 1,
    c[1] * 2 + (octant & 1),
    c[2] * 2 + ((octant >> 1) & 1),
    c[3] * 2 + (octant >> 2),
  ];
}

export function lodParent(c: LodCoord): LodCoord {
  return [c[0] + 1, Math.floor(c[1] / 2), Math.floor(c[2] / 2), Math.floor(c[3] / 2)];
}

export function lodAncestor(c: LodCoord, level: number): LodCoord {
  const d = 2 ** (level - c[0]);
  return [level, Math.floor(c[1] / d), Math.floor(c[2] / d), Math.floor(c[3] / d)];
}

const CHUNK_OFFSET_XZ = 2 ** 23 / CHUNK_SIZE;
const CHUNK_OFFSET_Y = -World.worldMinY / CHUNK_SIZE;

export function lodOfChunk(c: ChunkCoord): LodCoord {
  return [0, c[0] + CHUNK_OFFSET_XZ, c[1] + CHUNK_OFFSET_Y, c[2] + CHUNK_OFFSET_XZ];
}

export function chunkOfLod(c: LodCoord): ChunkCoord {
  return [c[1] - CHUNK_OFFSET_XZ, c[2] - CHUNK_OFFSET_Y, c[3] - CHUNK_OFFSET_XZ];
}

/** Inside the octree, within the world's rows, and overlapping the world's disc. */
export function lodInWorld(c: LodCoord): boolean {
  const [level, i, j, k] = c;
  if (level < 0 || level > MAX_LEVEL) return false;
  const across = sectionsAcross(level);
  if (i < 0 || k < 0 || i >= across || k >= across || j < 0 || j >= lodRows(level)) return false;
  const [x0, , z0] = sectionOrigin(c);
  const s = sectionSize(level);
  const nearest = (lo: number, hi: number): number => (lo > 0 ? lo : hi - 1 < 0 ? hi - 1 : 0);
  const x = nearest(x0, x0 + s);
  const z = nearest(z0, z0 + s);
  return x * x + z * z < World.worldRadius * World.worldRadius;
}

/** The section at `level` holding world point p. */
export function sectionAt(level: number, p: Vec3): LodCoord {
  const s = sectionSize(level);
  return [
    level,
    Math.floor((p[0] - ORIGIN[0]) / s),
    Math.floor((p[1] - ORIGIN[1]) / s),
    Math.floor((p[2] - ORIGIN[2]) / s),
  ];
}

/** Height bounds of a column of sections (lod.h LodBounds). */
export interface LodBounds {
  lo: number;
  hi: number;
  anyInside: boolean;
}

/** Empty or Buried when the column's bounds decide the section (with its apron), else Content. */
export function kindFromBounds(c: LodCoord, b: LodBounds): LodKind {
  if (!lodInWorld(c) || !b.anyInside) return LodKind.Empty;
  const [, y0] = sectionOrigin(c);
  const cell = cellSize(c[0]);
  if (y0 - cell > b.hi) return LodKind.Empty;
  if (y0 + SECTION_CELLS * cell < b.lo) return LodKind.Buried;
  return LodKind.Content;
}
