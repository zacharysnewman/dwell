import { describe, expect, it } from 'vitest';
import {
  findPlace,
  gotoPlace,
  type Camera,
  type FindHost,
  type FoundPlace,
  type Vec3,
} from './find';

function place(over: Partial<FoundPlace> = {}): FoundPlace {
  return {
    cell: [10, 80, 20],
    at: [10.5, 40.5, 20.5],
    surfaceY: 40.5,
    face: 1,
    block: 'dwell:water[fill=8]',
    depth: 3,
    area: 500,
    distance: 12,
    current: [0, 0],
    flow: [0, 0],
    ...over,
  };
}

interface Call {
  query: string;
  from: Vec3;
  radius: number;
  minDepth: number;
  face: number;
  keep: boolean;
}

/** A fake page: `answers` in turn for each find (the last repeats), a clock moved by `wait`. */
function host(answers: (FoundPlace | null)[], over: Partial<FindHost> = {}) {
  const finds: Call[] = [];
  const teleports: Vec3[] = [];
  const views: unknown[][] = [];
  let now = 0;
  const realNow = Date.now;
  Date.now = () => now;
  const h: FindHost = {
    feet: () => [0, 50, 0],
    find: (query, from, radius, _s, minDepth, _a, _c, face, keep) => {
      finds.push({ query, from, radius, minDepth, face, keep });
      return answers.length > 1 ? (answers.shift() ?? null) : (answers[0] ?? null);
    },
    viewFound: (height, aim, yaw, toward, pitch, turn): Camera => {
      views.push([height, aim, yaw, toward, pitch, turn]);
      return { position: [0, height, 0], yaw, pitch };
    },
    fly: () => undefined,
    teleport: (feet) => {
      teleports.push(feet);
      return true;
    },
    findOnMap: () => Promise.resolve(null),
    wait: (ms) => {
      now += ms;
      return Promise.resolve();
    },
    ...over,
  };
  return { h, finds, teleports, views, restore: () => (Date.now = realNow) };
}

describe('find', () => {
  it('searches from the feet on the start face by default, with the filters', () => {
    const t = host([place()]);
    expect(findPlace(t.h, 'water', { minDepth: 2 })).toEqual(place());
    expect(t.finds[0]).toEqual({
      query: 'water',
      from: [0, 50, 0],
      radius: 256,
      minDepth: 2,
      face: 0,
      keep: true,
    });
    findPlace(t.h, 'water', { face: 'b', from: [1, 2, 3] });
    expect(t.finds[1]).toMatchObject({ from: [1, 2, 3], face: -1 });
    t.restore();
  });
});

describe('goto', () => {
  it('places the camera over the place found and aims as asked', async () => {
    const t = host([place()]);
    const r = await gotoPlace(t.h, 'water', { height: 2, aim: 'downstream', pitch: -15 });
    expect(r.found).toEqual(place());
    expect(t.views).toEqual([[2, 'downstream', 0, [0, 0, 0], -15, 0]]);
    expect(t.teleports).toEqual([]);
    t.restore();
  });

  it('fails loudly when nothing is found, naming what it looked for', async () => {
    const t = host([null]);
    await expect(gotoPlace(t.h, 'water', { minDepth: 3, minArea: 400, maxS: 1 })).rejects.toThrow(
      /no "water" ≥ 3 m deep ≥ 400 cells wide within 256 m of \[0\.0, 50\.0, 0\.0\] after 1 s/,
    );
    // It kept trying until maxS.
    expect(t.finds.length).toBeGreaterThan(2);
    t.restore();
  });

  it('keeps trying while the currents arrive, then looks downstream', async () => {
    let tries = 0;
    const t = host([place()], {
      viewFound: (height, aim, yaw, _toward, pitch): Camera => {
        tries += 1;
        if (tries < 3) throw new Error('no current at [10, 80, 20] to look along');
        return { position: [0, height, 0], yaw, pitch };
      },
    });
    await gotoPlace(t.h, 'water', { aim: 'downstream', maxS: 5 });
    expect(tries).toBe(3);
    const still = host([place()], {
      viewFound: () => {
        throw new Error('no current at [10, 80, 20] to look along');
      },
    });
    await expect(gotoPlace(still.h, 'water', { aim: 'downstream' })).rejects.toThrow(
      /goto: no current/,
    );
    t.restore();
  });

  it('looks at the nearest lookAt from the place, without forgetting the place', async () => {
    const sand = place({ at: [40.5, 41, 20.5], block: 'dwell:sand' });
    const t = host([place(), sand]);
    await gotoPlace(t.h, 'water', { lookAt: 'dwell:sand', turn: 180 });
    expect(t.finds[1]).toMatchObject({
      query: 'dwell:sand',
      from: [10.5, 40.5, 20.5],
      face: 2,
      keep: false,
    });
    // Toward it, pitched toward it (NaN: the client works the pitch out), turned.
    expect(t.views).toEqual([[1.6, 'toward', 0, [40.5, 41, 20.5], NaN, 180]]);
    t.restore();
  });

  it('waits for a lookAt to stream in, and fails naming it when it never does', async () => {
    const sand = place({ at: [40.5, 41, 20.5], block: 'dwell:sand' });
    // The place, no land yet, the place again, then the land.
    const t = host([place(), null, place(), sand]);
    await gotoPlace(t.h, 'water', { lookAt: 'land', maxS: 5 });
    expect(t.views).toEqual([[1.6, 'toward', 0, [40.5, 41, 20.5], NaN, 0]]);
    t.restore();
    const never = host([place(), null, place(), null]);
    await expect(gotoPlace(never.h, 'water', { lookAt: 'land', maxS: 1 })).rejects.toThrow(
      /no "land" to look at among the loaded chunks near \[10\.5, 40\.5, 20\.5\]/,
    );
    never.restore();
  });

  it('with scope world goes to the map water first, holds the player there and moves it over', async () => {
    const hit = { at: [500.5, 32, 900.5], kind: 'sea', face: 'a', ground: 28, distance: 1000 };
    const asked: unknown[] = [];
    const t = host([null, place({ at: [505.5, 32, 902.5], face: 1 })], {
      findOnMap: (...args) => {
        asked.push(args);
        return Promise.resolve(JSON.stringify(hit));
      },
    });
    const r = await gotoPlace(t.h, 'water', {
      scope: 'world',
      water: 'sea',
      minDepth: 3,
      height: 2,
    });
    expect(asked).toEqual([['sea', [0, 50, 0], 30000, 0, 3]]);
    expect(r.map).toEqual(hit);
    // The cells searched from the map's water; the player held 2 m over it, then over the place.
    expect(t.finds[0]).toMatchObject({ from: [500.5, 32, 900.5], radius: 256 });
    expect(t.teleports[0]).toEqual([500.5, 34, 900.5]);
    expect(t.teleports.at(-1)).toEqual([505.5, 34, 902.5]);
    t.restore();
  });

  it('with scope world says so when the map has no such water', async () => {
    const t = host([place()], { findOnMap: () => Promise.resolve('null') });
    await expect(gotoPlace(t.h, 'water', { scope: 'world', water: 'lake' })).rejects.toThrow(
      /no lake on the world map within 30000 m/,
    );
    t.restore();
  });

  it('moves a face-B player along its up (−y)', async () => {
    const t = host([place({ at: [1.5, -40.5, 2.5], face: -1 })]);
    await gotoPlace(t.h, 'water', { move: true, height: 3 });
    expect(t.teleports).toEqual([[1.5, -43.5, 2.5]]);
    t.restore();
  });
});
