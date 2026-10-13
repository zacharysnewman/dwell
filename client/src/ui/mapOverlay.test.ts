import { describe, expect, it } from 'vitest';
import {
  DISC_ZOOM,
  MAP_COLUMN_BYTES,
  MAP_SIZE,
  MAP_ZOOMS,
  SEA_SHALLOW,
  SHORE_COLOR,
  formatMapDistance,
  heightColor,
  mapColumn,
  mapPixels,
  mapView,
} from './mapOverlay';

function map(columns: [number, number][]): Uint8Array {
  const out = new Uint8Array(columns.length * MAP_COLUMN_BYTES);
  columns.forEach(([h, flags], i) => {
    out[i * MAP_COLUMN_BYTES] = h & 0xff;
    out[i * MAP_COLUMN_BYTES + 1] = (h >> 8) & 0xff;
    out[i * MAP_COLUMN_BYTES + 2] = flags;
  });
  return out;
}

const rgb = (c: number) => [(c >> 16) & 0xff, (c >> 8) & 0xff, c & 0xff];

describe('terrain map overlay', () => {
  it('decodes columns: signed height, beyond the rim', () => {
    const bytes = map([
      [-540, 0],
      [1950, 0],
      [0, 1],
      [12, 0],
    ]);
    expect(mapColumn(bytes, 2, 0, 0)).toEqual({ height: -540, outside: false });
    expect(mapColumn(bytes, 2, 1, 0).height).toBe(1950);
    expect(mapColumn(bytes, 2, 0, 1).outside).toBe(true);
  });

  it('colours by height: the sea by depth, the shore, the land from lowland to upland', () => {
    expect(heightColor(0)).toBe(SHORE_COLOR);
    expect(heightColor(1)).toBe(SHORE_COLOR);
    expect(heightColor(-0.01)).toBe(SEA_SHALLOW);
    expect(heightColor(2)).not.toBe(SHORE_COLOR);
    expect(heightColor(0.01 + 2)).not.toBe(SHORE_COLOR);
    expect(heightColor(-40)).not.toBe(heightColor(-1));
    expect(heightColor(150)).not.toBe(heightColor(10));
  });

  it('shades slopes on land, not the sea, and leaves the void black', () => {
    const px = mapPixels(
      map([
        [10, 0],
        [10, 0],
        [-5, 0],
        [0, 1],
      ]),
      2,
      8,
    );
    expect([px[0], px[1], px[2]]).toEqual(rgb(heightColor(10)));
    expect([px[8], px[9], px[10]]).toEqual(rgb(heightColor(-5)));
    expect([px[12], px[13], px[14], px[15]]).toEqual([0, 0, 0, 255]);
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
