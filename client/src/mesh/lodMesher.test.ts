import { describe, expect, it } from 'vitest';
import { LOD_VOLUME, lodCell } from '../lod/grid';
import { averageTileColor } from '../render/textures';
import { lodColor, meshSection } from './lodMesher';

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

  it('draws liquids opaque at coarse levels, hiding what is below and with skirts', () => {
    const cells = new Uint16Array(LOD_VOLUME);
    for (let z = -1; z <= 32; z++) {
      for (let x = -1; x <= 32; x++) {
        cells[lodCell(x, 0, z)] = 12; // sand
        cells[lodCell(x, 1, z)] = 10; // water
      }
    }
    const m = meshSection(cells, true);
    expect(quads(m.water)).toBe(0);
    // The water's top, and the sand's bottom (nothing below it); the sand's top is hidden.
    expect(quads(m.opaque)).toBe(2);
    // Sides of both layers are hidden by the apron: skirts, water included.
    for (const face of [0, 1, 4, 5]) expect(quads(m.skirts[face] ?? m.water)).toBe(2);
  });
});
