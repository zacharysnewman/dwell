import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import {
  clipStripCamera,
  clipStripSheet,
  clipStripTimes,
  parseScenarios,
  parseStep,
  overBudget,
  buildPage,
  scenarioUrl,
  scenarioView,
  swingClip,
  timelapseTimes,
} from './scenarios.ts';
import { HOTBAR_SLOTS } from '../src/ui/inventory.gen.ts';
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
      // A spot step is a placed shot (`<name>.png`, compared like a shot).
      .filter((s) => !s.steps.some((x) => x.do === 'shot' || x.do === 'spot'))
      .map((s) => s.name);
    expect(without).toEqual([]);
  });

  it('sets other settings mid-run with a view step (the cost profile ablations)', () => {
    const [s] = parseScenarios(
      file([
        {
          name: 'v',
          steps: [
            { do: 'view', clouds: 'off', speedEffects: false },
            { do: 'view', distanceM: 128, pixelError: 8, smoke: false, flowParticles: false },
            { do: 'view', flowRipples: false, flowFoam: false, waterfalls: false },
          ],
        },
      ]),
    );
    expect(s.steps[0]).toEqual({ do: 'view', clouds: 'off', speedEffects: false });
    expect(s.steps[1]).toEqual({
      do: 'view',
      distanceM: 128,
      pixelError: 8,
      smoke: false,
      flowParticles: false,
    });
    expect(s.steps[2]).toEqual({
      do: 'view',
      flowRipples: false,
      flowFoam: false,
      waterfalls: false,
    });
    const bad =
      (step: unknown): (() => unknown) =>
      () =>
        parseScenarios(file([{ name: 'a', steps: [step] }]));
    // Medium is a Clouds tier (shared/look/clouds.json `tiers.medium`, the desktops' default).
    expect(
      parseScenarios(file([{ name: 'a', steps: [{ do: 'view', clouds: 'medium' }] }])).at(0)
        ?.steps[0],
    ).toEqual({ do: 'view', clouds: 'medium' });
    expect(bad({ do: 'view', clouds: 'ultra' })).toThrow(/"off", "low", "medium" or "high"/);
    expect(bad({ do: 'view', pixelError: 40 })).toThrow(/pixelError/);
    expect(bad({ do: 'view', fog: false })).toThrow(/fog/);
  });

  it('picks an item by name, and a select past the hotbar fails', () => {
    // 0.52.23's six-slot hotbar: a `select` of the old slots 6–9 (the torch, the axe, …) wrapped
    // onto another slot and the scenario placed the wrong thing without a word.
    const [s] = parseScenarios(
      file([
        {
          name: 'a',
          steps: [
            { do: 'select', slot: HOTBAR_SLOTS - 1 },
            { do: 'pick', item: 'dwell:torch' },
          ],
        },
      ]),
    );
    expect(s.steps).toEqual([
      { do: 'select', slot: HOTBAR_SLOTS - 1 },
      { do: 'pick', item: 'dwell:torch' },
    ]);
    const bad =
      (step: unknown): (() => unknown) =>
      () =>
        parseScenarios(file([{ name: 'a', steps: [step] }]));
    expect(bad({ do: 'select', slot: HOTBAR_SLOTS })).toThrow(/slot: 6 is outside 0–5/);
    expect(bad({ do: 'select', slot: 8 })).toThrow(/slot/);
    expect(bad({ do: 'select', slot: 1.5 })).toThrow(/whole number/);
    expect(bad({ do: 'pick', item: 'torch' })).toThrow(/item name/);
    expect(bad({ do: 'pick' })).toThrow(/item/);
    expect(bad({ do: 'pick', item: 'dwell:torch', slot: 2 })).toThrow(/slot/);
  });

  it('the checked-in scenarios reach items by pick, not past the hotbar', () => {
    const json = JSON.parse(
      readFileSync(new URL('../bench/scenarios.json', import.meta.url), 'utf8'),
    ) as unknown;
    const picks = parseScenarios(json).flatMap((s) =>
      s.steps.flatMap((t) => (t.do === 'pick' ? [t.item] : [])),
    );
    expect(new Set(picks)).toEqual(
      new Set([
        'dwell:log',
        'dwell:leaves',
        'dwell:stone_axe',
        'dwell:torch',
        'dwell:dirt',
        'dwell:stone',
        'dwell:wooden_shovel',
        'dwell:stone_pickaxe',
      ]),
    );
  });

  it('fills defaults and makes a local-world address', () => {
    const [s] = parseScenarios(file([minimal]));
    expect(s.seed).toBe(7);
    expect(s.window).toEqual({ width: 1280, height: 720 });
    // A Retina page: the device's pixel ratio (shots at its pixels), in [1, 4].
    const retina = parseScenarios(
      file([{ ...minimal, window: { width: 1280, height: 800, pixelRatio: 2 } }]),
    );
    expect(retina[0]?.window).toEqual({ width: 1280, height: 800, pixelRatio: 2 });
    expect(viewUrl('./', scenarioView(s))).toBe('./?local=1&seed=7');
  });

  it('carries the player on a moving raft (the carry rig)', () => {
    const [s] = parseScenarios(
      file([
        {
          name: 'c',
          steps: [
            { do: 'carry', speed: 3, yaw: 90, s: 20 },
            { do: 'carry', speed: 2, yaw: 0, s: 5, lift: 0 },
          ],
        },
      ]),
    );
    expect(s.steps).toEqual([
      { do: 'carry', speed: 3, yaw: 90, s: 20 },
      { do: 'carry', speed: 2, yaw: 0, s: 5, lift: 0 },
    ]);
    expect(() =>
      parseScenarios(file([{ name: 'c', steps: [{ do: 'carry', speed: 3, yaw: 0 }] }])),
    ).toThrow(/s/);
  });

  it('finds the nearest herd (the finder, WORK_QUEUE item 46)', () => {
    const [s] = parseScenarios(
      file([{ name: 'h', steps: [{ do: 'herd' }, { do: 'herd', kind: 'wolf', back: 60 }] }]),
    );
    expect(s.steps).toEqual([{ do: 'herd' }, { do: 'herd', kind: 'wolf', back: 60 }]);
    expect(() =>
      parseScenarios(file([{ name: 'h', steps: [{ do: 'herd', kind: 'bear' }] }])),
    ).toThrow(/kind/);
  });

  it('teleports onto the moon (docs/DEV_TOOLS.md "The moon")', () => {
    const [s] = parseScenarios(
      file([
        {
          name: 'm',
          steps: [{ do: 'moonLand' }, { do: 'moonLand', face: 'b', maxS: 30, waitS: 1 }],
        },
      ]),
    );
    expect(s.steps).toEqual([
      { do: 'moonLand' },
      { do: 'moonLand', face: 'b', maxS: 30, waitS: 1 },
    ]);
    const bad = (step: Record<string, unknown>) => () =>
      parseScenarios(file([{ name: 'm', steps: [{ do: 'moonLand', ...step }] }]));
    expect(bad({ face: 'c' })).toThrow(/face/);
    expect(bad({ maxS: 0 })).toThrow(/maxS/);
    expect(bad({ to: 'moon' })).toThrow(/to/);
  });

  it('teleports onto the sun (SUN_WORLD.md, scenario sun-draw)', () => {
    const [s] = parseScenarios(
      file([
        {
          name: 's',
          steps: [
            { do: 'sunLand' },
            { do: 'sunLand', face: 'b', maxS: 30 },
            { do: 'sunLand', river: true },
          ],
        },
      ]),
    );
    expect(s.steps).toEqual([
      { do: 'sunLand' },
      { do: 'sunLand', face: 'b', maxS: 30 },
      { do: 'sunLand', river: true },
    ]);
    expect(() =>
      parseScenarios(file([{ name: 's', steps: [{ do: 'moonLand', river: true }] }])),
    ).toThrow(/river/);
    expect(() =>
      parseScenarios(file([{ name: 's', steps: [{ do: 'sunLand', face: 'c' }] }])),
    ).toThrow(/face/);
  });

  it('flies to the moon and home (docs/DEV_TOOLS.md "The moon")', () => {
    const [s] = parseScenarios(
      file([
        {
          name: 'm',
          steps: [
            { do: 'flyTo', to: 'moon' },
            { do: 'flyTo', to: 'moon', face: 'b', landM: 30, maxS: 90 },
            { do: 'flyTo', to: 'moon', marginM: 500 },
            { do: 'flyTo', to: 'home' },
          ],
        },
      ]),
    );
    expect(s.steps).toEqual([
      { do: 'flyTo', to: 'moon' },
      { do: 'flyTo', to: 'moon', face: 'b', landM: 30, maxS: 90 },
      { do: 'flyTo', to: 'moon', marginM: 500 },
      { do: 'flyTo', to: 'home' },
    ]);
    const bad = (step: Record<string, unknown>) => () =>
      parseScenarios(file([{ name: 'm', steps: [{ do: 'flyTo', ...step }] }]));
    expect(bad({ to: 'mars' })).toThrow(/to/);
    expect(bad({ to: 'home', face: 'a' })).toThrow(/face/);
    expect(bad({ to: 'moon', face: 'c' })).toThrow(/face/);
    expect(bad({ to: 'moon', landM: 1 })).toThrow(/landM/);
    expect(bad({ to: 'moon', marginM: -5 })).toThrow(/marginM/);
    expect(bad({ to: 'home', marginM: 500 })).toThrow(/marginM/);
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

  it('finds places by what is there (goto, docs/DEV_TOOLS.md "Finding places")', () => {
    const [s] = parseScenarios(
      file([
        {
          name: 'g',
          steps: [
            {
              do: 'goto',
              find: 'water',
              scope: 'world',
              water: 'sea',
              minDepth: 3,
              minArea: 2000,
              height: 2,
              lookAt: 'dwell:sand',
              turn: 180,
              pitch: -3,
              maxS: 120,
            },
            { do: 'goto', find: 'dwell:sand', yaw: 90, move: true, face: 'any' },
            { do: 'goto', find: 'water', minCurrent: 0.1, aim: 'downstream', height: -0.5 },
            { do: 'camera', position: [5, 2, 0], yaw: 0, pitch: -10, aroundFound: true },
          ],
        },
      ]),
    );
    expect(s.steps[0]).toEqual({
      do: 'goto',
      find: 'water',
      scope: 'world',
      water: 'sea',
      minDepth: 3,
      minArea: 2000,
      height: 2,
      lookAt: 'dwell:sand',
      turn: 180,
      pitch: -3,
      maxS: 120,
    });
    expect(s.steps[1]).toEqual({
      do: 'goto',
      find: 'dwell:sand',
      yaw: 90,
      move: true,
      face: 'any',
    });
    expect(s.steps[2]).toMatchObject({ aim: 'downstream', minCurrent: 0.1, height: -0.5 });
    expect(s.steps[3]).toMatchObject({ aroundFound: true });

    const bad =
      (step: unknown): (() => unknown) =>
      () =>
        parseScenarios(file([{ name: 'a', steps: [step] }]));
    expect(bad({ do: 'goto' })).toThrow(/find: expected a string/);
    expect(bad({ do: 'goto', find: 'water; drop' })).toThrow(/expected a block name/);
    expect(bad({ do: 'goto', find: 'water', aim: 'sideways' })).toThrow(/downstream, upstream/);
    expect(bad({ do: 'goto', find: 'water', aim: 'downstream', yaw: 3 })).toThrow(/aim one way/);
    expect(bad({ do: 'goto', find: 'water', water: 'sea' })).toThrow(/only with scope "world"/);
    expect(bad({ do: 'goto', find: 'water', scope: 'world', water: 'pond' })).toThrow(
      /water, sea, river, lake/,
    );
    expect(bad({ do: 'goto', find: 'water', face: 'c' })).toThrow(/"a", "b" or "any"/);
    expect(bad({ do: 'goto', find: 'water', minArea: 2.5 })).toThrow(/whole number/);
    expect(bad({ do: 'goto', find: 'water', minDepth: -1 })).toThrow(/outside 0–256/);
    expect(bad({ do: 'goto', find: 'water', speed: 1 })).toThrow(/unknown field "speed"/);
    expect(
      bad({
        do: 'camera',
        position: [0, 0, 0],
        yaw: 0,
        pitch: 0,
        aroundFound: true,
        aroundPlayer: true,
      }),
    ).toThrow(/not with aroundPlayer/);
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
    expect(
      parseScenarios(
        file([{ name: 'a', steps: [{ do: 'burst', name: 'b', frames: 8, clock: 0.5 }] }]),
      )[0].steps[0],
    ).toEqual({ do: 'burst', name: 'b', frames: 8, clock: 0.5 });
    expect(bad({ do: 'burst', name: 'b', frames: 1 })).toThrow(/outside 2–200/);
    expect(bad({ do: 'burst', name: 'b', frames: 8, maxFlicker: 2 })).toThrow(/outside 0–1/);
    expect(bad({ do: 'burst', name: 'b', frames: 8, clock: 0.5, wind: 0 })).toThrow(/wind/);
    expect(
      parseScenarios(
        file([
          {
            name: 'a',
            steps: [
              {
                do: 'burst',
                name: 'b',
                frames: 8,
                drift: { position: [0.03, 0, 0], yaw: 0.05 },
                maxHueFlips: 0.01,
              },
            ],
          },
        ]),
      )[0].steps[0],
    ).toEqual({
      do: 'burst',
      name: 'b',
      frames: 8,
      drift: { position: [0.03, 0, 0], yaw: 0.05, pitch: 0 },
      maxHueFlips: 0.01,
    });
    expect(bad({ do: 'burst', name: 'b', frames: 8, drift: { position: [2, 0, 0] } })).toThrow(
      /over 1 m/,
    );
    expect(bad({ do: 'burst', name: 'b', frames: 8, drift: { roll: 1 } })).toThrow(/roll/);
    expect(
      parseScenarios(file([{ name: 'a', steps: [{ do: 'brush', command: 'preset:cube' }] }]))[0]
        .steps[0],
    ).toEqual({ do: 'brush', command: 'preset:cube' });
    expect(bad({ do: 'brush', command: 'apply; drop' })).toThrow(/expected a brush command/);
  });

  it('a timelapse walks the times of day, past midnight when it wraps', () => {
    expect(timelapseTimes({ from: 0, to: 1, frames: 4 })).toEqual([0, 0.25, 0.5, 0.75]);
    expect(timelapseTimes({ from: 0.75, to: 0.25, frames: 4 })).toEqual([0.75, 0.875, 0, 0.125]);
  });

  it('repeats steps, and takes a near/far split', () => {
    const body = [
      { do: 'edit', action: 'place' },
      { do: 'wait', s: 0.12 },
    ];
    const [s] = parseScenarios(
      file([
        {
          name: 'c',
          split: 'near',
          steps: [{ do: 'repeat', times: 3, steps: body }],
        },
      ]),
    );
    expect(s.steps[0]).toEqual({ do: 'repeat', times: 3, steps: body });
    expect(s.split).toBe('near');
    expect(scenarioUrl('./', s)).toBe('./?local=1&seed=7');
    // A face-B scenario spawns its player on face B (BIFACIAL_WORLD.md §11).
    const [b] = parseScenarios(file([{ ...minimal, face: 'b' }]));
    expect(b.face).toBe('b');
    expect(scenarioUrl('./', b)).toBe('./?local=1&seed=7&spawn=b');
    expect(() => parseScenarios(file([{ ...minimal, face: 'c' }]))).toThrow(/"a" or "b"/);
    // A scenario opens another world type by name (`?world=`); the terrain is the default.
    const [c] = parseScenarios(file([{ ...minimal, world: 'flat' }]));
    expect(scenarioUrl('./', c)).toBe('./?local=1&seed=7&world=flat');
    expect(() => parseScenarios(file([{ ...minimal, world: 'caves' }]))).toThrow(/one of terrain/);
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

  it('waits until nothing behind or ahead of the camera is loading (the join timeline)', () => {
    const [s] = parseScenarios(
      file([
        {
          name: 'a',
          steps: [
            { do: 'until', what: 'behind', maxS: 60 },
            { do: 'until', what: 'ahead', maxS: 5 },
          ],
        },
      ]),
    );
    expect(s.steps).toEqual([
      { do: 'until', what: 'behind', maxS: 60 },
      { do: 'until', what: 'ahead', maxS: 5 },
    ]);
    expect(() =>
      parseScenarios(file([{ name: 'a', steps: [{ do: 'until', what: 'left', maxS: 1 }] }])),
    ).toThrow(/what: expected one of behind, nearBehind, ahead/);
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

describe('clip-strip', () => {
  const bad = (step: unknown) => () => parseScenarios(file([{ name: 'a', steps: [step] }]));
  it('parses with defaults and rejects bad views and frame counts', () => {
    const s = parseScenarios(file([{ name: 'a', steps: [{ do: 'clip-strip', name: 'swing' }] }]))[0]
      ?.steps[0];
    expect(s).toEqual({
      do: 'clip-strip',
      name: 'swing',
      vertical: false,
      frames: 8,
      views: ['fp', 'tp-behind', 'tp-side'],
      at: [0, 0, 0],
    });
    expect(bad({ do: 'clip-strip', name: 's', views: ['top'] })).toThrow(/views/);
    expect(bad({ do: 'clip-strip', name: 's', frames: 1 })).toThrow(/outside 2–24/);
    expect(bad({ do: 'clip-strip', name: 's', frames: 2.5 })).toThrow(/whole number/);
    expect(bad({ do: 'repeat', times: 2, steps: [{ do: 'clip-strip', name: 's' }] })).toThrow(
      /can't be repeated/,
    );
  });

  it('spans the swing clip of the characters data, ends included', () => {
    const characters = JSON.parse(
      readFileSync(new URL('../../shared/characters/characters.json', import.meta.url), 'utf8'),
    ) as unknown;
    const level = swingClip(characters, false);
    expect(level.clip).toBe('fp_chop_level');
    expect(swingClip(characters, true).clip).toBe('fp_chop_vertical');
    const t = clipStripTimes(level.duration, 5);
    expect(t).toHaveLength(5);
    expect(t[0]).toBe(0);
    expect(t[4]).toBeCloseTo(level.duration);
    expect(() => swingClip({ characters: { player: {} }, clips: {} }, false)).toThrow(/chop_level/);
  });

  it('frames the stand-in from behind-right and from its right, looking at it', () => {
    const feet: [number, number, number] = [10, 50, -4];
    const behind = clipStripCamera('tp-behind', feet, 1);
    // Behind (−Z) and to its right (−X at a heading of 0), above, looking ahead, right, down.
    expect(behind.position[0]).toBeLessThan(feet[0]);
    expect(behind.position[2]).toBeLessThan(feet[2]);
    expect(behind.position[1]).toBeGreaterThan(feet[1] + 1.2);
    expect(behind.yaw).toBeGreaterThan(0);
    expect(behind.yaw).toBeLessThan(90);
    expect(behind.pitch).toBeLessThan(0);
    const side = clipStripCamera('tp-side', feet, 1);
    expect(side.position[0]).toBeLessThan(feet[0] - 2);
    expect(side.yaw).toBeGreaterThan(80);
    expect(side.yaw).toBeLessThan(100);
    // Face B: mirrored through the midplane (below the feet), the heading mirrored.
    const b = clipStripCamera('tp-behind', feet, -1);
    expect(b.position[1]).toBeLessThan(feet[1]);
    expect(b.yaw).toBeCloseTo(-behind.yaw);
  });

  it('lays the frames out a row per view', () => {
    const html = clipStripSheet('s <1>', [
      {
        view: 'fp',
        frames: [
          { t: 0, src: 'data:a' },
          { t: 0.31, src: 'data:b' },
        ],
      },
      { view: 'tp-side', frames: [{ t: 0, src: 'data:c' }] },
    ]);
    expect(html.match(/<tr>/g)).toHaveLength(2);
    expect(html).toContain('0.310 s');
    expect(html).toContain('s &lt;1>');
  });
});

describe('record budgets', () => {
  const one = (budget: unknown) =>
    parseScenarios({
      scenarios: [{ name: 's', steps: [{ do: 'record', name: 'move', budget }] }],
    })[0].steps[0];

  it('parses a budget and reports what is over it (BUGS.md #72)', () => {
    expect(one({ 'frame.cpu.p95': 14, 'jobs.apply.max': 6 })).toEqual({
      do: 'record',
      name: 'move',
      budget: { 'frame.cpu.p95': 14, 'jobs.apply.max': 6 },
    });
    const frames = { 'frame.cpu': { p95: 15.7, max: 21 }, 'jobs.apply': { max: 4 } };
    expect(overBudget(frames, { 'frame.cpu.p95': 14, 'jobs.apply.max': 6 })).toEqual([
      'frame.cpu.p95 15.70 over 14',
    ]);
    expect(overBudget(frames, { 'mesh_dirty.p95': 9 })).toEqual(['mesh_dirty.p95: not recorded']);
  });

  it('bounds the GPU timings, which are samples, not per-frame series (forest-perf)', () => {
    const frames = { 'frame.cpu': { p95: 9 } };
    const samples = { 'gpu.frame': { p95: 25.0 } };
    expect(overBudget(frames, { 'frame.cpu.p95': 16.7, 'gpu.frame.p95': 16.7 }, samples)).toEqual([
      'gpu.frame.p95 25.00 over 16.7',
    ]);
  });

  it('rejects a budget without a known statistic', () => {
    expect(() => one({ 'frame.cpu': 14 })).toThrow(/stat of mean, p50, p95, p99, max/);
    expect(() => one([1])).toThrow(/expected/);
  });

  it('the phone-moving scenario bounds its moving records', () => {
    const all = parseScenarios(JSON.parse(readFileSync('bench/scenarios.json', 'utf8')));
    const s = all.find((x) => x.name === 'phone-moving');
    const budgets = s?.steps.filter((x) => x.do === 'record' && x.budget !== undefined) ?? [];
    expect(budgets.length).toBe(3);
  });

  it('a report-only budget parses, and needs a budget (WORK_QUEUE item 32)', () => {
    const step = (o: Record<string, unknown>) =>
      parseScenarios({ scenarios: [{ name: 's', steps: [{ do: 'record', name: 'r', ...o }] }] })[0]
        .steps[0];
    expect(step({ budget: { 'gpu.frame.p95': 16.7 }, reportOnly: true })).toEqual({
      do: 'record',
      name: 'r',
      budget: { 'gpu.frame.p95': 16.7 },
      reportOnly: true,
    });
    expect(() => step({ reportOnly: true })).toThrow(/reports only against a budget/);
    expect(() => step({ budget: { 'gpu.frame.p95': 1 }, reportOnly: 1 })).toThrow(/expected true/);
  });

  it('forest-perf records the six forest spots at High against a report-only 16.7 ms frame', () => {
    const all = parseScenarios(JSON.parse(readFileSync('bench/scenarios.json', 'utf8')));
    const s = all.find((x) => x.name === 'forest-perf');
    expect(s?.query).toMatchObject({ worldgen: 'test:forest', preset: 'high' });
    const spots = s?.steps.filter((x) => x.do === 'spot').map((x) => x.spot);
    expect(spots).toEqual([
      'forest:inside',
      'forest:edge',
      'forest:over25',
      'forest:over40',
      'forest:over80',
      'forest:above',
    ]);
    const records = s?.steps.filter((x) => x.do === 'record') ?? [];
    expect(records.map((r) => r.name)).toEqual([
      'inside',
      'edge',
      'over25',
      'over40',
      'over80',
      'above',
    ]);
    for (const r of records) {
      expect(r).toMatchObject({
        reportOnly: true,
        budget: { 'frame.cpu.p95': 16.7, 'gpu.frame.p95': 16.7 },
      });
    }
  });
});

describe('buildPage', () => {
  it('opens a root at /dwell/ and a build path as it is', () => {
    expect(buildPage('http://localhost:4173')).toBe('http://localhost:4173/dwell/');
    expect(buildPage('http://localhost:4174/')).toBe('http://localhost:4174/dwell/');
    expect(buildPage('https://dropkickarcade.com/dwell/')).toBe(
      'https://dropkickarcade.com/dwell/',
    );
    // A released build behind the launcher (it 404'd as …/v/0.51.24/dwell/).
    expect(buildPage('https://dropkickarcade.com/dwell/v/0.51.24/')).toBe(
      'https://dropkickarcade.com/dwell/v/0.51.24/',
    );
    expect(buildPage('https://dropkickarcade.com/dwell/v/0.51.24')).toBe(
      'https://dropkickarcade.com/dwell/v/0.51.24/',
    );
  });

  it('reads a theater step: a preset or a path, waiting and hiding the HUD by default', () => {
    expect(parseStep('s', { do: 'theater', preset: 'orbit', params: { radiusM: 30 } })).toEqual({
      do: 'theater',
      preset: 'orbit',
      params: { radiusM: 30 },
      wait: true,
      hud: false,
    });
    const path = { formatVersion: 1, keyframes: [] };
    expect(parseStep('s', { do: 'theater', path, wait: false, subject: 'player' })).toEqual({
      do: 'theater',
      path,
      subject: 'player',
      wait: false,
      hud: false,
    });
    expect(() => parseStep('s', { do: 'theater' })).toThrow(/a path or a preset/);
    expect(() => parseStep('s', { do: 'theater', path, preset: 'orbit' })).toThrow(
      /a path or a preset/,
    );
    expect(() => parseStep('s', { do: 'theater', preset: 'orbit', params: { r: 'x' } })).toThrow(
      /params\.r/,
    );
  });
});
