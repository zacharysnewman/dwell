import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import {
  computeRegistryHash,
  parseState,
  REGISTRY_HASH,
  STATE_COUNT,
  STATE_DEFS,
  stateId,
  stateProperty,
  stateString,
  withProperty,
} from './blocks';

// The vector both registries are checked against (shared/blocks/vectors.txt, generated; the C++
// side reads the same file in block_registry_test.cpp).
const vector = readFileSync(new URL('../../../shared/blocks/vectors.txt', import.meta.url), 'utf8')
  .split('\n')
  .filter((l) => l && !l.startsWith('#'));
const vectorHash = BigInt(`0x${vector[0]?.split(' ')[1] ?? ''}`);
const vectorStates = vector.slice(1).map((l) => l.slice(l.indexOf(' ') + 1));

describe('block registry (docs/BLOCK_REGISTRY.md)', () => {
  it('has the same states in the same order as the shared vector, and the same hash', () => {
    expect(STATE_DEFS.map((s) => s.state)).toEqual(vectorStates);
    expect(REGISTRY_HASH).toBe(vectorHash);
    expect(computeRegistryHash()).toBe(REGISTRY_HASH);
  });

  it('round-trips every state: id → canonical string → id', () => {
    for (let id = 0; id < STATE_COUNT; ++id) {
      const text = stateString(id) ?? '';
      expect(parseState(text)).toEqual({ id });
      expect(stateId(text)).toBe(id);
    }
  });

  it('writes every property, keys alphabetical; a block without properties is just its id', () => {
    expect(stateString(stateId('dwell:stone'))).toBe('dwell:stone');
    const parsed = parseState('dwell:ladder[flooded=true,facing=east]');
    expect(stateString('id' in parsed ? parsed.id : -1)).toBe(
      'dwell:ladder[facing=east,flooded=true]',
    );
  });

  it('parses any key order and fills missing properties with their defaults', () => {
    const north = stateId('dwell:ladder[facing=north,flooded=false]');
    expect(parseState('dwell:ladder')).toEqual({ id: north });
    expect(parseState('dwell:ladder[]')).toEqual({ id: north });
    expect(parseState('dwell:ladder[flooded=true,facing=south]')).toEqual(
      parseState('dwell:ladder[facing=south,flooded=true]'),
    );
    expect(parseState('dwell:ladder[facing=west]')).toEqual({
      id: stateId('dwell:ladder[facing=west,flooded=false]'),
    });
  });

  it('rejects invalid strings with a message', () => {
    for (const bad of [
      'dwell:nothing',
      'stone',
      'dwell:stone[facing=north]',
      'dwell:ladder[facing=up]',
      'dwell:ladder[facing=north,facing=east]',
      'dwell:ladder[facing]',
      'dwell:ladder[facing=north,]',
      'dwell:ladder[facing=north',
      '',
    ]) {
      expect(parseState(bad), bad).toHaveProperty('error');
    }
  });

  it('reads and sets properties (placement, rotation)', () => {
    const ladder = stateId('dwell:ladder[facing=north,flooded=false]');
    expect(stateProperty(ladder, 'facing')).toBe('north');
    expect(stateProperty(ladder, 'flooded')).toBe('false');
    expect(stateProperty(ladder, 'half')).toBeUndefined();
    const east = withProperty(ladder, 'facing', 'east');
    expect(stateString(east ?? -1)).toBe('dwell:ladder[facing=east,flooded=false]');
    expect(withProperty(ladder, 'facing', 'up')).toBeUndefined();
    expect(withProperty(stateId('dwell:stone'), 'facing', 'east')).toBeUndefined();
  });

  it('is the registry the determinism goldens were generated with (regenerate them when ids move)', () => {
    for (const name of ['chunk-hashes', 'lod-hashes']) {
      const text = readFileSync(
        new URL(`../../../server/tests/worldgen/golden/${name}.txt`, import.meta.url),
        'utf8',
      );
      const recorded = /^# registry ([0-9a-f]+)$/m.exec(text)?.[1];
      expect(recorded, name).toBe(REGISTRY_HASH.toString(16));
    }
  });
});
