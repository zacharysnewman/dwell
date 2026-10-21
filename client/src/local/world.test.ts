import { describe, expect, it } from 'vitest';
import { GENERATORS, parseLocalWorld } from './world';

describe('local world options', () => {
  it('defaults to procedural terrain with seed 0', () => {
    expect(parseLocalWorld('')).toEqual({ worldSeed: 0, generatorVersion: GENERATORS.terrain });
  });

  it('reads ?world= and ?seed=', () => {
    expect(parseLocalWorld('?world=flat&seed=42')).toEqual({
      worldSeed: 42,
      generatorVersion: GENERATORS.flat,
    });
    // The movement playground was retired (Phase 15): terrain instead.
    expect(parseLocalWorld('?world=playground').generatorVersion).toBe(GENERATORS.terrain);
    expect(parseLocalWorld('?world=flat').generatorVersion).toBe(GENERATORS.flat);
    expect(parseLocalWorld('?world=sandbox').generatorVersion).toBe(GENERATORS.sandbox);
  });

  it('ignores unknown worlds and invalid seeds', () => {
    expect(parseLocalWorld('?world=moon&seed=-3')).toEqual({
      worldSeed: 0,
      generatorVersion: GENERATORS.terrain,
    });
    expect(parseLocalWorld('?seed=1.5').worldSeed).toBe(0);
  });

  it('reads the dev link ?spawn=b as a face-B spawn in a world file of its own', () => {
    expect(parseLocalWorld('?seed=7&spawn=b')).toEqual({
      worldSeed: 7,
      generatorVersion: GENERATORS.terrain,
      file: 'local-g8-s7-faceb',
      spawnFaceB: true,
    });
    expect(parseLocalWorld('?spawn=a').spawnFaceB).toBeUndefined();
    // By a portal's opening (BIFACIAL_WORLD.md §6), on either face, in a world file of its own.
    expect(parseLocalWorld('?seed=7&spawn=portal')).toEqual({
      worldSeed: 7,
      generatorVersion: GENERATORS.terrain,
      file: 'local-g8-s7-portal',
      spawnPortal: true,
    });
    expect(parseLocalWorld('?seed=7&spawn=portal-b')).toMatchObject({
      file: 'local-g8-s7-portal-b',
      spawnFaceB: true,
      spawnPortal: true,
    });
  });
});
