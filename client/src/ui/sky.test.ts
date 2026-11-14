import { describe, expect, it } from 'vitest';
import { ORBIT_WORLDS, SKY_PRESETS, orbitWorldsOf, skyOptions, skyPreset } from './sky';

describe('the Sky choice (REALISTIC_WORLD.md §8)', () => {
  it("offers orbits.json's presets, the default first", () => {
    expect(SKY_PRESETS.map((p) => p.name)).toEqual(['default', 'day_varies', 'extreme']);
    expect(SKY_PRESETS.map((p) => p.label)).toEqual(['Default', 'Day varies', 'Extreme']);
  });

  it('only a world whose climate reads the sky rolls an orbit: Realistic, not the starter', () => {
    expect(ORBIT_WORLDS).toEqual(['test:realistic']);
    expect(skyOptions('test:realistic', 'extreme')).toEqual({ sky: 'extreme' });
    expect(skyOptions('test:realistic', '')).toEqual({ sky: 'default' });
    expect(skyOptions('test:realistic', 'nope')).toEqual({ sky: 'default' });
    expect(skyOptions('test:starter', 'extreme')).toEqual({});
    expect(skyOptions('', 'extreme')).toEqual({});
  });

  it('a world file added with a climate by data alone rolls one', () => {
    const worlds = orbitWorldsOf([
      { name: 'test:mine', overrides: { tectonics: { climate: 1 } } },
      { name: 'test:plain', overrides: {} },
    ]);
    expect(worlds).toEqual(['test:mine']);
    expect(skyOptions('test:mine', 'day_varies', worlds)).toEqual({ sky: 'day_varies' });
    expect(skyPreset(undefined)).toBe('default');
  });
});
