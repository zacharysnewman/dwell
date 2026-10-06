import { describe, expect, it } from 'vitest';
import { MATERIALS, materialStyle, PLACEABLE } from './materials';

describe('material styles', () => {
  it('mirror the registry state for state, in id order', () => {
    expect(MATERIALS.map((m) => m.name)).toEqual([
      'dwell:air',
      'dwell:bedrock',
      'dwell:stone',
      'dwell:dirt',
      'dwell:grass',
      'dwell:stone_slab',
      'dwell:ladder[facing=north,flooded=false]',
      'dwell:ladder[facing=north,flooded=true]',
      'dwell:ladder[facing=east,flooded=false]',
      'dwell:ladder[facing=east,flooded=true]',
      'dwell:ladder[facing=south,flooded=false]',
      'dwell:ladder[facing=south,flooded=true]',
      'dwell:ladder[facing=west,flooded=false]',
      'dwell:ladder[facing=west,flooded=true]',
      'dwell:water',
      'dwell:launch_pad',
      'dwell:sand',
      'dwell:sandstone',
      'dwell:gravel',
      'dwell:snow',
      'dwell:log',
      'dwell:leaves',
      'dwell:coal_ore',
      'dwell:iron_ore',
      'dwell:gold_ore',
    ]);
  });

  it('mark the placeable set of the C++ registry (Placeable, block_edit_test.cpp)', () => {
    expect(PLACEABLE.map((id) => MATERIALS[id]?.name)).toEqual([
      'dwell:stone',
      'dwell:dirt',
      'dwell:grass',
      'dwell:stone_slab',
      'dwell:ladder[facing=north,flooded=false]',
      'dwell:ladder[facing=east,flooded=false]',
      'dwell:ladder[facing=south,flooded=false]',
      'dwell:ladder[facing=west,flooded=false]',
      'dwell:sand',
      'dwell:sandstone',
      'dwell:gravel',
      'dwell:snow',
      'dwell:log',
      'dwell:leaves',
      'dwell:coal_ore',
      'dwell:iron_ore',
      'dwell:gold_ore',
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
