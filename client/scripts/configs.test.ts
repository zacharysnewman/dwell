import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { configParams, parseConfigs, selectConfigs } from './configs.ts';
import { parseScenarios, scenarioUrl } from './scenarios.ts';

const raster = { name: 'raster' };

describe('bench --compare configurations', () => {
  it('parses the checked-in file', () => {
    const file = parseConfigs(
      JSON.parse(
        readFileSync(new URL('../bench/configs.json', import.meta.url), 'utf8'),
      ) as unknown,
    );
    expect(file.baseline).toBe('raster');
    expect(file.configs.map((c) => c.name)).toEqual([
      'raster',
      'outline-silhouette',
      'outline-creases',
      'outline-blocks',
      'pixelart-180',
      'pixelart-270',
      'pixelart-270-warm',
      'pixelart-270-snap',
      'pixelart-360',
      'shadows-low',
      'shadows-off',
      'torch-shadows-off',
      'torch-shadows-high',
      'lod-shadows-off',
      'raster-half',
    ]);
  });

  it('a new configuration works by data alone', () => {
    const file = parseConfigs({ configs: [raster, { name: 'raster-quarter', scale: 0.25 }] });
    const c = file.configs[1];
    expect(configParams(c)).toEqual({ scale: '0.25' });
    const [sc] = parseScenarios({ scenarios: [{ name: 'a', steps: [{ do: 'mark' }] }] });
    expect(scenarioUrl('http://h/dwell/', sc, configParams(c))).toBe(
      'http://h/dwell/?local=1&seed=7&scale=0.25',
    );
  });

  it('a configuration adds its own page parameters', () => {
    const file = parseConfigs({
      configs: [raster, { name: 'no-caustics', params: { waterdbg: 'caustics' } }],
    });
    expect(configParams(file.configs[1])).toEqual({ waterdbg: 'caustics', scale: '1' });
    expect(() => parseConfigs({ configs: [{ ...raster, params: { a: 1 } }] })).toThrow(/params/);
  });

  it('selects by name with the baseline first', () => {
    const file = parseConfigs({
      baseline: 'b',
      configs: [{ name: 'a' }, { name: 'b' }, { name: 'c' }],
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
    // The renderer switch is gone (ADR 0044): a configuration naming one is an unknown field.
    expect(bad([{ ...raster, renderer: 'march' }])).toThrow(/unknown field "renderer"/);
    expect(bad([{ name: 'r', scale: 3 }])).toThrow(/outside 0.25–2/);
    expect(bad([{ name: 'R' }])).toThrow(/name: expected lowercase/);
    expect(bad([raster, raster])).toThrow(/used twice/);
    expect(bad([raster], { baseline: 'other' })).toThrow(/no configuration "other"/);
  });
});
