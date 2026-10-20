import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { parseScenarios, scenarioUrl, scenarioView, timelapseTimes } from './scenarios.ts';
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

  it('every checked-in scenario that records also takes a shot (for --compare)', () => {
    const json = JSON.parse(
      readFileSync(new URL('../bench/scenarios.json', import.meta.url), 'utf8'),
    ) as unknown;
    const without = parseScenarios(json)
      .filter((s) => s.steps.some((x) => x.do === 'record'))
      .filter((s) => !s.steps.some((x) => x.do === 'shot'))
      .map((s) => s.name);
    expect(without).toEqual([]);
  });

  it('fills defaults and makes a local-world address', () => {
    const [s] = parseScenarios(file([minimal]));
    expect(s.seed).toBe(7);
    expect(s.window).toEqual({ width: 1280, height: 720 });
    expect(viewUrl('./', scenarioView(s))).toBe('./?local=1&seed=7');
  });

  it('zooms the spyglass to a step, or out (docs/SPYGLASS.md)', () => {
    const [s] = parseScenarios(
      file([
        {
          name: 'z',
          steps: [
            { do: 'spyglass', on: true, step: 1 },
            { do: 'spyglass', on: false },
          ],
        },
      ]),
    );
    expect(s.steps).toEqual([
      { do: 'spyglass', on: true, step: 1 },
      { do: 'spyglass', on: false },
    ]);
    expect(() =>
      parseScenarios(file([{ name: 'z', steps: [{ do: 'spyglass', on: 'yes' }] }])),
    ).toThrow(/on/);
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
            { do: 'camera', position: [4, -0.5, 0], yaw: 90, pitch: 0, aroundPlayer: true },
            { do: 'traceStart' },
            { do: 'traceStop', name: 'x' },
            { do: 'record', name: 'end' },
            { do: 'time', t: 0.25, moon: 0.5 },
            { do: 'timelapse', name: 'day', frames: 24 },
          ],
        },
      ]),
    );
    expect(s.steps.map((x) => x.do)).toEqual([
      'fly',
      'hold',
      'camera',
      'camera',
      'traceStart',
      'traceStop',
      'record',
      'time',
      'timelapse',
    ]);
    expect(s.steps[7]).toEqual({ do: 'time', t: 0.25, moon: 0.5 });
    expect(s.steps[8]).toEqual({
      do: 'timelapse',
      name: 'day',
      frames: 24,
      from: 0,
      to: 1,
      waitS: 0.3,
    });
  });

  it('times of day and timelapse frames are checked', () => {
    const bad =
      (step: unknown): (() => unknown) =>
      () =>
        parseScenarios(file([{ name: 'a', steps: [step] }]));
    expect(bad({ do: 'time', t: 1.5 })).toThrow(/outside 0–1/);
    expect(bad({ do: 'time' })).toThrow(/expected a number/);
    expect(
      parseScenarios(file([{ name: 'a', steps: [{ do: 'waterDebug', names: 'ssr,clouds' }] }]))[0]
        .steps[0],
    ).toEqual({ do: 'waterDebug', names: 'ssr,clouds' });
    expect(bad({ do: 'waterDebug', names: 'ssr; drop' })).toThrow(/switch names/);
    expect(bad({ do: 'timelapse', name: 'x', frames: 1 })).toThrow(/outside 2–1000/);
  });

  it('a timelapse walks the times of day, past midnight when it wraps', () => {
    expect(timelapseTimes({ from: 0, to: 1, frames: 4 })).toEqual([0, 0.25, 0.5, 0.75]);
    expect(timelapseTimes({ from: 0.75, to: 0.25, frames: 4 })).toEqual([0.75, 0.875, 0, 0.125]);
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

  it('sets the water quality and its cost switches mid-run', () => {
    const [s] = parseScenarios(
      file([
        {
          name: 'a',
          steps: [{ do: 'water', quality: 'medium', debug: 'caustics' }, { do: 'water' }],
        },
      ]),
    );
    expect(s.steps).toEqual([
      { do: 'water', quality: 'medium', debug: 'caustics' },
      { do: 'water' },
    ]);
    expect(() =>
      parseScenarios(file([{ name: 'a', steps: [{ do: 'water', quality: 'ultra' }] }])),
    ).toThrow(/quality/);
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
    expect(() =>
      parseScenarios(
        file([
          {
            name: 'a',
            steps: [{ do: 'camera', position: [0, 0, 0], yaw: 0, pitch: 0, aroundPlayer: 1 }],
          },
        ]),
      ),
    ).toThrow(/aroundPlayer: expected true or false/);
  });
});
