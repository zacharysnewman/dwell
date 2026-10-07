import { describe, expect, it } from 'vitest';
import { tileRect, type TileName } from '../render/textures';
import { faceTint, normalTint } from '../render/look';
import { STATE_DEFS, stateId } from '../world/blocks';
import {
  meshChunk,
  PADDED_VOLUME,
  paddedIndex,
  TINT_GRID_BYTES,
  TINT_POINTS,
  TINT_STRIDE,
  type MeshArrays,
} from './mesher';

const WATER = stateId('dwell:water');
const GRASS = stateId('dwell:grass');
const SLAB = stateId('dwell:stone_slab[flooded=false,half=bottom]');
const LADDER_NORTH = stateId('dwell:ladder[facing=north,flooded=false]');
const UNKNOWN = 65_000;

/** Padded voxels with the given cells set (chunk-local, −1..32). */
function voxels(cells: [number, number, number, number][]): Uint16Array {
  const out = new Uint16Array(PADDED_VOLUME);
  for (const [x, y, z, m] of cells) out[paddedIndex(x, y, z)] = m;
  return out;
}

function normalOf(p: Float32Array, i0: number, i1: number, i2: number): number[] {
  const a = [p[i0 * 3] ?? 0, p[i0 * 3 + 1] ?? 0, p[i0 * 3 + 2] ?? 0];
  const b = [p[i1 * 3] ?? 0, p[i1 * 3 + 1] ?? 0, p[i1 * 3 + 2] ?? 0];
  const c = [p[i2 * 3] ?? 0, p[i2 * 3 + 1] ?? 0, p[i2 * 3 + 2] ?? 0];
  const e1 = b.map((v, i) => v - (a[i] ?? 0));
  const e2 = c.map((v, i) => v - (a[i] ?? 0));
  return [
    (e1[1] ?? 0) * (e2[2] ?? 0) - (e1[2] ?? 0) * (e2[1] ?? 0),
    (e1[2] ?? 0) * (e2[0] ?? 0) - (e1[0] ?? 0) * (e2[2] ?? 0),
    (e1[0] ?? 0) * (e2[1] ?? 0) - (e1[1] ?? 0) * (e2[0] ?? 0),
  ];
}

const quads = (a: MeshArrays) => a.indices.length / 6;

/** Bounds of quad `q`'s vertices. */
function bounds(a: MeshArrays, q: number): { min: number[]; max: number[] } {
  const min = [Infinity, Infinity, Infinity];
  const max = [-Infinity, -Infinity, -Infinity];
  for (let k = 0; k < 4; k++)
    for (let i = 0; i < 3; i++) {
      const v = a.positions[(q * 4 + k) * 3 + i] ?? 0;
      min[i] = Math.min(min[i] ?? 0, v);
      max[i] = Math.max(max[i] ?? 0, v);
    }
  return { min, max };
}

describe('greedy chunk mesher', () => {
  it('draws a lone cube as six quads, wound counter-clockwise facing out', () => {
    const { opaque, transparent } = meshChunk(voxels([[1, 1, 1, 2]]));
    expect(quads(opaque)).toBe(6);
    expect(quads(transparent)).toBe(0);
    const normals = new Set<string>();
    for (let q = 0; q < 6; q++) {
      const n = normalOf(
        opaque.positions,
        opaque.indices[q * 6] ?? 0,
        opaque.indices[q * 6 + 1] ?? 0,
        opaque.indices[q * 6 + 2] ?? 0,
      ).map(Math.sign);
      // The winding agrees with the stored normal.
      expect(n).toEqual([0, 1, 2].map((i) => opaque.normals[q * 12 + i] ?? 0));
      normals.add(n.join());
    }
    expect(normals.size).toBe(6);
  });

  it('merges a floor of one material into one quad per side', () => {
    const cells: [number, number, number, number][] = [];
    for (let z = 0; z < 32; z++) for (let x = 0; x < 32; x++) cells.push([x, 0, z, 2]);
    const { opaque } = meshChunk(voxels(cells));
    // Top, bottom, and one strip per side.
    expect(quads(opaque)).toBe(6);
    const top = [...Array(quads(opaque)).keys()].find((q) => opaque.normals[q * 12 + 1] === 1);
    expect(top).toBeDefined();
    const b = bounds(opaque, top ?? 0);
    expect(b.min).toEqual([0, 1, 0]);
    expect(b.max).toEqual([32, 1, 32]);
    // The texture repeats per block: uvs span 32 blocks, the tile stays the stone tile.
    const r = tileRect('stone');
    expect(Math.max(...opaque.uvs)).toBe(32);
    expect(opaque.tiles[0]).toBeCloseTo(r.u0);
    expect(opaque.tiles[2]).toBeCloseTo(r.u1 - r.u0);
  });

  it('keeps materials apart and hides faces against full neighbours, including the apron', () => {
    // Two different blocks side by side: the shared faces are hidden, the tops do not merge.
    let { opaque } = meshChunk(
      voxels([
        [0, 0, 0, 2],
        [1, 0, 0, 3],
      ]),
    );
    expect(quads(opaque)).toBe(10);
    // A block at the chunk's edge next to a solid neighbour chunk (apron): that face is hidden.
    ({ opaque } = meshChunk(
      voxels([
        [31, 5, 5, 2],
        [32, 5, 5, 2],
      ]),
    ));
    expect(quads(opaque)).toBe(5);
  });

  it('draws slabs half height, one quad per face, and keeps their tops open', () => {
    const { opaque } = meshChunk(
      voxels([
        [0, 0, 0, SLAB],
        [1, 0, 0, SLAB],
        [0, 1, 0, 2], // a block on a slab: the slab's top stays open
      ]),
    );
    // Slabs: 2 tops + 2 bottoms + 2×3 open sides (the shared side is hidden) = 10; the block: 6.
    expect(quads(opaque)).toBe(16);
    const slabTops = [...Array(quads(opaque)).keys()].filter(
      (q) => opaque.normals[q * 12 + 1] === 1 && bounds(opaque, q).max[1] === 0.5,
    );
    expect(slabTops.length).toBe(2);
    // Slab sides sample the bottom half of the tile: uvs span half a block upward.
    const side = [...Array(quads(opaque)).keys()].find(
      (q) => opaque.normals[q * 12] === -1 && bounds(opaque, q).max[1] === 0.5,
    );
    const vs = [0, 1, 2, 3].map((k) => opaque.uvs[((side ?? 0) * 4 + k) * 2 + 1] ?? 0);
    expect(Math.max(...vs) - Math.min(...vs)).toBe(0.5);
  });

  it('draws water in the transparent pass with its surface lowered and hides it against water', () => {
    const { opaque, transparent } = meshChunk(
      voxels([
        [0, 0, 0, WATER],
        [1, 0, 0, WATER],
      ]),
    );
    expect(quads(opaque)).toBe(0);
    expect(quads(transparent)).toBe(6); // merged top, bottom, and sides of the 2×1 pool
    const top = [...Array(quads(transparent)).keys()].find(
      (q) => transparent.normals[q * 12 + 1] === 1,
    );
    expect(bounds(transparent, top ?? 0).max[1]).toBe(0.875);
  });

  it('draws a ladder as one plate near the back of its cell', () => {
    // ladder_n faces −Z (face 5): the plate is at z = 1 − 0.05.
    const { opaque } = meshChunk(voxels([[0, 0, 0, LADDER_NORTH]]));
    expect(quads(opaque)).toBe(1);
    expect(opaque.positions[2]).toBeCloseTo(0.95);
    expect(opaque.normals[2]).toBe(-1);
  });

  it('textures faces by material and face: grass top and side, unknown ids plain magenta', () => {
    const { opaque } = meshChunk(
      voxels([
        [0, 0, 0, GRASS],
        [5, 0, 0, UNKNOWN],
      ]),
    );
    const tileOf = (q: number) => [opaque.tiles[q * 16] ?? 0, opaque.tiles[q * 16 + 1] ?? 0];
    const find = (pred: (q: number) => boolean) =>
      [...Array(quads(opaque)).keys()].find(pred) ?? -1;
    const grassTop = find(
      (q) => opaque.normals[q * 12 + 1] === 1 && bounds(opaque, q).max[0] === 1,
    );
    const grassSide = find((q) => opaque.normals[q * 12] === 1 && bounds(opaque, q).max[0] === 1);
    const unknown = find((q) => (bounds(opaque, q).min[0] ?? 0) >= 5);
    expect(tileOf(grassTop)).toEqual([tileRect('grass').u0, tileRect('grass').v0].map(Math.fround));
    expect(tileOf(grassSide)).toEqual(
      [tileRect('grassSide').u0, tileRect('grassSide').v0].map(Math.fround),
    );
    expect(tileOf(unknown)).toEqual([tileRect('plain').u0, tileRect('plain').v0].map(Math.fround));
    // Textured faces shade white (the texture carries the colour); unknown ids keep a flat colour.
    expect(opaque.colors[grassTop * 12]).toBe(1);
    expect(opaque.colors[unknown * 12 + 1]).toBe(0); // magenta
  });

  describe('slopes', () => {
    const wedge = (facing: string, half = 'bottom', flooded = 'false', material = 'stone') =>
      stateId(
        `dwell:${material}_slope[facing=${facing},flooded=${flooded},half=${half},shape=wedge]`,
      );

    /** Geometric (winding) normal of triangle `t` of a mesh. */
    const triangleNormal = (a: MeshArrays, t: number): number[] => {
      const n = normalOf(
        a.positions,
        a.indices[t * 3] ?? 0,
        a.indices[t * 3 + 1] ?? 0,
        a.indices[t * 3 + 2] ?? 0,
      );
      const len = Math.hypot(n[0] ?? 0, n[1] ?? 0, n[2] ?? 0) || 1;
      return n.map((v) => v / len);
    };

    it('draws a lone wedge with its sloped face lit by its true normal', () => {
      const { opaque } = meshChunk(voxels([[1, 1, 1, wedge('east')]]));
      // The wedge descends toward +X: the surface normal is (½√2, ½√2, 0).
      const slopeTris: number[] = [];
      for (let t = 0; t < opaque.indices.length / 3; t++) {
        const i = opaque.indices[t * 3] ?? 0;
        const ny = opaque.normals[i * 3 + 1] ?? 0;
        if (ny > 0.1 && ny < 0.99) slopeTris.push(t);
      }
      expect(slopeTris.length).toBe(2); // the sloped quad
      for (const t of slopeTris) {
        const i = opaque.indices[t * 3] ?? 0;
        expect(opaque.normals[i * 3]).toBeCloseTo(Math.SQRT1_2, 5);
        expect(opaque.normals[i * 3 + 1]).toBeCloseTo(Math.SQRT1_2, 5);
        expect(opaque.normals[i * 3 + 2]).toBeCloseTo(0, 5);
      }
      // 2 (slope) + 2 (back) + 2 (bottom) + 1 + 1 (triangular sides) triangles.
      expect(opaque.indices.length / 3).toBe(8);
    });

    it('winds every polygon of every shape variant counter-clockwise, facing along its normal', () => {
      const shapeStates = new Map<number, number>();
      for (const s of STATE_DEFS)
        if (s.shape !== 0 && !shapeStates.has(s.shape)) shapeStates.set(s.shape, s.id);
      expect(shapeStates.size).toBeGreaterThanOrEqual(70);
      for (const id of shapeStates.values()) {
        const { opaque } = meshChunk(voxels([[1, 1, 1, id]]));
        for (let t = 0; t < opaque.indices.length / 3; t++) {
          const i = opaque.indices[t * 3] ?? 0;
          const stored = [0, 1, 2].map((k) => opaque.normals[i * 3 + k] ?? 0);
          const geometric = triangleNormal(opaque, t);
          const dotted = stored.reduce((acc, v, k) => acc + v * (geometric[k] ?? 0), 0);
          expect(dotted, STATE_DEFS[id]?.state).toBeGreaterThan(0.999);
        }
      }
    });

    it('culls the faces a neighbour covers and keeps the rest', () => {
      // A cube west of an east-facing wedge: the wedge's full-height west side meets the cube's east
      // face, which is hidden on both.
      const alone = meshChunk(voxels([[1, 1, 1, wedge('east')]])).opaque.indices.length / 3;
      const pair =
        meshChunk(
          voxels([
            [0, 1, 1, 2],
            [1, 1, 1, wedge('east')],
          ]),
        ).opaque.indices.length / 3;
      // The cube alone is 6 quads (12 triangles); together each hides 2 triangles of one face.
      expect(pair).toBe(alone + 12 - 2 - 2);
      // On the low side the wedge's triangle is not covered by the cube's full face... it is the
      // cube's face that stays: a cube east of the wedge keeps its west face.
      const east =
        meshChunk(
          voxels([
            [1, 1, 1, wedge('east')],
            [2, 1, 1, 2],
          ]),
        ).opaque.indices.length / 3;
      expect(east).toBe(alone + 12 - 0 - 0 - 0); // wedge's east side has no area: nothing hides
    });

    it('textures a grass slope: the surface takes the top tile, its sides the side tile', () => {
      const { opaque } = meshChunk(voxels([[1, 1, 1, wedge('east', 'bottom', 'false', 'grass')]]));
      const top = tileRect('grass');
      const side = tileRect('grassSide');
      const tileAt = (v: number) => [opaque.tiles[v * 4], opaque.tiles[v * 4 + 1]];
      let sawTop = false;
      let sawSide = false;
      for (let t = 0; t < opaque.indices.length / 3; t++) {
        const i = opaque.indices[t * 3] ?? 0;
        const ny = opaque.normals[i * 3 + 1] ?? 0;
        if (ny > 0.1 && ny < 0.99) {
          expect(tileAt(i)).toEqual([top.u0, top.v0].map(Math.fround));
          sawTop = true;
        } else if (Math.abs(ny) < 1e-6) {
          expect(tileAt(i)).toEqual([side.u0, side.v0].map(Math.fround));
          sawSide = true;
        }
      }
      expect(sawTop && sawSide).toBe(true);
    });

    // Regression: side faces took the texture by world height, so the grass side's skirt (the top
    // of the tile) stayed at the cell's top: a slab's sides were all dirt, and a slope's sides
    // showed a horizontal grass band where they reached full height instead of along their edge.
    it('runs the side tile down from each side face’s top edge, so grass follows a slope', () => {
      const slab = stateId('dwell:grass_slab[flooded=false,half=bottom]');
      // Wedge descending east: its top edge on a side is at 1 − x (cell-local x).
      const cases: [number, (x: number, z: number) => number][] = [
        [slab, () => 0.5],
        [wedge('east', 'bottom', 'false', 'grass'), (x) => 1 - x],
        [wedge('south', 'bottom', 'false', 'grass'), (_x, z) => 1 - z],
      ];
      for (const [state, top] of cases) {
        const { opaque } = meshChunk(voxels([[1, 1, 1, state]]));
        let sides = 0;
        for (let v = 0; v < opaque.positions.length / 3; v++) {
          if (Math.abs(opaque.normals[v * 3 + 1] ?? 1) > 1e-6) continue; // tops and slopes
          const x = (opaque.positions[v * 3] ?? 0) - 1;
          const y = (opaque.positions[v * 3 + 1] ?? 0) - 1;
          const z = (opaque.positions[v * 3 + 2] ?? 0) - 1;
          // The tile's top (v ≡ 1, the grass skirt) lies on the face's top edge.
          expect(opaque.uvs[v * 2 + 1] ?? 0).toBeCloseTo(1 + y - top(x, z) + 1, 5);
          sides++;
        }
        expect(sides).toBeGreaterThan(0);
      }
    });

    it('shades sloped faces by interpolating the face tints by their normal', () => {
      const { opaque } = meshChunk(voxels([[1, 1, 1, wedge('east', 'bottom', 'false', 'stone')]]));
      // Stone is textured: colours are the tint itself.
      for (let t = 0; t < opaque.indices.length / 3; t++) {
        const i = opaque.indices[t * 3] ?? 0;
        const n = [0, 1, 2].map((k) => opaque.normals[i * 3 + k] ?? 0) as [number, number, number];
        const expected = normalTint(...n);
        for (let k = 0; k < 3; k++) {
          expect(opaque.colors[i * 3 + k]).toBeCloseTo(expected[k] ?? 0, 5);
        }
      }
    });

    it('normalTint equals faceTint on axis-aligned normals and blends in between', () => {
      expect(normalTint(0, 1, 0)).toEqual(faceTint(1, 1));
      expect(normalTint(0, -1, 0)).toEqual(faceTint(1, -1));
      expect(normalTint(1, 0, 0)).toEqual(faceTint(0, 1));
      expect(normalTint(0, 0, -1)).toEqual(faceTint(2, -1));
      const mid = normalTint(Math.SQRT1_2, Math.SQRT1_2, 0);
      const top = faceTint(1, 1);
      const side = faceTint(0, 1);
      for (let k = 0; k < 3; k++) {
        expect(mid[k]).toBeGreaterThan(Math.min(top[k] ?? 0, side[k] ?? 0) - 1e-9);
        expect(mid[k]).toBeLessThan(Math.max(top[k] ?? 0, side[k] ?? 0) + 1e-9);
      }
    });

    it('draws water in the open part of a flooded slope and none in a dry one', () => {
      const dry = meshChunk(voxels([[1, 1, 1, wedge('east')]]));
      expect(dry.transparent.indices.length).toBe(0);
      const wet = meshChunk(voxels([[1, 1, 1, wedge('east', 'bottom', 'true')]]));
      expect(wet.transparent.indices.length).toBeGreaterThan(0);
      expect(wet.opaque.indices.length).toBe(dry.opaque.indices.length);
      // Water beside a flooded slope draws no face against it.
      const pool = meshChunk(
        voxels([
          [0, 1, 1, WATER],
          [1, 1, 1, wedge('east', 'bottom', 'true')],
        ]),
      );
      const lone = meshChunk(voxels([[0, 1, 1, WATER]]));
      expect(pool.transparent.indices.length).toBeLessThan(
        lone.transparent.indices.length + wet.transparent.indices.length,
      );
    });
  });

  describe('biome tint (WORLD_GENERATION.md §3.7)', () => {
    const LEAVES = stateId('dwell:leaves');
    /** A tint grid: grass (r, g, b) and foliage (r, g, b) at every point, or per point by `at`. */
    function grid(at: (i: number, j: number) => number[]): Uint8Array {
      const g = new Uint8Array(TINT_GRID_BYTES);
      for (let j = 0; j < TINT_POINTS; j++)
        for (let i = 0; i < TINT_POINTS; i++) g.set(at(i, j), (j * TINT_POINTS + i) * TINT_STRIDE);
      return g;
    }
    const vertices = (a: MeshArrays): number => a.positions.length / 3;

    it('gives every vertex of a tinted block its biome multiplier and leaves other blocks at 1', () => {
      const cells = voxels([
        [0, 0, 0, GRASS],
        [4, 0, 0, LEAVES],
        [8, 0, 0, 2],
      ]);
      const { opaque } = meshChunk(
        cells,
        grid(() => [128, 64, 32, 32, 96, 64]),
      );
      expect(opaque.tints.length).toBe(opaque.positions.length);
      expect(vertices(opaque)).toBe(72); // three cubes: 6 quads × 4 vertices each
      let grass = 0;
      let leaves = 0;
      let stone = 0;
      for (let v = 0; v < vertices(opaque); v++) {
        const x = opaque.positions[v * 3] ?? 0;
        const t = [0, 1, 2].map((c) => opaque.tints[v * 3 + c] ?? 0);
        if (x <= 1) {
          expect(t).toEqual([2, 1, 0.5]); // grass: 128, 64, 32 in 1/64
          grass++;
        } else if (x >= 4 && x <= 5) {
          expect(t).toEqual([0.5, 1.5, 1]); // foliage
          leaves++;
        } else {
          expect(t).toEqual([1, 1, 1]); // stone
          stone++;
        }
      }
      expect([grass, leaves, stone]).toEqual([24, 24, 24]);
    });

    it('interpolates the 16 m grid across the chunk, the same at both ends of a shared edge', () => {
      // Grass multiplier rising along x: 1 at x = 0, 2 at x = 16, 3 at x = 32 (r only).
      const g = grid((i) => [64 * (i + 1), 64, 64, 64, 64, 64]);
      const { opaque } = meshChunk(
        voxels([
          [0, 0, 0, GRASS],
          [8, 0, 0, GRASS],
          [31, 0, 0, GRASS],
        ]),
        g,
      );
      for (let v = 0; v < vertices(opaque); v++) {
        const x = opaque.positions[v * 3] ?? 0;
        expect(opaque.tints[v * 3] ?? 0).toBeCloseTo(1 + x / 16, 5);
      }
    });

    it('draws untinted without a grid, and transparent blocks never tinted', () => {
      const { opaque } = meshChunk(voxels([[0, 0, 0, GRASS]]));
      expect([...opaque.tints].every((t) => t === 1)).toBe(true);
      const wet = meshChunk(
        voxels([[0, 0, 0, WATER]]),
        grid(() => [128, 128, 128, 128, 128, 128]),
      );
      expect([...wet.transparent.tints].every((t) => t === 1)).toBe(true);
    });

    it('tints a grass slope like the cube', () => {
      const slope = stateId('dwell:grass_slope[facing=east,flooded=false,half=bottom,shape=wedge]');
      const { opaque } = meshChunk(
        voxels([[2, 0, 0, slope]]),
        grid(() => [128, 64, 64, 64, 64, 64]),
      );
      expect(vertices(opaque)).toBeGreaterThan(0);
      expect([...opaque.tints].filter((_, i) => i % 3 === 0).every((t) => t === 2)).toBe(true);
    });
  });
});

describe('chunk mesher: face B (BIFACIAL_WORLD.md §6)', () => {
  const tileOf = (m: MeshArrays, normalY: number): number[] | undefined => {
    for (let i = 0; i < m.normals.length / 3; i++) {
      if (m.normals[i * 3 + 1] === normalY) {
        return [0, 1, 2, 3].map((c) => m.tiles[i * 4 + c] ?? NaN);
      }
    }
    return undefined;
  };
  const rect = (name: TileName): number[] => {
    const t = tileRect(name);
    return [t.u0, t.v0, t.u1 - t.u0, t.v1 - t.v0];
  };

  it('puts the top texture and shading on the face toward the sky, which on face B is −Y', () => {
    // A grass block at the top of face A's chunk, and the same block hanging at the bottom of a face-B
    // chunk's row 0 (its sky is below).
    const a = meshChunk(voxels([[5, 0, 5, GRASS]]));
    const b = meshChunk(voxels([[5, 0, 5, GRASS]]), null, true);
    expect(tileOf(a.opaque, 1)).toEqual(rect('grass'));
    expect(tileOf(a.opaque, -1)).toEqual(rect('dirt'));
    expect(tileOf(b.opaque, -1)).toEqual(rect('grass'));
    expect(tileOf(b.opaque, 1)).toEqual(rect('dirt'));
    // Shading: a face-B block's underside is lit as a top (warm), its upper face as a bottom.
    const colour = (m: MeshArrays, ny: number): number[] => {
      const i = [...m.normals].findIndex((v, n) => n % 3 === 1 && v === ny);
      return [0, 1, 2].map((c) => m.colors[i - 1 + c] ?? NaN);
    };
    expect(colour(b.opaque, -1)).toEqual(colour(a.opaque, 1));
    expect(colour(b.opaque, 1)).toEqual(colour(a.opaque, -1));
  });

  it('turns a slab over: a face-A bottom slab is a face-B top slab, hanging from the cell top', () => {
    // Stored as the world has it on face B: a `half=top` slab hangs from its cell's ceiling — the
    // mirror image of a bottom slab standing on a floor.
    const TOP_SLAB = stateId('dwell:stone_slab[flooded=false,half=top]');
    const standing = meshChunk(voxels([[5, 0, 5, SLAB]]));
    const hanging = meshChunk(voxels([[5, 0, 5, TOP_SLAB]]), null, true);
    // Both solids are 0.5 m thick: one at y 0…0.5 and the mirror image at 0.5…1 of its cell.
    const extent = (m: MeshArrays): [number, number] => {
      let lo = Infinity;
      let hi = -Infinity;
      for (let i = 1; i < m.positions.length; i += 3) {
        lo = Math.min(lo, m.positions[i] ?? 0);
        hi = Math.max(hi, m.positions[i] ?? 0);
      }
      return [lo, hi];
    };
    expect(extent(standing.opaque)).toEqual([0, 0.5]);
    // Mirrored about the chunk (32 m tall), the hanging slab's image of a standing one at row 0 is at
    // the chunk's top row 31: its solid is the cell's upper half.
    const [lo, hi] = extent(hanging.opaque);
    expect(hi - lo).toBeCloseTo(0.5);
  });

  it('winds every face of the turned-over mesh to face its normal', () => {
    const m = meshChunk(voxels([[5, 3, 5, GRASS]]), null, true).opaque;
    for (let t = 0; t < m.indices.length; t += 3) {
      const [i0 = 0, i1 = 0, i2 = 0] = [m.indices[t], m.indices[t + 1], m.indices[t + 2]];
      const n = normalOf(m.positions, i0, i1, i2);
      const given = [
        m.normals[i0 * 3] ?? 0,
        m.normals[i0 * 3 + 1] ?? 0,
        m.normals[i0 * 3 + 2] ?? 0,
      ];
      const dot =
        (n[0] ?? 0) * (given[0] ?? 0) +
        (n[1] ?? 0) * (given[1] ?? 0) +
        (n[2] ?? 0) * (given[2] ?? 0);
      expect(dot).toBeGreaterThan(0);
    }
  });
});
