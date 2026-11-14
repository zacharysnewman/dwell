import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { parseScenarios, parseStep, spotScenario } from './scenarios.ts';
import {
  eyeOver,
  findSpot,
  groundRows,
  openAt,
  OPEN_HEIGHT_M,
  parseSpots,
  spotParams,
} from './spots.ts';
import { staleEntries } from './warmKey.ts';

const file = (spots: unknown): unknown => ({ spots });

describe('saved spots', () => {
  it('the checked-in spots parse, and every starter spot opens the starter world', () => {
    const spots = parseSpots(JSON.parse(readFileSync('bench/spots.json', 'utf8')) as unknown);
    expect(spots.length).toBeGreaterThanOrEqual(2);
    for (const s of spots.filter((x) => x.name.startsWith('starter:'))) {
      expect(s.world).toBe('test:starter');
    }
    // The tree performance reference (WORK_QUEUE item 32): the forest world's three spots.
    const forest = spots.filter((x) => x.name.startsWith('forest:')).map((x) => x.name);
    expect(forest).toEqual([
      'forest:inside',
      'forest:edge',
      'forest:over25',
      'forest:over40',
      'forest:over80',
      'forest:above',
    ]);
    for (const s of spots.filter((x) => x.name.startsWith('forest:'))) {
      expect(s.world).toBe('test:forest');
    }
  });

  it('a spot added by data alone is found and opens at its place', () => {
    const spots = parseSpots(
      file({
        'fixture:hill': { world: 'test:tall_peak', above: [10.5, -4.5, 6], yaw: 90, openY: 200 },
      }),
    );
    const s = findSpot(spots, 'fixture:hill');
    expect(s.place).toEqual({ above: [10.5, -4.5, 6], yaw: 90, pitch: 0, face: 'a' });
    expect(spotParams(s)).toEqual({
      worldgen: 'test:tall_peak',
      seed: '0',
      fresh: '1',
      at: '10.5,200,-4.5',
      time: '0.45',
    });
    expect(spotScenario(s).steps).toEqual([
      { do: 'spot', spot: 'fixture:hill', name: 'fixture-hill' },
    ]);
  });

  it('an unknown spot names the ones there are', () => {
    const spots = parseSpots(file({ 'a:b': { world: 'terrain', at: [0, 50, 0] } }));
    expect(() => findSpot(spots, 'a:c')).toThrow(/no spot "a:c" \(have a:b\)/);
  });

  it('bad entries fail with the entry named', () => {
    const bad: [unknown, RegExp][] = [
      [{ world: 'terrain' }, /spots\["x:y"\]: expected "at"/],
      [{ world: 'terrain', at: [0, 1, 0], above: [0, 0, 1] }, /"at" or "above", not both/],
      [{ world: 'moon', at: [0, 1, 0] }, /world: expected "terrain" or "test:<fixture>"/],
      [{ world: 'terrain', at: [0, 1, 0], colour: 1 }, /unknown field "colour"/],
      [{ world: 'terrain', at: [0, 1, 0], openY: 9 }, /openY: only an "above" spot/],
      [{ world: 'terrain', at: [0, 1, 0], pitch: 120 }, /pitch: 120 is outside/],
    ];
    for (const [entry, message] of bad) {
      expect(() => parseSpots(file({ 'x:y': entry }))).toThrow(message);
    }
    expect(() => parseSpots(file({ Bad: { world: 'terrain', at: [0, 1, 0] } }))).toThrow(
      /<world>:<place>/,
    );
  });

  it('an `at` on the wrong side of the midplane for its face fails (the y sign slip)', () => {
    expect(() => parseSpots(file({ 'x:y': { world: 'terrain', at: [5, -80, 5] } }))).toThrow(
      /on face B's side of the midplane .* "a" \(the default\)/,
    );
    expect(() =>
      parseSpots(file({ 'x:y': { world: 'terrain', at: [5, 80, 5], face: 'b' } })),
    ).toThrow(/on face A's side/);
    const [b] = parseSpots(file({ 'x:y': { world: 'terrain', at: [5, -80, 5], face: 'b' } }));
    // Face B's feet are 1.6 m below the eye's −y, toward its up (−y).
    expect(openAt(b.place)).toEqual([5, -78.4, 5]);
  });

  it('`above` opens high over the spot on its face, then scans its column face-locally', () => {
    const [a] = parseSpots(file({ 'x:y': { world: 'terrain', above: [1, 2, 3] } }));
    expect(openAt(a.place)).toEqual([1, OPEN_HEIGHT_M, 2]);
    // Face A reads rows from the top down; face B from its top (most negative) up to the midplane.
    expect(groundRows('a', 2, 0.5)).toEqual([3, 2, 1, 0]);
    expect(groundRows('b', 2, 0.5)).toEqual([-4, -3, -2, -1]);
    // The eye 3 m over the ground cell's top along each face's up: row 10 tops at 5.5 m on A, and
    // its mirror, row −11, at −5.5 m on B.
    expect(eyeOver('a', 10, 0.5, 3)).toBe(8.5);
    expect(eyeOver('b', -11, 0.5, 3)).toBe(-8.5);
  });
});

describe('the placed shot step', () => {
  it('parses a placement with defaults, and keeps a plain shot plain', () => {
    expect(parseStep('s', { do: 'shot', name: 'plain' })).toEqual({
      do: 'shot',
      name: 'plain',
      quietFrames: 30,
      maxS: 90,
    });
    expect(
      parseStep('s', { do: 'shot', name: 'hill', above: [1, 2, 10], yaw: 45, pitch: -10 }),
    ).toEqual({
      do: 'shot',
      name: 'hill',
      place: { above: [1, 2, 10], yaw: 45, pitch: -10, face: 'a' },
      quietFrames: 30,
      maxS: 90,
    });
  });

  it('rejects a face-B point given for face A, and both placements at once', () => {
    expect(() => parseStep('s', { do: 'shot', name: 'x', at: [0, -40, 0] })).toThrow(
      /face B's side of the midplane/,
    );
    expect(() =>
      parseStep('s', { do: 'shot', name: 'x', at: [0, 40, 0], above: [0, 0, 1] }),
    ).toThrow(/not both/);
  });

  it('a spot step names its shot after the spot, and neither can be repeated', () => {
    expect(parseStep('s', { do: 'spot', spot: 'starter:broadleaf' })).toEqual({
      do: 'spot',
      spot: 'starter:broadleaf',
      name: 'starter-broadleaf',
    });
    expect(() =>
      parseScenarios({
        scenarios: [
          {
            name: 'r',
            steps: [{ do: 'repeat', times: 2, steps: [{ do: 'spot', spot: 'a:b' }] }],
          },
        ],
      }),
    ).toThrow(/"spot" can't be repeated/);
  });
});

describe('the warm cache', () => {
  it('keeps the newest builds and never the current one', () => {
    const e = [
      { name: 'old', mtimeMs: 1 },
      { name: 'new', mtimeMs: 3 },
      { name: 'mid', mtimeMs: 2 },
    ];
    expect(staleEntries(e, 2, 'new')).toEqual(['old']);
    expect(staleEntries(e, 1, 'old')).toEqual(['mid']);
  });
});
