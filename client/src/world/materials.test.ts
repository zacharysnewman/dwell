import { describe, expect, it } from 'vitest';
import { MATERIALS, materialStyle, PLACEABLE } from './materials';

describe('material styles', () => {
  it('mirror the registry state for state, in id order', () => {
    // The explicit blocks first, then each shapeable material's slope and slab families.
    const explicit = MATERIALS.map((m) => m.name).slice(0, 28);
    expect(explicit).toEqual([
      'dwell:air',
      'dwell:bedrock',
      'dwell:stone',
      'dwell:dirt',
      'dwell:grass',
      'dwell:stone_slab[flooded=false,half=bottom]',
      'dwell:stone_slab[flooded=false,half=top]',
      'dwell:stone_slab[flooded=true,half=bottom]',
      'dwell:stone_slab[flooded=true,half=top]',
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
    const families = MATERIALS.slice(28).map((m) => m.name.replace(/\[.*$/, ''));
    expect(new Set(families)).toEqual(
      new Set(
        ['stone', 'dirt', 'grass', 'sand', 'sandstone', 'gravel', 'snow', 'log']
          .flatMap((m) => [`dwell:${m}_slope`, `dwell:${m}_slab`])
          .filter((n) => n !== 'dwell:stone_slab'),
      ),
    );
  });

  it('mark the placeable set of the C++ registry (Placeable, block_edit_test.cpp)', () => {
    // The shaped families are all placeable (the shape key picks among them); the other states are
    // the palette's slots.
    const names = PLACEABLE.map((id) => MATERIALS[id]?.name ?? '');
    const shaped = names.filter((n) => /_(slope|slab)\[/.test(n));
    expect(shaped.length).toBe(8 * (144 + 4));
    expect(names.filter((n) => !shaped.includes(n))).toEqual([
      'dwell:stone',
      'dwell:dirt',
      'dwell:grass',
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
    expect(materialStyle(65_000).color).toBe(0xff00ff);
  });
});

describe('material textures', () => {
  it('every visible material is textured', () => {
    const untextured = MATERIALS.filter((m) => m.opacity > 0 && !m.textures).map((m) => m.name);
    expect(untextured).toEqual([]);
  });
});
