// Procedural block textures from tiled (periodic) noise. Every tile is seamless: the noise lattice
// wraps at the tile size, so a face's texture continues across neighbouring blocks. Tiles live in
// one atlas, each surrounded by a gutter of wrapped texels so mipmapping never bleeds into a
// neighbouring tile. Pure data (no WebGL), so it is deterministic and testable.

/** Texels per tile edge; every noise lattice size below divides it. */
export const TILE = 32;
/** Wrapped texels around each tile, so mip levels down to 4×4 texels per tile never bleed. */
export const GUTTER = 16;
export const CELL = TILE + 2 * GUTTER;
export const ATLAS_CELLS = 4;
export const ATLAS_SIZE = CELL * ATLAS_CELLS;

export type TileName =
  | 'plain'
  | 'grass'
  | 'grassSide'
  | 'dirt'
  | 'stone'
  | 'sand'
  | 'sandstone'
  | 'gravel'
  | 'snow'
  | 'logSide'
  | 'logTop'
  | 'leaves'
  | 'coalOre'
  | 'ironOre'
  | 'goldOre';
export const TILE_ORDER: readonly TileName[] = [
  'plain',
  'grass',
  'grassSide',
  'dirt',
  'stone',
  'sand',
  'sandstone',
  'gravel',
  'snow',
  'logSide',
  'logTop',
  'leaves',
  'coalOre',
  'ironOre',
  'goldOre',
];

export interface TileRect {
  u0: number;
  v0: number;
  u1: number;
  v1: number;
}

/** Hash of lattice point (i, j) in layer `k` → [0, 1). */
function hash(i: number, j: number, k: number, seed: number): number {
  let h = (i * 374761393 + j * 668265263 + k * 2147483647 + seed * 144269504) | 0;
  h = Math.imul(h ^ (h >>> 13), 1274126177);
  h ^= h >>> 16;
  return (h >>> 0) / 4294967296;
}

const smooth = (t: number) => t * t * (3 - 2 * t);

/**
 * Value noise at texel (x, y) over a lattice of `cells` × `cells` per tile, wrapping at the tile
 * edge: noise(x + TILE, y) === noise(x, y). Result in [0, 1].
 */
export function tiledNoise(
  x: number,
  y: number,
  cells: number,
  layer: number,
  seed: number,
): number {
  const fx = (x / TILE) * cells;
  const fy = (y / TILE) * cells;
  const x0 = Math.floor(fx);
  const y0 = Math.floor(fy);
  const tx = smooth(fx - x0);
  const ty = smooth(fy - y0);
  const wrap = (v: number) => ((v % cells) + cells) % cells;
  const h = (i: number, j: number) => hash(wrap(i), wrap(j), layer, seed);
  const a = h(x0, y0) + (h(x0 + 1, y0) - h(x0, y0)) * tx;
  const b = h(x0, y0 + 1) + (h(x0 + 1, y0 + 1) - h(x0, y0 + 1)) * tx;
  return a + (b - a) * ty;
}

/** Fractal (fBm) sum of tiled noise: lattices of 4, 8, 16, and 32 cells per tile. In [0, 1]. */
export function tiledFbm(x: number, y: number, seed: number): number {
  let sum = 0;
  let norm = 0;
  let amplitude = 1;
  for (let octave = 0, cells = 4; cells <= TILE; octave++, cells *= 2) {
    sum += tiledNoise(x, y, cells, octave, seed) * amplitude;
    norm += amplitude;
    amplitude *= 0.5;
  }
  return sum / norm;
}

type Rgb = [number, number, number];
const rgb = (hex: number): Rgb => [(hex >> 16) & 0xff, (hex >> 8) & 0xff, hex & 0xff];
const mix = (a: Rgb, b: Rgb, t: number): Rgb => [
  a[0] + (b[0] - a[0]) * t,
  a[1] + (b[1] - a[1]) * t,
  a[2] + (b[2] - a[2]) * t,
];
const scale = (c: Rgb, s: number): Rgb => [c[0] * s, c[1] * s, c[2] * s];

const GRASS = rgb(0x5e9c3a);
const GRASS_DRY = rgb(0x8fae3f);
const DIRT = rgb(0x7a5534);
const STONE = rgb(0xa4a7ab);
const STONE_COOL = rgb(0x959ca8);

function grass(x: number, y: number): Rgb {
  const n = tiledFbm(x, y, 11);
  const patches = tiledNoise(x, y, 4, 7, 12);
  const blade = hash(x, y, 9, 13); // per-texel speckle: darker and lighter blades
  // Gentle: large features repeat once per block, so dry patches only tint.
  let c = scale(mix(GRASS, GRASS_DRY, 0.35 * patches), 0.78 + 0.38 * n);
  if (blade > 0.9) c = scale(c, 0.78);
  else if (blade < 0.06) c = scale(c, 1.14);
  return c;
}

function dirt(x: number, y: number): Rgb {
  const n = tiledFbm(x, y, 21);
  const pebble = hash(x, y, 3, 22);
  let c = scale(DIRT, 0.72 + 0.45 * n);
  if (pebble > 0.95) c = scale(c, 1.25);
  return c;
}

/** Dirt with a ragged grass fringe along the top edge (y = TILE − 1 is the top). */
function grassSide(x: number, y: number): Rgb {
  // Fringe depth varies along x with 1D tiled noise (sampled on a fixed row).
  const depth = 4 + 5 * tiledNoise(x, 0, 8, 5, 31);
  return TILE - 1 - y < depth ? grass(x, y) : dirt(x, y);
}

function stone(x: number, y: number): Rgb {
  const n = tiledFbm(x, y, 41);
  const tint = tiledNoise(x, y, 4, 2, 42);
  // Fine flecks rather than large features: at one tile per block, anything big repeats visibly.
  const fleck = hash(x, y, 5, 44);
  let c = scale(mix(STONE, STONE_COOL, tint), 0.74 + 0.4 * n);
  if (fleck > 0.92) c = scale(c, 0.82);
  else if (fleck < 0.05) c = scale(c, 1.12);
  return c;
}

const SAND = rgb(0xdbcf9a);
const SANDSTONE = rgb(0xc9b37a);
const GRAVEL = rgb(0x8c8580);
const SNOW = rgb(0xf2f5f8);
const BARK = rgb(0x6b4a2b);
const WOOD = rgb(0xb08a55);
const LEAVES = rgb(0x3f7d2c);

function sand(x: number, y: number): Rgb {
  const n = tiledFbm(x, y, 51);
  const grain = hash(x, y, 1, 52);
  let c = scale(SAND, 0.86 + 0.2 * n);
  if (grain > 0.9) c = scale(c, 0.9);
  return c;
}

/** Sandstone: sand with horizontal bands (the tile's rows wrap, so bands tile seamlessly). */
function sandstone(x: number, y: number): Rgb {
  const n = tiledFbm(x, y, 61);
  const band = tiledNoise(0, y, 8, 3, 62);
  return scale(SANDSTONE, 0.78 + 0.18 * n + 0.12 * band);
}

function gravel(x: number, y: number): Rgb {
  // Pebbles: coarse value noise quantised into light and dark stones.
  const pebble = tiledNoise(x, y, 8, 1, 71);
  const n = tiledFbm(x, y, 72);
  return scale(GRAVEL, (pebble > 0.55 ? 1.12 : pebble < 0.4 ? 0.78 : 0.95) * (0.85 + 0.25 * n));
}

function snow(x: number, y: number): Rgb {
  const n = tiledFbm(x, y, 81);
  return scale(SNOW, 0.92 + 0.08 * n);
}

/** Bark: vertical streaks (noise varying along x only, stretched along y). */
function logSide(x: number, y: number): Rgb {
  const streak = tiledNoise(x, y * 0.125, 16, 4, 91);
  const n = tiledFbm(x, y, 92);
  return scale(BARK, 0.7 + 0.35 * streak + 0.15 * n);
}

/** Cut wood with growth rings around the tile centre, and a bark rim. */
function logTop(x: number, y: number): Rgb {
  const dx = x - TILE / 2 + 0.5;
  const dy = y - TILE / 2 + 0.5;
  const r = Math.max(Math.abs(dx), Math.abs(dy));
  if (r > TILE / 2 - 3) return logSide(x, y);
  const ring = 0.5 + 0.5 * Math.cos(Math.hypot(dx, dy) * 1.3);
  return scale(WOOD, 0.8 + 0.15 * ring + 0.1 * tiledFbm(x, y, 93));
}

function leaves(x: number, y: number): Rgb {
  const n = tiledFbm(x, y, 101);
  const gap = hash(x, y, 2, 102);
  let c = scale(LEAVES, 0.7 + 0.45 * n);
  if (gap > 0.88) c = scale(c, 0.55); // dark gaps between leaves
  return c;
}

/** Stone with clusters of a mineral colour. */
function ore(color: number, seed: number): (x: number, y: number) => Rgb {
  const mineral = rgb(color);
  return (x, y) => {
    const cluster = tiledNoise(x, y, 8, 0, seed);
    const speck = hash(x, y, 6, seed + 1);
    if (cluster > 0.62 && speck > 0.25) return scale(mineral, 0.8 + 0.3 * speck);
    return stone(x, y);
  };
}

const PAINTERS: Record<TileName, (x: number, y: number) => Rgb> = {
  plain: () => [255, 255, 255],
  grass,
  grassSide,
  dirt,
  stone,
  sand,
  sandstone,
  gravel,
  snow,
  logSide,
  logTop,
  leaves,
  coalOre: ore(0x26262a, 111),
  ironOre: ore(0xc8926a, 121),
  goldOre: ore(0xf2c230, 131),
};

export interface Atlas {
  size: number;
  /** RGBA8, row 0 at v = 0 (bottom), i.e. no vertical flip on upload. */
  data: Uint8Array;
  rect(name: TileName): TileRect;
}

export function tileRect(name: TileName): TileRect {
  const index = TILE_ORDER.indexOf(name);
  const cx = index % ATLAS_CELLS;
  const cy = Math.floor(index / ATLAS_CELLS);
  const u0 = (cx * CELL + GUTTER) / ATLAS_SIZE;
  const v0 = (cy * CELL + GUTTER) / ATLAS_SIZE;
  return { u0, v0, u1: u0 + TILE / ATLAS_SIZE, v1: v0 + TILE / ATLAS_SIZE };
}

export function buildAtlas(): Atlas {
  const data = new Uint8Array(ATLAS_SIZE * ATLAS_SIZE * 4);
  TILE_ORDER.forEach((name, index) => {
    const paint = PAINTERS[name];
    const ox = (index % ATLAS_CELLS) * CELL;
    const oy = Math.floor(index / ATLAS_CELLS) * CELL;
    for (let py = 0; py < CELL; py++) {
      for (let px = 0; px < CELL; px++) {
        // The gutter repeats the tile (it is seamless, so wrapping is exact).
        const tx = (((px - GUTTER) % TILE) + TILE) % TILE;
        const ty = (((py - GUTTER) % TILE) + TILE) % TILE;
        const [r, g, b] = paint(tx, ty);
        const o = ((oy + py) * ATLAS_SIZE + ox + px) * 4;
        data[o] = Math.round(Math.min(255, Math.max(0, r)));
        data[o + 1] = Math.round(Math.min(255, Math.max(0, g)));
        data[o + 2] = Math.round(Math.min(255, Math.max(0, b)));
        data[o + 3] = 255;
      }
    }
  });
  return { size: ATLAS_SIZE, data, rect: tileRect };
}
