// Procedural block textures from tiled (periodic) noise. Every tile is seamless: the noise lattice
// wraps at the tile size, so a face's texture continues across neighbouring blocks. Tiles live in
// one atlas, each surrounded by a gutter of wrapped texels so mipmapping never bleeds into a
// neighbouring tile. Pure data (no WebGL), so it is deterministic and testable.

/** Texels per tile edge; every noise lattice size below divides it. */
export const TILE = 32;
/** Wrapped texels around each tile, so mip levels down to 4×4 texels per tile never bleed. */
export const GUTTER = 16;
export const CELL = TILE + 2 * GUTTER;
export const ATLAS_CELLS = 8;
export const ATLAS_SIZE = CELL * ATLAS_CELLS;

export type TileName =
  'plain' | 'grass' | 'grassSide' | 'dirt' | 'stone' | 'sand' | 'water' | 'ladder';
export const TILE_ORDER: readonly TileName[] = [
  'plain',
  'grass',
  'grassSide',
  'dirt',
  'stone',
  'sand',
  'water',
  'ladder',
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

// Base colours target the measured palette (WORLD_GENERATION.md §1.3–1.4): vivid yellow-green
// grass, warm red-brown dirt, warm grey rock with cream and violet flecks, turquoise water.
const GRASS = rgb(0x8dbf3f);
const GRASS_HIGH = rgb(0xb5cf4f);
const GRASS_LOW = rgb(0x5f9a3a);
const DIRT = rgb(0x8a5a3a);
const STONE = rgb(0xbbafa6);
const STONE_COOL = rgb(0x9c95a8);
const STONE_FLECK = rgb(0xd6c6b4);

function grass(x: number, y: number): Rgb {
  const n = tiledFbm(x, y, 11);
  const patches = tiledNoise(x, y, 4, 7, 12);
  const blade = hash(x, y, 9, 13); // per-texel speckle: darker and lighter blades
  // Gentle: large features repeat once per block, so bright patches only tint; noise runs from
  // the deeper green in the hollows to the yellow-green highlights.
  const lit = mix(GRASS, GRASS_HIGH, 0.55 * patches);
  let c = mix(GRASS_LOW, lit, Math.min(1, 0.35 + 0.9 * n));
  if (blade > 0.9) c = scale(c, 0.82);
  else if (blade < 0.06) c = scale(c, 1.1);
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
  let c = scale(mix(STONE, STONE_COOL, 0.8 * tint), 0.78 + 0.34 * n);
  if (fleck > 0.92) c = mix(c, STONE_COOL, 0.5);
  else if (fleck < 0.05) c = STONE_FLECK;
  return c;
}

const SAND = rgb(0xeed9a4);

function sand(x: number, y: number): Rgb {
  const n = tiledFbm(x, y, 51);
  const grain = hash(x, y, 1, 52);
  let c = scale(SAND, 0.86 + 0.2 * n);
  if (grain > 0.9) c = scale(c, 0.9);
  return c;
}

const WATER = rgb(0x3fb2d6);
const RAIL = rgb(0xa0703a);

/** Water: soft ripples (the face's opacity makes it translucent). */
function water(x: number, y: number): Rgb {
  const ripple = tiledNoise(x, y, 4, 5, 151);
  const fine = tiledFbm(x, y, 152);
  return scale(WATER, 0.85 + 0.15 * ripple + 0.12 * fine);
}

/** Ladder: two rails and four rungs over a dark backing. */
function ladder(x: number, y: number): Rgb {
  const grain = 0.8 + 0.3 * tiledNoise(x, y * 0.25, 16, 6, 161);
  const rail = (x >= 3 && x <= 6) || (x >= 25 && x <= 28);
  const rung = y % 8 >= 2 && y % 8 <= 4;
  if (rail) return scale(RAIL, grain);
  if (rung) return scale(RAIL, grain * 0.9);
  return scale(RAIL, 0.3 + 0.08 * tiledFbm(x, y, 162));
}

const PAINTERS: Record<TileName, (x: number, y: number) => Rgb> = {
  plain: () => [255, 255, 255],
  grass,
  grassSide,
  dirt,
  stone,
  sand,
  water,
  ladder,
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

let shared: Atlas | null = null;
/** The atlas, built once per page (the renderer and the hotbar's swatches share it). */
export function sharedAtlas(): Atlas {
  shared ??= buildAtlas();
  return shared;
}

/** An sRGB byte (0–255) as linear light (0–1). */
export function srgbToLinear(byte: number): number {
  const c = byte / 255;
  return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
}

/** Linear light (0–1) as an sRGB byte (0–255). */
export function linearToSrgbByte(v: number): number {
  const c = v <= 0.0031308 ? v * 12.92 : 1.055 * v ** (1 / 2.4) - 0.055;
  return Math.round(Math.min(1, Math.max(0, c)) * 255);
}

const averages = new Map<TileName, number>();
/**
 * A tile's average colour (0xRRGGBB, sRGB): the mean of its texels in linear light, as a
 * mipmapped sRGB texture averages from afar — the flat colour of LOD faces (ARCHITECTURE.md
 * §6.6). Computed from the painters, without building the atlas.
 */
export function averageTileColor(name: TileName): number {
  let c = averages.get(name);
  if (c === undefined) {
    const paint = PAINTERS[name];
    let r = 0;
    let g = 0;
    let b = 0;
    for (let y = 0; y < TILE; y++) {
      for (let x = 0; x < TILE; x++) {
        const [pr, pg, pb] = paint(x, y);
        r += srgbToLinear(Math.round(Math.min(255, Math.max(0, pr))));
        g += srgbToLinear(Math.round(Math.min(255, Math.max(0, pg))));
        b += srgbToLinear(Math.round(Math.min(255, Math.max(0, pb))));
      }
    }
    const n = TILE * TILE;
    c = (linearToSrgbByte(r / n) << 16) | (linearToSrgbByte(g / n) << 8) | linearToSrgbByte(b / n);
    averages.set(name, c);
  }
  return c;
}

/** One tile's TILE × TILE texels, top row first (for a 2D canvas). */
export function tilePixels(atlas: Atlas, name: TileName): Uint8ClampedArray<ArrayBuffer> {
  const out = new Uint8ClampedArray(TILE * TILE * 4);
  const index = TILE_ORDER.indexOf(name);
  const ox = (index % ATLAS_CELLS) * CELL + GUTTER;
  const oy = Math.floor(index / ATLAS_CELLS) * CELL + GUTTER;
  for (let y = 0; y < TILE; y++) {
    const from = ((oy + TILE - 1 - y) * atlas.size + ox) * 4;
    out.set(atlas.data.subarray(from, from + TILE * 4), y * TILE * 4);
  }
  return out;
}
