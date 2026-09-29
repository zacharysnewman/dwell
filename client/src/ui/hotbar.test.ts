import { describe, expect, it } from 'vitest';
import { sharedAtlas, TILE, tilePixels, tileRect, ATLAS_SIZE } from '../render/textures';
import { slotForKey, slotKey } from './hotbar';

describe('hotbar keys', () => {
  it('number keys 1–9 pick the first nine slots and 0 the tenth', () => {
    expect(slotForKey('Digit1')).toBe(0);
    expect(slotForKey('Digit9')).toBe(8);
    expect(slotForKey('Digit0')).toBe(9);
    expect(slotForKey('KeyA')).toBeNull();
    expect([0, 8, 9, 10].map(slotKey)).toEqual(['1', '9', '0', '']);
  });
});

describe('hotbar swatches', () => {
  it('copy a tile out of the atlas, top row first', () => {
    const atlas = sharedAtlas();
    const px = tilePixels(atlas, 'stone');
    expect(px.length).toBe(TILE * TILE * 4);
    // The swatch's top-left texel is the tile's top row (highest v) in the atlas.
    const r = tileRect('stone');
    const x = Math.round(r.u0 * ATLAS_SIZE);
    const y = Math.round(r.v1 * ATLAS_SIZE) - 1;
    const o = (y * ATLAS_SIZE + x) * 4;
    expect([...px.subarray(0, 4)]).toEqual([...atlas.data.subarray(o, o + 4)]);
  });
});
