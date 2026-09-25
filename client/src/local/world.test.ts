import { describe, expect, it } from 'vitest';
import { GENERATORS, parseLocalWorld } from './world';

describe('local world options', () => {
  it('defaults to procedural terrain with seed 0', () => {
    expect(parseLocalWorld('')).toEqual({ worldSeed: 0, generatorVersion: GENERATORS.terrain });
  });

  it('reads ?world= and ?seed=', () => {
    expect(parseLocalWorld('?world=playground&seed=42')).toEqual({
      worldSeed: 42,
      generatorVersion: GENERATORS.playground,
    });
    expect(parseLocalWorld('?world=flat').generatorVersion).toBe(GENERATORS.flat);
  });

  it('ignores unknown worlds and invalid seeds', () => {
    expect(parseLocalWorld('?world=moon&seed=-3')).toEqual({
      worldSeed: 0,
      generatorVersion: GENERATORS.terrain,
    });
    expect(parseLocalWorld('?seed=1.5').worldSeed).toBe(0);
  });
});
