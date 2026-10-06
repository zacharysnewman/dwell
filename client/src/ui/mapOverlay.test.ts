import { describe, expect, it } from 'vitest';
import {
  BIOME_COLORS,
  DISC_ZOOM,
  MAP_SIZE,
  MAP_ZOOMS,
  WATER_COLOR,
  formatMapDistance,
  mapColumn,
  mapPixels,
  mapView,
} from './mapOverlay';

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
    expect(mapColumn(bytes, 2, 0, 0)).toEqual({
      height: -540,
      biome: 0,
      outside: false,
      river: false,
    });
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

  it('draws river and lake water over the biome (flag 2), the sea by its biome', () => {
    const bytes = map([
      [120, 2, 2],
      [120, 2, 0],
    ]);
    expect(mapColumn(bytes, 2, 0, 0).river).toBe(true);
    expect(mapColumn(bytes, 2, 1, 0).river).toBe(false);
    const px = mapPixels(bytes, 2, 8);
    expect([px[0], px[1], px[2]]).toEqual([
      (WATER_COLOR >> 16) & 0xff,
      (WATER_COLOR >> 8) & 0xff,
      WATER_COLOR & 0xff,
    ]);
    const plains = BIOME_COLORS[2] ?? 0;
    expect(px[4]).toBe((plains >> 16) & 0xff);
  });

  it('zooms from the surroundings out to the whole disc, centred on the player until then', () => {
    // The first zoom is the old fixed map: 128 columns of 8 m around the player.
    expect(mapView(0, 1000, -2000)).toEqual({ x0: 1000 - 512, z0: -2000 - 512, step: 8 });
    // Every zoom is a wider view; the last covers the whole 16,384 km disc about the origin,
    // wherever the player is.
    for (let z = 1; z < MAP_ZOOMS.length; z++) {
      expect(MAP_ZOOMS[z] ?? 0).toBeGreaterThan(MAP_ZOOMS[z - 1] ?? 0);
    }
    const disc = mapView(DISC_ZOOM, 3_000_000, -5_000_000);
    expect(disc.step * MAP_SIZE).toBe(16_384_000);
    expect(disc.x0).toBe(-8_192_000);
    expect(disc.z0).toBe(-8_192_000);
    expect(mapView(DISC_ZOOM, 0, 0)).toEqual(disc);
    // A wide zoom short of the disc stays on the player.
    expect(mapView(DISC_ZOOM - 1, 10_000, 20_000).x0).toBe(10_000 - (MAP_SIZE / 2) * 32768);
    // Out of range zooms clamp.
    expect(mapView(99, 0, 0)).toEqual(disc);
    expect(mapView(-3, 0, 0).step).toBe(8);
  });

  it('writes distances for a caption', () => {
    expect(formatMapDistance(8)).toBe('8 m');
    expect(formatMapDistance(1024)).toBe('1 km');
    expect(formatMapDistance(65_536)).toBe('65.5 km');
    expect(formatMapDistance(16_384_000)).toBe('16384 km');
  });
});
