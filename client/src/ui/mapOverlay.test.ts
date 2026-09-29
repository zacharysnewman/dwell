import { describe, expect, it } from 'vitest';
import { BIOME_COLORS, mapColumn, mapPixels } from './mapOverlay';

function map(columns: [number, number, number][]): Uint8Array {
  const out = new Uint8Array(columns.length * 4);
  columns.forEach(([h, biome, flags], i) => {
    out[i * 4] = h & 0xff;
    out[i * 4 + 1] = (h >> 8) & 0xff;
    out[i * 4 + 2] = biome;
    out[i * 4 + 3] = flags;
  });
  return out;
}

describe('terrain map overlay', () => {
  it('decodes columns: signed height, biome, beyond the rim', () => {
    const bytes = map([
      [-540, 0, 0],
      [1950, 6, 0],
      [0, 2, 1],
      [12, 3, 0],
    ]);
    expect(mapColumn(bytes, 2, 0, 0)).toEqual({ height: -540, biome: 0, outside: false });
    expect(mapColumn(bytes, 2, 1, 0).height).toBe(1950);
    expect(mapColumn(bytes, 2, 0, 1).outside).toBe(true);
  });

  it('colours by biome, shades slopes, darkens deep oceans, and leaves the void black', () => {
    const px = mapPixels(
      map([
        [10, 2, 0],
        [10, 2, 0],
        [-300, 0, 0],
        [0, 2, 1],
      ]),
      2,
      8,
    );
    const plains = BIOME_COLORS[2] ?? 0;
    expect([px[0], px[1], px[2]]).toEqual([
      (plains >> 16) & 0xff,
      (plains >> 8) & 0xff,
      plains & 0xff,
    ]);
    const ocean = BIOME_COLORS[0] ?? 0;
    expect(px[8] ?? 0).toBeLessThan(((ocean >> 16) & 0xff) * 0.8);
    expect([px[12], px[13], px[14], px[15]]).toEqual([0, 0, 0, 255]);
  });
});
