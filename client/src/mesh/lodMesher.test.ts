import { describe, expect, it } from 'vitest';
import { LOD_VOLUME, lodCell } from '../lod/grid';
import { averageTileColor, srgbToLinear } from '../render/textures';
import { lodColor, meshSection, tintedColor } from './lodMesher';

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

  it('tint mode leaves liquids out and recolours the floor as seen through them', () => {
    const cells = new Uint16Array(LOD_VOLUME);
    for (let z = -1; z <= 32; z++) {
      for (let x = -1; x <= 32; x++) {
        cells[lodCell(x, 0, z)] = 12; // sand
        cells[lodCell(x, 1, z)] = 10; // water
      }
    }
    const m = meshSection(cells, 'tint');
    expect(quads(m.water)).toBe(0);
    // The sand's top (under water, tinted) and bottom (open, its own colour); sides are skirts.
    expect(quads(m.opaque)).toBe(2);
    const n = m.opaque.normals;
    const c = m.opaque.colors;
    const top = [0, 1, 2, 3].map((q) => q * 4).find((v) => n[v * 3 + 1] === 1) ?? -1;
    expect(top).toBeGreaterThanOrEqual(0);
    const expected = tintedColor(12, 0, 10);
    expect(c[top * 3 + 1]).toBeCloseTo(srgbToLinear((expected >> 8) & 0xff), 4);
    // Bluer than dry sand, still not the water's own colour.
    expect(expected).not.toBe(lodColor(12, 0));
    expect(expected & 0xff).toBeGreaterThan(lodColor(12, 0) & 0xff);
    expect(expected).not.toBe(lodColor(10, 0));
  });
});
