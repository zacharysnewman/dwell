import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { parseScenarios, scenarioUrl, scenarioView } from './scenarios.ts';
import { viewUrl } from './views.ts';

const file = (scenarios: unknown[]): unknown => ({ scenarios });
const minimal = { name: 'a', steps: [{ do: 'wait', s: 1 }] };

describe('scenarios', () => {
  it('parses the checked-in file', () => {
    const json = JSON.parse(
      readFileSync(new URL('../bench/scenarios.json', import.meta.url), 'utf8'),
    ) as unknown;
    expect(parseScenarios(json).length).toBeGreaterThan(0);
  });

  it('fills defaults and makes a local-world address', () => {
    const [s] = parseScenarios(file([minimal]));
    expect(s.seed).toBe(7);
    expect(s.window).toEqual({ width: 1280, height: 720 });
    expect(viewUrl('./', scenarioView(s))).toBe('./?local=1&seed=7');
  });

  it('a new step entry works by data alone', () => {
    const [s] = parseScenarios(
      file([
        {
          name: 'b',
          steps: [
            { do: 'fly', on: true },
            { do: 'hold', code: 'KeyW', s: 2 },
            { do: 'camera', position: [0, 100, 0], yaw: 0, pitch: -30 },
            { do: 'traceStart' },
            { do: 'traceStop', name: 'x' },
            { do: 'record', name: 'end' },
          ],
        },
      ]),
    );
    expect(s.steps.map((x) => x.do)).toEqual([
      'fly',
      'hold',
      'camera',
      'traceStart',
      'traceStop',
      'record',
    ]);
  });

  it('repeats steps, and takes a renderer and a near/far split', () => {
    const body = [
      { do: 'edit', action: 'place' },
      { do: 'wait', s: 0.12 },
    ];
    const [s] = parseScenarios(
      file([
        {
          name: 'c',
          renderer: 'march',
          split: 'near',
          steps: [{ do: 'repeat', times: 3, steps: body }],
        },
      ]),
    );
    expect(s.steps[0]).toEqual({ do: 'repeat', times: 3, steps: body });
    expect(s.split).toBe('near');
    expect(scenarioUrl('./', s)).toBe('./?local=1&seed=7&renderer=march');
    const repeat = (times: number, steps: unknown[]): unknown =>
      file([{ name: 'a', steps: [{ do: 'repeat', times, steps }] }]);
    expect(() => parseScenarios(repeat(2, [{ do: 'record', name: 'x' }]))).toThrow(
      /"record" can't be repeated/,
    );
    expect(() => parseScenarios(repeat(1.5, body))).toThrow(/whole number/);
    expect(() => parseScenarios(repeat(2, []))).toThrow(/non-empty/);
    expect(() =>
      parseScenarios(file([{ name: 'a', split: 'mid', steps: [{ do: 'mark' }] }])),
    ).toThrow(/"near" or "far"/);
  });

  it('rejects bad entries with a message', () => {
    expect(() => parseScenarios(file([{ name: 'a', steps: [{ do: 'jump' }] }]))).toThrow(
      /"do" must be one of/,
    );
    expect(() => parseScenarios(file([{ name: 'a', steps: [{ do: 'wait', secs: 1 }] }]))).toThrow(
      /unknown field "secs"/,
    );
    expect(() => parseScenarios(file([{ name: 'A b', steps: [{ do: 'mark' }] }]))).toThrow(
      /lowercase/,
    );
    expect(() => parseScenarios(file([minimal, minimal]))).toThrow(/used twice/);
    expect(() =>
      parseScenarios(file([{ name: 'a', steps: [{ do: 'traceStop', name: 't' }] }])),
    ).toThrow(/not tracing/);
  });
});
