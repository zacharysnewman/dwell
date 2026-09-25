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

export type TileName = 'plain' | 'grass' | 'grassSide' | 'dirt' | 'stone';
const TILE_ORDER: readonly TileName[] = ['plain', 'grass', 'grassSide', 'dirt', 'stone'];

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

const PAINTERS: Record<TileName, (x: number, y: number) => Rgb> = {
  plain: () => [255, 255, 255],
  grass,
  grassSide,
  dirt,
  stone,
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
