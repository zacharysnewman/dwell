import { describe, expect, it } from 'vitest';
import { MATERIALS, materialStyle, PLACEABLE } from './materials';

describe('material styles', () => {
  it('mirror the registry state for state, in id order', () => {
    expect(MATERIALS.map((m) => m.name)).toEqual([
      'dwell:air',
      'dwell:stone',
      'dwell:dirt',
      'dwell:grass',
      'dwell:ladder[facing=north,flooded=false]',
      'dwell:ladder[facing=north,flooded=true]',
      'dwell:ladder[facing=east,flooded=false]',
      'dwell:ladder[facing=east,flooded=true]',
      'dwell:ladder[facing=south,flooded=false]',
      'dwell:ladder[facing=south,flooded=true]',
      'dwell:ladder[facing=west,flooded=false]',
      'dwell:ladder[facing=west,flooded=true]',
      'dwell:water',
      'dwell:sand',
    ]);
  });

  it('mark the placeable set of the C++ registry (Placeable, block_edit_test.cpp)', () => {
    expect(PLACEABLE.map((id) => MATERIALS[id]?.name ?? '')).toEqual([
      'dwell:stone',
      'dwell:dirt',
      'dwell:grass',
      'dwell:ladder[facing=north,flooded=false]',
      'dwell:ladder[facing=east,flooded=false]',
      'dwell:ladder[facing=south,flooded=false]',
      'dwell:ladder[facing=west,flooded=false]',
      'dwell:sand',
    ]);
  });

  it('draws unknown ids in magenta', () => {
    expect(materialStyle(65_000).color).toBe(0xff00ff);
  });
});

describe('material textures', () => {
  it('every visible material is textured', () => {
    const untextured = MATERIALS.filter((m) => m.opacity > 0 && !m.textures).map((m) => m.name);
    expect(untextured).toEqual([]);
  });
});
