import { describe, expect, it } from 'vitest';
import { LOD_PAD, LOD_VOLUME, lodCell } from '../lod/grid';
import { averageTileColor } from '../render/textures';
import { lodColor, meshSection, SURFACE_STRIDE } from './lodMesher';

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
        cells[lodCell(x, 1, z)] = 10; // water
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
    cells[lodCell(3, 4, 5)] = 4; // grass: its top face is lit at full shade
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
      expect(m.opaque.colors[top * 3 + ch]).toBeCloseTo(linear(rgb[ch] ?? 0), 4);
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
        cells[lodCell(x, 0, z)] = 10; // water (the cell holds the whole sea)
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
          cells[lodCell(x, 1, z)] = 10; // water
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
});
