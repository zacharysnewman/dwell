import { describe, expect, it } from 'vitest';
import { LOD_PAD, LOD_VOLUME, lodCell } from '../lod/grid';
import { faceTint } from '../render/look';
import { averageTileColor } from '../render/textures';
import { stateId } from '../world/blocks';
import { lodColor, LodTint, meshSection, SURFACE_STRIDE } from './lodMesher';

const WATER = stateId('dwell:water');
const quads = (m: { indices: Uint32Array }): number => m.indices.length / 6;

describe('LOD section mesher (§6.6)', () => {
  it('draws a lone cell as six faces, in cell units', () => {
    const cells = new Uint16Array(LOD_VOLUME);
    cells[lodCell(3, 4, 5)] = 2;
    const m = meshSection(cells);
    expect(quads(m.opaque)).toBe(6);
    expect(Math.max(...m.opaque.positions)).toBe(6);
    expect(m.skirts.every((s) => quads(s) === 0)).toBe(true);
  });

  it('merges a floor, and puts border faces the apron hides into per-side skirts', () => {
    // A 32 × 32 floor at y = 0 with its neighbours' floors (the apron) around it.
    const cells = new Uint16Array(LOD_VOLUME);
    for (let z = -1; z <= 32; z++) for (let x = -1; x <= 32; x++) cells[lodCell(x, 0, z)] = 4;
    const m = meshSection(cells);
    // Top and bottom each one merged quad; the sides are all hidden by the apron.
    expect(quads(m.opaque)).toBe(2);
    for (const face of [0, 1, 4, 5]) expect(quads(m.skirts[face] ?? m.opaque)).toBe(1);
    expect(quads(m.skirts[2] ?? m.opaque) + quads(m.skirts[3] ?? m.opaque)).toBe(0);
    // Without the apron the sides are ordinary faces.
    const alone = new Uint16Array(LOD_VOLUME);
    for (let z = 0; z < 32; z++) for (let x = 0; x < 32; x++) alone[lodCell(x, 0, z)] = 4;
    const a = meshSection(alone);
    expect(quads(a.opaque)).toBe(6);
    expect(a.skirts.every((s) => quads(s) === 0)).toBe(true);
  });

  it('draws water apart, hides it against water, and colours faces flat', () => {
    const cells = new Uint16Array(LOD_VOLUME);
    for (let z = -1; z <= 32; z++) {
      for (let x = -1; x <= 32; x++) {
        cells[lodCell(x, 0, z)] = 12; // sand
        cells[lodCell(x, 1, z)] = WATER; // water
      }
    }
    const m = meshSection(cells);
    expect(quads(m.water)).toBe(1); // only the surface: sides hidden by the apron's water
    // The sand's sides are hidden by the apron's sand (skirts); water never gets skirts.
    for (const face of [0, 1, 4, 5]) expect(quads(m.skirts[face] ?? m.water)).toBe(1);
    const top = lodColor(4, 0);
    expect(top).toBe(averageTileColor('grass'));
    expect(lodColor(2, 1)).toBe(averageTileColor('stone'));
  });

  it('writes linear vertex colours: the textured chunks are sRGB, decoded before lighting', () => {
    // Regression (phone playtest): sRGB bytes used as linear colours drew distant land paler.
    const cells = new Uint16Array(LOD_VOLUME);
    cells[lodCell(3, 4, 5)] = 4; // grass
    const m = meshSection(cells);
    const n = m.opaque.normals;
    let top = -1;
    for (let v = 0; v < n.length / 3; v++) if (n[v * 3 + 1] === 1) top = v;
    expect(top).toBeGreaterThanOrEqual(0);
    const srgb = averageTileColor('grass');
    const linear = (byte: number): number => {
      const c = byte / 255;
      return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
    };
    const rgb = [(srgb >> 16) & 0xff, (srgb >> 8) & 0xff, srgb & 0xff];
    for (let ch = 0; ch < 3; ch++) {
      expect(m.opaque.colors[top * 3 + ch]).toBeCloseTo(
        linear(rgb[ch] ?? 0) * (faceTint(1, 1)[ch] ?? 0),
        4,
      );
    }
  });

  it('draws a column top at its surface height, not at its cell top', () => {
    // Regression (playtest: distant land and seas looked too tall): cells fill from their bottom
    // voxel, so cell tops lift the ground by up to a cell — kilometres at the horizon.
    const cells = new Uint16Array(LOD_VOLUME);
    const surface = new Float32Array(LOD_PAD * LOD_PAD * SURFACE_STRIDE);
    const col = (x: number, z: number) => (x + 1 + LOD_PAD * (z + 1)) * SURFACE_STRIDE;
    for (let z = -1; z <= 32; z++) {
      for (let x = -1; x <= 32; x++) {
        cells[lodCell(x, 0, z)] = 4; // grass, one cell thick
        surface[col(x, z)] = x < 16 ? 0.5 : 1; // a terrace at half height, then the full cell
        surface[col(x, z) + 1] = 4;
        surface[col(x, z) + 2] = 1; // valid
      }
    }
    const m = meshSection(cells, { surface });
    const tops = new Set<number>();
    let wallUp = 0;
    const p = m.opaque.positions;
    const n = m.opaque.normals;
    for (let v = 0; v < p.length / 3; v++) {
      if (n[v * 3 + 1] === 1) tops.add(Math.round((p[v * 3 + 1] ?? 0) * 100) / 100);
      // The wall between the terraces faces −X at x = 16, from 0.5 to 1.
      if (n[v * 3] === -1 && p[v * 3] === 16) wallUp = Math.max(wallUp, p[v * 3 + 1] ?? 0);
    }
    expect([...tops].sort()).toEqual([0.5, 1]);
    expect(wallUp).toBeCloseTo(1, 5);
  });

  it("draws a sea floor inside a water cell under a water surface at the chunks' water height", () => {
    // Regression (playtest: a seam and a height step where near water met LOD water, and a hard
    // edge where coarse water stopped being drawn): every level draws the see-through surface,
    // 1/8 m below its cell top as the chunks draw it (waterDrop, in cells), over the true floor.
    const cells = new Uint16Array(LOD_VOLUME);
    const surface = new Float32Array(LOD_PAD * LOD_PAD * SURFACE_STRIDE);
    for (let z = -1; z <= 32; z++) {
      for (let x = -1; x <= 32; x++) {
        cells[lodCell(x, -1, z)] = 12; // sand below
        cells[lodCell(x, 0, z)] = WATER; // water (the cell holds the whole sea)
        const c = (x + 1 + LOD_PAD * (z + 1)) * SURFACE_STRIDE;
        surface[c] = 0.5;
        surface[c + 1] = 12; // a sand floor
        surface[c + 2] = 1 | 2; // valid, wet
      }
    }
    const waterDrop = 0.125 / 256; // level 8
    const m = meshSection(cells, { surface, waterDrop });
    const heights = (f: typeof m.opaque, up: boolean) => {
      const ys = new Set<number>();
      for (let v = 0; v < f.positions.length / 3; v++) {
        if ((f.normals[v * 3 + 1] ?? 0) === (up ? 1 : -1)) ys.add(f.positions[v * 3 + 1] ?? 0);
      }
      return [...ys];
    };
    expect(heights(m.water, true)).toEqual([1 - waterDrop]);
    expect(heights(m.opaque, true)).toEqual([0.5]);
    // One merged rectangle of water over the section.
    expect(quads(m.water)).toBe(1);
  });

  describe("water above sea level (rivers and lakes: not on the cells' grid)", () => {
    // Regression (playtest: distant rivers flooded their banks, and from the coarsest levels hung
    // as sheets hundreds of metres up): a liquid's top was drawn at its cell's top — right for the
    // sea, whose level is a cell boundary at every level, but a river at y = 121 in a 16 m cell
    // (112..128) was drawn at 128, and in a 512 m cell at 512. The surface data carries each wet
    // column's water level (4th float, in cells), and the water is drawn there.
    const waterDrop = 0.125 / 16; // level 4
    const ys = (f: { positions: Float32Array; normals: Float32Array }, axis: number, sign = 1) => {
      const out = new Set<number>();
      for (let v = 0; v < f.positions.length / 3; v++)
        if ((f.normals[v * 3 + axis] ?? 0) === sign) out.add(f.positions[v * 3 + 1] ?? 0);
      return [...out].sort((a, b) => a - b);
    };
    const surfaceOf = (
      at: (x: number, z: number) => { h: number; m: number; wet: boolean; water: number },
    ) => {
      const surface = new Float32Array(LOD_PAD * LOD_PAD * SURFACE_STRIDE);
      for (let z = -1; z <= 32; z++)
        for (let x = -1; x <= 32; x++) {
          const c = (x + 1 + LOD_PAD * (z + 1)) * SURFACE_STRIDE;
          const s = at(x, z);
          surface[c] = s.h;
          surface[c + 1] = s.m;
          surface[c + 2] = 1 | (s.wet ? 2 : 0);
          surface[c + 3] = s.water;
        }
      return surface;
    };

    it('draws a river over a floor in its cell at the water level, not the cell top', () => {
      // Floor 118 m and water 121 m in the cell 112..128: 0.375 and 0.5625 of a cell.
      const cells = new Uint16Array(LOD_VOLUME);
      for (let z = -1; z <= 32; z++)
        for (let x = -1; x <= 32; x++) {
          cells[lodCell(x, -1, z)] = 12;
          cells[lodCell(x, 0, z)] = WATER;
        }
      const surface = surfaceOf(() => ({ h: 0.375, m: 12, wet: true, water: 0.5625 }));
      const m = meshSection(cells, { surface, waterDrop });
      expect(ys(m.water, 1)).toEqual([0.5625 - waterDrop]);
      expect(Math.max(...ys(m.opaque, 1))).toBeLessThan(0.5625 - waterDrop);
    });

    it('caps a column of water cells at the water level', () => {
      // A lake 1.25 cells deep over a full sand cell: water cells at y = 1 and 2, the surface at 2.25.
      const cells = new Uint16Array(LOD_VOLUME);
      for (let z = -1; z <= 32; z++)
        for (let x = -1; x <= 32; x++) {
          cells[lodCell(x, 0, z)] = 12;
          cells[lodCell(x, 1, z)] = WATER;
          cells[lodCell(x, 2, z)] = WATER;
        }
      const surface = surfaceOf(() => ({ h: 1, m: 12, wet: true, water: 2.25 }));
      const m = meshSection(cells, { surface, waterDrop });
      expect(ys(m.water, 1)).toEqual([2.25 - waterDrop]);
    });

    it('keeps a floor that rounds up to half a cell below shallow water', () => {
      // Floor 0.28, water 0.3 of the cell: the floor's half-cell step (0.5) would cover the water.
      const cells = new Uint16Array(LOD_VOLUME);
      for (let z = -1; z <= 32; z++)
        for (let x = -1; x <= 32; x++) {
          cells[lodCell(x, -1, z)] = 12;
          cells[lodCell(x, 0, z)] = WATER;
        }
      const surface = surfaceOf(() => ({ h: 0.28, m: 12, wet: true, water: 0.3 }));
      const m = meshSection(cells, { surface, waterDrop });
      // (0.3 as the Float32 surface data holds it.)
      const [top = Number.NaN, ...more] = ys(m.water, 1);
      expect(more).toEqual([]);
      expect(top).toBeCloseTo(0.3 - waterDrop, 6);
      expect(Math.max(...ys(m.opaque, 1))).toBeLessThan(top);
    });

    it("ends water's side faces at the water level", () => {
      // A pool (x < 16: a water cell over a sand cell, the surface half way up it) beside
      // lower dry ground (x ≥ 16: the sand cell only): the pool's +X face stands from its cell's
      // bottom to the water level, not to the cell's top.
      const cells = new Uint16Array(LOD_VOLUME);
      for (let z = -1; z <= 32; z++)
        for (let x = -1; x <= 32; x++) {
          cells[lodCell(x, 0, z)] = 12;
          if (x < 16) cells[lodCell(x, 1, z)] = WATER;
        }
      const surface = surfaceOf((x) =>
        x < 16 ? { h: 0.75, m: 12, wet: true, water: 1.5 } : { h: 1, m: 12, wet: false, water: 0 },
      );
      const m = meshSection(cells, { surface, waterDrop });
      expect(ys(m.water, 1)).toEqual([1.5 - waterDrop]);
      expect(Math.max(...ys(m.water, 0, 1))).toBeCloseTo(1.5 - waterDrop, 6);
    });
  });

  it('never draws a sea floor in the water surface or over the cell below', () => {
    // Regression (playtest: z-fighting on distant water): a floor inside a water cell, rounded to
    // half cells, landed on the cell's bottom — a second top over the solid cell's own — or on its
    // top, level with the water surface (1/8 m, a sliver of a cell, above or below it).
    const upward = (f: {
      positions: Float32Array;
      normals: Float32Array;
      indices: Uint32Array;
    }) => {
      // Upward quads by height.
      const at = new Map<number, number>();
      for (let q = 0; q < f.indices.length / 6; q++) {
        const v = f.indices[q * 6] ?? 0;
        if (f.normals[v * 3 + 1] !== 1) continue;
        const y = f.positions[v * 3 + 1] ?? 0;
        at.set(y, (at.get(y) ?? 0) + 1);
      }
      return at;
    };
    for (const floor of [0.1, 0.9]) {
      const cells = new Uint16Array(LOD_VOLUME);
      const surface = new Float32Array(LOD_PAD * LOD_PAD * SURFACE_STRIDE);
      for (let z = -1; z <= 32; z++) {
        for (let x = -1; x <= 32; x++) {
          cells[lodCell(x, 0, z)] = 12; // sand below
          cells[lodCell(x, 1, z)] = WATER; // water
          const c = (x + 1 + LOD_PAD * (z + 1)) * SURFACE_STRIDE;
          surface[c] = 1 + floor;
          surface[c + 1] = 4; // a floor of another material than the cell below
          surface[c + 2] = 1 | 2; // valid, wet
        }
      }
      const waterDrop = 0.125 / 256; // level 8
      const m = meshSection(cells, { surface, waterDrop });
      const tops = upward(m.opaque);
      // One floor, drawn once, clearly under the water surface.
      expect([...tops.values()], `floor ${String(floor)}`).toEqual([1]);
      const [y = 1] = [...tops.keys()];
      expect(y, `floor ${String(floor)}`).toBeLessThanOrEqual(1.5);
      expect([...upward(m.water).keys()]).toEqual([2 - waterDrop]);
    }
  });

  it('closes steps between surfaces across the section border without skirts', () => {
    // Regression (playtest: sky-blue cracks along straight lines): where a column's surface was
    // lower than its neighbour across the border, the step between them went only into a skirt,
    // which is hidden when the neighbour section is drawn at the same level.
    const surface = new Float32Array(LOD_PAD * LOD_PAD * SURFACE_STRIDE);
    const cells = new Uint16Array(LOD_VOLUME);
    const set = (x: number, z: number, top: number, h: number) => {
      for (let y = -1; y <= top; y++) cells[lodCell(x, y, z)] = 4;
      const c = (x + 1 + LOD_PAD * (z + 1)) * SURFACE_STRIDE;
      surface[c] = h;
      surface[c + 1] = 4;
      surface[c + 2] = 1;
    };
    for (let z = -1; z <= 32; z++) {
      set(-1, z, 1, 1.5); // across the −X border: a full cell 1, surface half way up it
      set(0, z, 0, 0.5); // ours: surface half way up cell 0, a whole cell and a half lower
      for (let x = 1; x <= 32; x++) set(x, z, 0, 0.5);
    }
    // On the −X border plane, the neighbour's cell 0 shows above our surface: from 0.5 to 1,
    // facing us (+X), in the opaque mesh (the neighbour section draws its cell 1 itself).
    const covered = (m: ReturnType<typeof meshSection>, facing: number) => {
      const p = m.opaque.positions;
      const n = m.opaque.normals;
      let lo = Infinity;
      let hi = -Infinity;
      for (let v = 0; v < p.length / 3; v++) {
        if (p[v * 3] !== 0 || n[v * 3] !== facing) continue;
        lo = Math.min(lo, p[v * 3 + 1] ?? 0);
        hi = Math.max(hi, p[v * 3 + 1] ?? 0);
      }
      return [lo, hi];
    };
    expect(covered(meshSection(cells, { surface }), 1)).toEqual([0.5, 1]);
    // Both surfaces in cell 0, ours higher: our side shows from the neighbour's (0) to ours (0.5),
    // facing it (−X).
    cells.fill(0);
    surface.fill(0);
    for (let z = -1; z <= 32; z++) {
      set(-1, z, 0, 0);
      for (let x = 0; x <= 32; x++) set(x, z, 0, 0.5);
    }
    expect(covered(meshSection(cells, { surface }), -1)).toEqual([0, 0.5]);
  });

  describe('slopes (SLOPE_BLOCKS.md §3.2)', () => {
    /** A section whose column x has `top(x, z)` solid cells (y = 0 … top − 1) of grass. */
    function terrain(top: (x: number, z: number) => number): Uint16Array {
      const cells = new Uint16Array(LOD_VOLUME);
      for (let z = -1; z <= 32; z++)
        for (let x = -1; x <= 32; x++)
          for (let y = 0; y < top(x, z); y++) cells[lodCell(x, y, z)] = 4;
      return cells;
    }

    /** Triangles of a mesh as (vertices, unit normal). */
    function triangles(m: { positions: Float32Array; indices: Uint32Array }) {
      const out: { v: number[][]; n: number[] }[] = [];
      for (let t = 0; t < m.indices.length / 3; t++) {
        const v = [0, 1, 2].map((k) => {
          const i = m.indices[t * 3 + k] ?? 0;
          return [
            m.positions[i * 3] ?? 0,
            m.positions[i * 3 + 1] ?? 0,
            m.positions[i * 3 + 2] ?? 0,
          ];
        });
        const e1 = v[1]!.map((c, i) => c - (v[0]?.[i] ?? 0)); // eslint-disable-line @typescript-eslint/no-non-null-assertion
        const e2 = v[2]!.map((c, i) => c - (v[0]?.[i] ?? 0)); // eslint-disable-line @typescript-eslint/no-non-null-assertion
        const n = [
          (e1[1] ?? 0) * (e2[2] ?? 0) - (e1[2] ?? 0) * (e2[1] ?? 0),
          (e1[2] ?? 0) * (e2[0] ?? 0) - (e1[0] ?? 0) * (e2[2] ?? 0),
          (e1[0] ?? 0) * (e2[1] ?? 0) - (e1[1] ?? 0) * (e2[0] ?? 0),
        ];
        const len = Math.hypot(...n);
        out.push({ v, n: len > 0 ? n.map((c) => c / len) : n });
      }
      return out;
    }

    /** The mesh's top surface height at (x, z): the highest upward triangle over the point. */
    function heightAt(m: { positions: Float32Array; indices: Uint32Array }, x: number, z: number) {
      let best = -Infinity;
      for (const { v, n } of triangles(m)) {
        if ((n[1] ?? 0) < 0.1) continue;
        const [a, b, c] = v as [number[], number[], number[]];
        const d =
          ((b[2] ?? 0) - (c[2] ?? 0)) * ((a[0] ?? 0) - (c[0] ?? 0)) +
          ((c[0] ?? 0) - (b[0] ?? 0)) * ((a[2] ?? 0) - (c[2] ?? 0));
        if (Math.abs(d) < 1e-9) continue;
        const l1 =
          (((b[2] ?? 0) - (c[2] ?? 0)) * (x - (c[0] ?? 0)) +
            ((c[0] ?? 0) - (b[0] ?? 0)) * (z - (c[2] ?? 0))) /
          d;
        const l2 =
          (((c[2] ?? 0) - (a[2] ?? 0)) * (x - (c[0] ?? 0)) +
            ((a[0] ?? 0) - (c[0] ?? 0)) * (z - (c[2] ?? 0))) /
          d;
        const l3 = 1 - l1 - l2;
        if (l1 < -1e-6 || l2 < -1e-6 || l3 < -1e-6) continue;
        best = Math.max(best, l1 * (a[1] ?? 0) + l2 * (b[1] ?? 0) + l3 * (c[1] ?? 0));
      }
      return best;
    }

    it('draws a staircase of cells as sloped facets, within half a cell of the line it climbs', () => {
      // Column x holds top(x) cells: 5, 6, 7, 8 along +x (x 10 … 13), flat 5 before and 8 after: a
      // ramp rising one cell per column, drawn without section data (as a modified, downsampled
      // section is).
      const top = (x: number) => (x < 10 ? 5 : x > 13 ? 8 : x - 5);
      const cells = terrain((x) => top(x));
      const plain = meshSection(cells);
      const sloped = meshSection(cells, { slopes: true });
      // Tilted faces rise towards +x (their normals lean to −x), none lean sideways.
      const tilted = triangles(sloped.opaque).filter(
        (t) => (t.n[1] ?? 0) > 0.5 && (t.n[1] ?? 0) < 0.99,
      );
      expect(tilted.length).toBeGreaterThan(0);
      for (const t of tilted) {
        expect(t.n[0]).toBeLessThan(-0.2);
        expect(t.n[2]).toBeCloseTo(0, 3);
      }
      // A column's top is a piece within its own cell, so the surface stays within half a cell of
      // the line through the column tops, y = x + 0.5 − 5.5.
      for (let x = 11; x <= 12; x++) {
        const h = heightAt(sloped.opaque, x + 0.5, 16.5);
        expect(Math.abs(h - (x + 0.5 - 5.5))).toBeLessThanOrEqual(0.5);
      }
      expect(heightAt(sloped.opaque, 5.5, 16.5)).toBeCloseTo(5, 5); // the flat before is flat
      expect(heightAt(sloped.opaque, 20.5, 16.5)).toBeCloseTo(8, 5);
      // The terraced version is flat on each column, a half cell off the line in the middle of one.
      const flat = heightAt(plain.opaque, 11.5, 16.5);
      const steep = heightAt(plain.opaque, 11.1, 16.5);
      expect(flat).toBeCloseTo(steep, 5);
      expect(heightAt(sloped.opaque, 11.1, 16.5)).not.toBeCloseTo(
        heightAt(sloped.opaque, 11.9, 16.5),
        2,
      );
    });

    it('leaves flat ground exactly as it was', () => {
      const cells = terrain(() => 3);
      const plain = meshSection(cells);
      const sloped = meshSection(cells, { slopes: true });
      expect(sloped.opaque.indices.length).toBe(plain.opaque.indices.length);
      expect(Array.from(sloped.opaque.positions)).toEqual(Array.from(plain.opaque.positions));
    });

    it('keeps cliffs: a three-cell step stays a wall', () => {
      const cells = terrain((x) => (x < 16 ? 2 : 5));
      const m = meshSection(cells, { slopes: true });
      const tilted = triangles(m.opaque).filter((t) => (t.n[1] ?? 0) > 0.1 && (t.n[1] ?? 0) < 0.99);
      expect(tilted).toHaveLength(0);
      // The wall faces −x at x = 16 from the low top up to the high one.
      let wall = 0;
      for (const t of triangles(m.opaque)) {
        if ((t.n[0] ?? 0) < -0.99 && t.v.every((p) => p[0] === 16))
          wall = Math.max(wall, ...t.v.map((p) => p[1] ?? 0));
      }
      expect(wall).toBe(5);
    });

    it("turns generated sections' surfaces into slopes too, with surface heights", () => {
      // A gentle ground: the surface rises 0.5 cell per column across x 8 … 20, in cell 2.
      const cells = terrain(() => 3);
      const surface = new Float32Array(LOD_PAD * LOD_PAD * SURFACE_STRIDE);
      const col = (x: number, z: number) => (x + 1 + LOD_PAD * (z + 1)) * SURFACE_STRIDE;
      for (let z = -1; z <= 32; z++)
        for (let x = -1; x <= 32; x++) {
          const h = 2 + Math.min(1, Math.max(0, (x - 8) / 12));
          surface[col(x, z)] = h;
          surface[col(x, z) + 1] = 4;
          surface[col(x, z) + 2] = 1;
        }
      const m = meshSection(cells, { surface, slopes: true });
      for (let x = 9; x <= 19; x++) {
        const h = heightAt(m.opaque, x + 0.5, 16.5);
        const line = 2 + (x + 0.5 - 8.5) / 12; // the surface at the column's middle
        expect(Math.abs(h - line)).toBeLessThanOrEqual(0.5);
      }
    });
  });

  describe('biome tint', () => {
    const GRASS = stateId('dwell:grass');
    const pack = (r: number, g: number, b: number): number => (r << 16) | (g << 8) | b;
    /** A surface array whose every column carries the tints (and no surface of its own). */
    function tinted(grass: number, foliage: number): Float32Array {
      const surface = new Float32Array(LOD_PAD * LOD_PAD * SURFACE_STRIDE);
      for (let c = 0; c < LOD_PAD * LOD_PAD; c++) {
        surface[c * SURFACE_STRIDE + 4] = grass;
        surface[c * SURFACE_STRIDE + 5] = foliage;
      }
      return surface;
    }
    const floor = (m: number): Uint16Array => {
      const cells = new Uint16Array(LOD_VOLUME);
      for (let z = 0; z < 32; z++) for (let x = 0; x < 32; x++) cells[lodCell(x, 0, z)] = m;
      return cells;
    };

    it('multiplies a grass top by the column tints and leaves other materials alone', () => {
      const surface = tinted(pack(128, 64, 32), pack(64, 64, 64));
      const grass = meshSection(floor(GRASS), { surface });
      const plain = meshSection(floor(GRASS));
      const stone = meshSection(floor(2), { surface });
      const stoneNone = meshSection(floor(2));
      // The top's colour is the untinted one times (2, 1, 0.5); stone is as without tints.
      expect(grass.opaque.colors.length).toBe(plain.opaque.colors.length);
      for (let i = 0; i < grass.opaque.normals.length; i += 3) {
        if (grass.opaque.normals[i + 1] !== 1) continue; // the top face
        expect(grass.opaque.colors[i] ?? 0).toBeCloseTo((plain.opaque.colors[i] ?? 0) * 2, 5);
        expect(grass.opaque.colors[i + 1] ?? 0).toBeCloseTo(plain.opaque.colors[i + 1] ?? 0, 5);
        expect(grass.opaque.colors[i + 2] ?? 0).toBeCloseTo(
          (plain.opaque.colors[i + 2] ?? 0) / 2,
          5,
        );
      }
      expect([...stone.opaque.colors]).toEqual([...stoneNone.opaque.colors]);
    });

    it('does nothing without surface data, or for columns with no tint', () => {
      const cells = floor(GRASS);
      expect([...meshSection(cells, { surface: null }).opaque.colors]).toEqual([
        ...meshSection(cells).opaque.colors,
      ]);
      const none = new Float32Array(LOD_PAD * LOD_PAD * SURFACE_STRIDE);
      expect([...meshSection(cells, { surface: none }).opaque.colors]).toEqual([
        ...meshSection(cells).opaque.colors,
      ]);
    });

    it('blends between columns: a vertex between two biomes takes the mean', () => {
      const surface = new Float32Array(LOD_PAD * LOD_PAD * SURFACE_STRIDE);
      for (let z = -1; z <= 32; z++)
        for (let x = -1; x <= 32; x++) {
          const c = x + 1 + LOD_PAD * (z + 1);
          surface[c * SURFACE_STRIDE + 4] = x < 16 ? pack(64, 64, 64) : pack(128, 128, 128);
        }
      const t = new LodTint(surface);
      const at = (x: number): number => {
        const rgb = [1, 1, 1];
        t.apply('grass', x, 10, rgb);
        return rgb[0] ?? 0;
      };
      expect(at(8)).toBeCloseTo(1, 5);
      expect(at(24)).toBeCloseTo(2, 5);
      expect(at(16)).toBeCloseTo(1.5, 5); // the border between columns 15 and 16
    });
  });
});
