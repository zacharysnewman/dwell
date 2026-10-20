import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import {
  configParams,
  DEFAULT_SUPPORT,
  parseConfigs,
  partition,
  selectConfigs,
  supportOf,
} from './configs.ts';
import { parseScenarios, scenarioUrl } from './scenarios.ts';

const raster = { name: 'raster', renderer: 'raster' };

describe('bench --compare configurations', () => {
  it('parses the checked-in file: raster runs today, the march entries wait', () => {
    const file = parseConfigs(
      JSON.parse(
        readFileSync(new URL('../bench/configs.json', import.meta.url), 'utf8'),
      ) as unknown,
    );
    expect(file.baseline).toBe('raster');
    const { run, skipped } = partition(file.configs, DEFAULT_SUPPORT);
    expect(run.map((c) => c.name)).toEqual(['raster', 'raster-half']);
    expect(skipped.map((s) => s.name)).toEqual([
      'march',
      'march-nobeam',
      'march-notemporal',
      'march-half-fsr',
      'march-075-fsr',
      'march-half-bilateral',
      'march-half-taa',
      'march-075-taa',
    ]);
    expect(skipped[0].reason).toMatch(/no "march" renderer/);
    // A build with the marcher and its upscalers runs all but TAA-U.
    const marcher = supportOf({
      renderers: ['raster', 'march'],
      upscalers: ['none', 'bilinear', 'fsr1', 'bilateral'],
    });
    expect(partition(file.configs, marcher).skipped.map((s) => s.name)).toEqual([
      'march-half-taa',
      'march-075-taa',
    ]);
  });

  it('a new configuration works by data alone', () => {
    const file = parseConfigs({
      configs: [raster, { name: 'raster-quarter', renderer: 'raster', scale: 0.25 }],
    });
    const c = file.configs[1];
    expect(c.upscaler).toBe('bilinear');
    expect(configParams(c)).toEqual({ renderer: 'raster', scale: '0.25' });
    const [sc] = parseScenarios({ scenarios: [{ name: 'a', steps: [{ do: 'mark' }] }] });
    expect(scenarioUrl('http://h/dwell/', sc, configParams(c))).toBe(
      'http://h/dwell/?local=1&seed=7&renderer=raster&scale=0.25',
    );
  });

  it('a configuration adds its own page parameters', () => {
    const file = parseConfigs({
      configs: [
        raster,
        { name: 'no-caustics', renderer: 'raster', params: { waterdbg: 'caustics' } },
      ],
    });
    expect(configParams(file.configs[1])).toEqual({
      waterdbg: 'caustics',
      renderer: 'raster',
      scale: '1',
    });
    expect(() => parseConfigs({ configs: [{ ...raster, params: { a: 1 } }] })).toThrow(/params/);
  });

  it('a build that offers a renderer and an upscaler runs their configurations', () => {
    const file = parseConfigs({
      configs: [
        raster,
        { name: 'march-half-fsr', renderer: 'march', scale: 0.5, upscaler: 'fsr1' },
        { name: 'march-half-taa', renderer: 'march', scale: 0.5, upscaler: 'taa-u' },
      ],
    });
    const support = supportOf({ renderers: ['raster', 'march'], upscalers: ['fsr1'] });
    const { run, skipped } = partition(file.configs, support);
    expect(run.map((c) => c.name)).toEqual(['raster', 'march-half-fsr']);
    expect(skipped).toEqual([
      { name: 'march-half-taa', reason: 'this build has no "taa-u" upscaler (it offers fsr1)' },
    ]);
    expect(configParams(run[1])).toEqual({ renderer: 'march', scale: '0.5', upscaler: 'fsr1' });
  });

  it('without the hooks a build has only raster', () => {
    expect(supportOf(null)).toEqual(DEFAULT_SUPPORT);
    expect(supportOf({ renderers: null, upscalers: 'x' })).toEqual(DEFAULT_SUPPORT);
  });

  it('selects by name with the baseline first', () => {
    const file = parseConfigs({
      baseline: 'b',
      configs: [
        { name: 'a', renderer: 'raster' },
        { name: 'b', renderer: 'raster' },
        { name: 'c', renderer: 'raster' },
      ],
    });
    expect(selectConfigs(file, []).map((c) => c.name)).toEqual(['b', 'a', 'c']);
    expect(selectConfigs(file, ['c']).map((c) => c.name)).toEqual(['b', 'c']);
    expect(() => selectConfigs(file, ['z'])).toThrow(/no configuration z/);
  });

  it('rejects bad entries with a message', () => {
    const bad =
      (configs: unknown[], extra = {}): (() => unknown) =>
      () =>
        parseConfigs({ configs, ...extra });
    expect(bad([])).toThrow(/at least one/);
    expect(bad([{ ...raster, scal: 1 }])).toThrow(/configs\["raster"\]: unknown field "scal"/);
    expect(bad([{ name: 'r', renderer: 'raster', scale: 3 }])).toThrow(/outside 0.25–2/);
    expect(bad([{ name: 'r', renderer: 'raster', scale: 0.5, upscaler: 'dlss' }])).toThrow(
      /upscaler: expected one of/,
    );
    expect(bad([{ name: 'r', renderer: 'raster', upscaler: 'fsr1' }])).toThrow(/needs a scale/);
    expect(bad([{ name: 'r' }])).toThrow(/renderer: expected lowercase/);
    expect(bad([raster, raster])).toThrow(/used twice/);
    expect(bad([raster], { baseline: 'march' })).toThrow(/no configuration "march"/);
  });
});
