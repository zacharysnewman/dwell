import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { configParams, parseConfigs, selectConfigs } from './configs.ts';
import { parseScenarios, scenarioUrl } from './scenarios.ts';

const raster = { name: 'raster' };

describe('bench --compare configurations', () => {
  it('parses the checked-in file', () => {
    // Derived from the file, so a new configuration needs no edit here: every entry parses (a bad
    // one throws with its name), in the file's order, and the baseline is one of them.
    const raw = JSON.parse(
      readFileSync(new URL('../bench/configs.json', import.meta.url), 'utf8'),
    ) as { baseline: string; configs: { name: string }[] };
    const file = parseConfigs(raw);
    expect(file.baseline).toBe('raster');
    expect(file.configs.map((c) => c.name)).toEqual(raw.configs.map((c) => c.name));
    expect(file.configs.length).toBeGreaterThan(1);
    expect(selectConfigs(file, [])[0].name).toBe(file.baseline);
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
