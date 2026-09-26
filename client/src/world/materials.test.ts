import { describe, expect, it } from 'vitest';
import { MATERIALS, materialStyle } from './materials';

describe('material styles', () => {
  it('mirror the C++ material table id for id (server/core/include/dwell/core/voxel.h)', () => {
    expect(MATERIALS.map((m) => m.name)).toEqual([
      'air',
      'bedrock',
      'stone',
      'dirt',
      'grass',
      'stone_slab',
      'ladder_n',
      'ladder_e',
      'ladder_s',
      'ladder_w',
      'water',
      'launch_pad',
      'sand',
      'sandstone',
      'gravel',
      'snow',
      'log',
      'leaves',
      'coal_ore',
      'iron_ore',
      'gold_ore',
    ]);
  });

  it('draws unknown ids in magenta', () => {
    expect(materialStyle(999).color).toBe(0xff00ff);
  });
});

describe('material textures', () => {
  it('every visible material is textured', () => {
    const untextured = MATERIALS.filter((m) => m.opacity > 0 && !m.textures).map((m) => m.name);
    expect(untextured).toEqual([]);
  });
});
