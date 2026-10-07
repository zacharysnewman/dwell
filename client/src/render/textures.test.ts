import { describe, expect, it } from 'vitest';
import {
  ATLAS_CELLS,
  ATLAS_SIZE,
  buildAtlas,
  CELL,
  GUTTER,
  TILE,
  tileRect,
  tiledFbm,
  tiledNoise,
  TILE_ORDER,
  type TileName,
} from './textures';

const TILES = TILE_ORDER;

function texel(atlas: ReturnType<typeof buildAtlas>, x: number, y: number): number[] {
  const o = (y * ATLAS_SIZE + x) * 4;
  return [atlas.data[o] ?? 0, atlas.data[o + 1] ?? 0, atlas.data[o + 2] ?? 0];
}

describe('tiled noise', () => {
  it('wraps at the tile size in both directions', () => {
    for (const [x, y] of [
      [0, 0],
      [3.5, 17.25],
      [31, 12],
    ] as const) {
      expect(tiledNoise(x + TILE, y, 8, 0, 1)).toBeCloseTo(tiledNoise(x, y, 8, 0, 1), 12);
      expect(tiledFbm(x, y + TILE, 5)).toBeCloseTo(tiledFbm(x, y, 5), 12);
      expect(tiledFbm(x - TILE, y - 2 * TILE, 5)).toBeCloseTo(tiledFbm(x, y, 5), 12);
    }
  });

  it('stays in [0, 1] and varies', () => {
    let lo = 1;
    let hi = 0;
    for (let y = 0; y < TILE; y++) {
      for (let x = 0; x < TILE; x++) {
        const n = tiledFbm(x, y, 3);
        lo = Math.min(lo, n);
        hi = Math.max(hi, n);
      }
    }
    expect(lo).toBeGreaterThanOrEqual(0);
    expect(hi).toBeLessThanOrEqual(1);
    expect(hi - lo).toBeGreaterThan(0.3);
  });
});

describe('texture atlas', () => {
  const atlas = buildAtlas();

  it('has a cell for every tile', () => {
    expect(TILE_ORDER.length).toBeLessThanOrEqual(ATLAS_CELLS * ATLAS_CELLS);
  });

  it('is deterministic', () => {
    // Byte comparison: a deep equality over the 1 MB atlas is slow.
    expect(Buffer.compare(buildAtlas().data, atlas.data)).toBe(0);
  });

  it('pads every tile with its own wrapped texels (seamless and mip-safe)', () => {
    for (const name of TILES) {
      const r = tileRect(name);
      const x0 = Math.round(r.u0 * ATLAS_SIZE);
      const y0 = Math.round(r.v0 * ATLAS_SIZE);
      expect(x0 % CELL).toBe(GUTTER);
      // The texel left of the tile equals the tile's last column; above its top equals its first row.
      for (let i = 0; i < TILE; i += 5) {
        expect(texel(atlas, x0 - 1, y0 + i)).toEqual(texel(atlas, x0 + TILE - 1, y0 + i));
        expect(texel(atlas, x0 + i, y0 + TILE)).toEqual(texel(atlas, x0 + i, y0));
      }
    }
  });

  it('paints grass green, stone grey, and a grass fringe along the top of the side tile', () => {
    const mean = (name: TileName, rows: [number, number] = [0, TILE]) => {
      const r = tileRect(name);
      const x0 = Math.round(r.u0 * ATLAS_SIZE);
      const y0 = Math.round(r.v0 * ATLAS_SIZE);
      const sum = [0, 0, 0];
      let n = 0;
      for (let y = rows[0]; y < rows[1]; y++) {
        for (let x = 0; x < TILE; x++) {
          texel(atlas, x0 + x, y0 + y).forEach((c, i) => (sum[i] = (sum[i] ?? 0) + c));
          n++;
        }
      }
      return sum.map((c) => c / n);
    };
    const [gr = 0, gg = 0, gb = 0] = mean('grass');
    expect(gg).toBeGreaterThan(gr);
    expect(gg).toBeGreaterThan(gb);
    const [sr = 0, sg = 0, sb = 0] = mean('stone');
    expect(Math.max(sr, sg, sb) - Math.min(sr, sg, sb)).toBeLessThan(20);
    const [, topG = 0] = mean('grassSide', [TILE - 3, TILE]);
    const [bottomR = 0, bottomG = 0] = mean('grassSide', [0, 8]);
    expect(topG).toBeGreaterThan(bottomG);
    expect(bottomR).toBeGreaterThan(bottomG); // dirt below the fringe
    expect(mean('plain')).toEqual([255, 255, 255]);
  });
});

describe('colourful vegetation tiles (WORLD_GENERATION.md §3.7)', () => {
  const atlas = buildAtlas();
  const mean = (name: TileName): number[] => {
    const r = tileRect(name);
    const x0 = Math.round(r.u0 * ATLAS_SIZE);
    const y0 = Math.round(r.v0 * ATLAS_SIZE);
    const sum = [0, 0, 0];
    for (let y = 0; y < TILE; y++) {
      for (let x = 0; x < TILE; x++) {
        texel(atlas, x0 + x, y0 + y).forEach((c, i) => (sum[i] = (sum[i] ?? 0) + c));
      }
    }
    return sum.map((c) => c / (TILE * TILE));
  };

  it('paints each accent leaf in its colour: orange, rust, pink, violet and yellow-green', () => {
    const [ar = 0, ag = 0, ab = 0] = mean('leavesAutumn');
    expect(ar).toBeGreaterThan(ag); // orange: red over green over blue
    expect(ag).toBeGreaterThan(ab);
    const [rr = 0, rg = 0] = mean('leavesRed');
    expect(rr).toBeGreaterThan(rg * 1.6); // rust: red well over green
    expect(rr).toBeLessThan(ar); // darker than the autumn orange
    const [br = 0, bg = 0, bb = 0] = mean('leavesBlossom');
    expect(br).toBeGreaterThan(bg * 1.4); // pink
    expect(bb).toBeGreaterThan(bg);
    const [vr = 0, vg = 0, vb = 0] = mean('leavesViolet');
    expect(vg).toBeLessThan(vr); // violet: green lowest
    expect(vg).toBeLessThan(vb);
    const [yr = 0, yg = 0, yb = 0] = mean('leavesBright');
    const [gr = 0, gg = 0] = mean('leaves');
    expect(yg).toBeGreaterThan(yb * 1.8); // yellow-green
    expect(yr / yg).toBeGreaterThan(gr / gg); // yellower (more red per green) than the green
  });

  it('paints meadow grass with flower flecks and golden grass yellow', () => {
    // The meadow's flecks are brighter and not green: some texels have blue or red over green.
    const r = tileRect('grassMeadow');
    const x0 = Math.round(r.u0 * ATLAS_SIZE);
    const y0 = Math.round(r.v0 * ATLAS_SIZE);
    let flowers = 0;
    for (let y = 0; y < TILE; y++) {
      for (let x = 0; x < TILE; x++) {
        const [pr = 0, pg = 0, pb = 0] = texel(atlas, x0 + x, y0 + y);
        if (pr > pg || pb > pg) flowers++;
      }
    }
    expect(flowers).toBeGreaterThan(3);
    expect(flowers).toBeLessThan(TILE * TILE * 0.1); // sparse
    const [fr = 0, fg = 0] = mean('grassGolden');
    const [er = 0, eg = 0] = mean('grass');
    expect(fr / fg).toBeGreaterThan(er / eg + 0.2); // warmer than the green
    // The side tiles carry the fringe over dirt, like the grass side.
    for (const side of ['grassMeadowSide', 'grassGoldenSide'] as const) {
      const sr = tileRect(side);
      const sx = Math.round(sr.u0 * ATLAS_SIZE);
      const sy = Math.round(sr.v0 * ATLAS_SIZE);
      const bottom = texel(atlas, sx + 5, sy + 1);
      expect(bottom[0] ?? 0).toBeGreaterThan(bottom[1] ?? 0); // dirt below the fringe
    }
  });
});
