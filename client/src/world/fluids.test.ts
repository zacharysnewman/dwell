import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { blockFluid, readFluids } from '../../../shared/blocks/fluids.mjs';

// The registry's fluid looks (docs/BLOCK_REGISTRY.md, "Fluid looks"): the checks shared/blocks/gen.mjs runs.
const json = (path: string): { namespace: string; fluids?: Record<string, unknown> } =>
  JSON.parse(readFileSync(new URL(path, import.meta.url), 'utf8')) as {
    namespace: string;
    fluids?: Record<string, unknown>;
  };
const dwell = json('../../../shared/blocks/dwell.json');
const fixture = json('../../../shared/blocks/fixtures/test.json');
const water = (dwell.fluids ?? {})['dwell:water'] as Record<string, unknown>;
const withWater = (patch: Record<string, unknown>) => [
  { namespace: 'dwell', fluids: { 'dwell:water': { ...water, ...patch } } },
];

describe('fluid looks', () => {
  it('reads the build fluids, and a second fluid added by data alone gets the next id', () => {
    expect(readFluids([dwell]).map((f) => f.name)).toEqual(['dwell:water']);
    const both = readFluids([dwell, fixture]);
    expect(both.map((f) => [f.name, f.surfaceDrop, f.opacity])).toEqual([
      ['dwell:water', 0.125, 0.72],
      ['test:oil', 0.25, 0.8],
    ]);
    expect(blockFluid('test:oil', { look: 'fluid', fluid: 'test:oil' }, both)).toBe(1);
    expect(blockFluid('dwell:stone', { look: 'cube' }, both)).toBeNull();
  });

  it('fails a bad entry with a message naming the fluid and the field', () => {
    expect(() => readFluids(withWater({ surfaceDrop: 1 }))).toThrow(
      'fluid dwell:water: surfaceDrop 1 is outside [0, 1)',
    );
    expect(() => readFluids(withWater({ opacity: 0 }))).toThrow('opacity 0 is outside (0, 1]');
    expect(() => readFluids(withWater({ ior: undefined }))).toThrow('missing ior');
    expect(() => readFluids(withWater({ glow: 1 }))).toThrow('unknown field glow');
    expect(() => readFluids(withWater({ absorption: [1, 2] }))).toThrow(
      'absorption must be three numbers',
    );
    expect(() => readFluids(withWater({ scatter: 'blue' }))).toThrow('scatter must be #rrggbb');
    expect(() => readFluids(withWater({ foam: { depth: -1, color: '#ffffff' } }))).toThrow(
      'foam.depth -1',
    );
    expect(() => readFluids([{ namespace: 'test', fluids: { 'dwell:lava': water } }])).toThrow(
      'must be named test:',
    );
  });

  it('checks blocks that reference a fluid', () => {
    const fluids = readFluids([dwell]);
    expect(() => blockFluid('dwell:x', { look: 'fluid' }, fluids)).toThrow('needs a `fluid` name');
    expect(() => blockFluid('dwell:x', { look: 'fluid', fluid: 'dwell:lava' }, fluids)).toThrow(
      'unknown fluid dwell:lava',
    );
    expect(() => blockFluid('dwell:x', { look: 'cube', fluid: 'dwell:water' }, fluids)).toThrow(
      '`fluid` needs "look": "fluid"',
    );
    expect(() =>
      blockFluid('dwell:x', { look: 'fluid', fluid: 'dwell:water', opacity: 0.5 }, fluids),
    ).toThrow('opacity comes from the fluid');
  });
});
