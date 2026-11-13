import { describe, expect, it } from 'vitest';
import {
  feetOf,
  GENERATORS,
  parseLocalWorld,
  sizeOptions,
  sizeScale,
  spawnOptions,
  WORLD_SIZES,
} from './world';

describe('local world options', () => {
  it('starts at a point given by ?at=x,y,z (docs/DEV_TOOLS.md)', () => {
    expect(parseLocalWorld('?seed=0&at=2364,141.5,7443')).toEqual({
      worldSeed: 0,
      generatorVersion: GENERATORS.terrain,
      spawnAt: [2364, 141.5, 7443],
    });
    expect(parseLocalWorld('?at=1,2').spawnAt).toBeUndefined();
    expect(parseLocalWorld('?at=1,x,3').spawnAt).toBeUndefined();
  });

  it('plays a game mode given by ?mode= (a known one only)', () => {
    expect(parseLocalWorld('?world=flat&mode=adventure').mode).toBe('adventure');
    expect(parseLocalWorld('?at=1,2,3&mode=adventure').mode).toBe('adventure');
    expect(parseLocalWorld('?mode=survival').mode).toBeUndefined();
  });

  it('makes a bigger terrain world by ?size=2x, its own world file (BIG_MAP.md M2b)', () => {
    expect(parseLocalWorld('?seed=5&size=2x')).toEqual({
      worldSeed: 5,
      generatorVersion: GENERATORS.terrain,
      file: 'local-g8-s5-x2',
      radiusScale: 2,
    });
    expect(parseLocalWorld('?size=10x').radiusScale).toBe(10);
    // Today's size and an unknown one are the plain link's world.
    expect(parseLocalWorld('?seed=5&size=1x')).toEqual({
      worldSeed: 5,
      generatorVersion: GENERATORS.terrain,
    });
    expect(parseLocalWorld('?seed=5&size=3x').radiusScale).toBeUndefined();
    // The flat and sandbox worlds are 1× only.
    expect(parseLocalWorld('?world=flat&size=2x')).toEqual({
      worldSeed: 0,
      generatorVersion: GENERATORS.flat,
    });
    // With a spawn or a point: the sized world's own file.
    expect(parseLocalWorld('?seed=5&size=2x&spawn=b')).toMatchObject({
      file: 'local-g8-s5-faceb-x2',
      radiusScale: 2,
      spawnFaceB: true,
    });
    expect(parseLocalWorld('?seed=5&size=4x&at=1,2,3')).toMatchObject({
      file: 'local-g8-s5-x4',
      radiusScale: 4,
      spawnAt: [1, 2, 3],
    });
  });

  it("names the sizes of the data, today's first", () => {
    expect(WORLD_SIZES[0]).toEqual({ name: '1x', radiusScale: 1 });
    expect(sizeScale('8x')).toBe(8);
    expect(sizeScale('nope')).toBeNull();
    expect(sizeOptions(1)).toEqual({});
    expect(sizeOptions(undefined)).toEqual({});
    expect(sizeOptions(3)).toEqual({});
    expect(sizeOptions(2)).toEqual({ radiusScale: 2 });
  });

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
      spawnOnOpen: true,
    });
    expect(parseLocalWorld('?spawn=a').spawnFaceB).toBeUndefined();
    // By a portal's opening (BIFACIAL_WORLD.md §6), on either face, in a world file of its own.
    expect(parseLocalWorld('?seed=7&spawn=portal')).toEqual({
      worldSeed: 7,
      generatorVersion: GENERATORS.terrain,
      file: 'local-g8-s7-portal',
      spawnPortal: true,
      spawnOnOpen: true,
    });
    expect(parseLocalWorld('?seed=7&spawn=portal-b')).toMatchObject({
      file: 'local-g8-s7-portal-b',
      spawnFaceB: true,
      spawnPortal: true,
    });
  });

  it('reads ?spawn=moon, sun-b, … (MOON_WORLD.md §8) as a spawn on that grid, its own world file', () => {
    expect(parseLocalWorld('?seed=7&spawn=sun-b')).toEqual({
      worldSeed: 7,
      generatorVersion: GENERATORS.terrain,
      file: 'local-g8-s7-sun-b',
      spawnGrid: 'sun-b',
      spawnOnOpen: true,
    });
    expect(parseLocalWorld('?spawn=moon')).toMatchObject({ spawnGrid: 'moon' });
    expect(parseLocalWorld('?spawn=moon').spawnFaceB).toBeUndefined();
    expect(parseLocalWorld('?spawn=mars').spawnGrid).toBeUndefined();
    expect(spawnOptions('portal-b').spawnGrid).toBeUndefined();
  });

  it('a spawn link puts the player at its spawn on every open, a plain link does not', () => {
    // Owner, 2026-10-27: a saved world opened by a spawn link moves its player to the spawn.
    expect(parseLocalWorld('?spawn=portal').spawnOnOpen).toBe(true);
    expect(parseLocalWorld('?spawn=b').spawnOnOpen).toBe(true);
    expect(parseLocalWorld('?seed=3').spawnOnOpen).toBeUndefined();
    expect(parseLocalWorld('?spawn=').spawnOnOpen).toBeUndefined();
  });

  it("gives a spawn's options", () => {
    expect(spawnOptions('')).toEqual({});
    expect(spawnOptions('b')).toEqual({ spawnFaceB: true });
    expect(spawnOptions('portal')).toEqual({ spawnPortal: true });
    expect(spawnOptions('portal-b')).toEqual({ spawnFaceB: true, spawnPortal: true });
  });
});

describe('feetOf (BUGS.md #84)', () => {
  it('takes [x, y, z] or { x, y, z } of finite numbers, and nothing else', () => {
    expect(feetOf([1, 2, 3])).toEqual([1, 2, 3]);
    expect(feetOf({ x: 1, y: 2, z: 3 })).toEqual([1, 2, 3]);
    expect(feetOf([1, 2])).toBeNull();
    expect(feetOf([1, Number.NaN, 3])).toBeNull();
    expect(feetOf({ x: 1, y: 2 })).toBeNull();
    expect(feetOf('1,2,3')).toBeNull();
  });
});
