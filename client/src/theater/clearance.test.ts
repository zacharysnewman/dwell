import { describe, expect, it } from 'vitest';
import data from '../../../shared/theater/cinematic.json';
import { CINEMATIC, parseCinematic, withOverrides } from './cinematic';
import {
  ClearanceCheck,
  keyTimes,
  runClearancePass,
  type ClearanceConfig,
  type ClearanceWorld,
  type Vec3,
} from './clearance';

const cfg: ClearanceConfig = { ...CINEMATIC.clearance, radiusM: 0.25, stepS: 0.125 };

/**
 * A fake world: the ground's top at y 10, a wall over x 20..24 up to y 18, loaded where `loaded`
 * says. The path is a straight line at 1 m/s from (0, y, 0.5) along +x for `duration` s, at height
 * `y(t)`; its keyframes every 10 s.
 */
function fakeWorld(
  y: (t: number) => number,
  duration = 40,
  loaded: (p: Vec3) => boolean = () => true,
) {
  const solidAt = ([x, py]: Vec3): boolean => py < 10 || (x >= 20 && x < 24 && py < 18);
  const near = (p: Vec3, r: number): boolean =>
    [
      [0, 0, 0],
      [r, 0, 0],
      [-r, 0, 0],
      [0, r, 0],
      [0, -r, 0],
    ].some(([dx = 0, dy = 0]) => solidAt([p[0] + dx, p[1] + dy, p[2]]));
  const world: ClearanceWorld = {
    samples: (_json, stepS) => {
      const out: number[] = [];
      for (let i = 0; i * stepS <= duration + 1e-9; i++) {
        const t = i * stepS;
        out.push(t, t, t, y(t), 0.5);
      }
      return out;
    },
    probe: (eyes, r, step, max) => {
      const codes: number[] = [];
      const lifts: number[] = [];
      const lift = (p: Vec3): number => {
        for (let h = 0; h <= max; h += step) if (!near([p[0], p[1] + h, p[2]], r)) return h;
        return NaN;
      };
      for (let i = 0; i < eyes.length; i += 3) {
        const p: Vec3 = [eyes[i] ?? 0, eyes[i + 1] ?? 0, eyes[i + 2] ?? 0];
        const c = near(p, r) ? 1 : loaded(p) ? 0 : -1;
        codes.push(c);
        lifts.push(c === 1 ? lift(p) : NaN);
      }
      return Promise.resolve({ codes, lifts });
    },
  };
  return world;
}

const path = JSON.stringify({
  formatVersion: 1,
  keyframes: [{ t: 0 }, { t: 10 }, { t: 20 }, { t: 30 }, { t: 40 }],
});

describe('the camera clearance check', () => {
  it('finds a path through a wall: its times, eye, keyframes and the lift that clears it', async () => {
    const check = new ClearanceCheck(
      fakeWorld(() => 15),
      path,
      cfg,
    );
    await check.probeUpTo(Infinity);
    const r = check.result();
    expect(r.clipping).toHaveLength(1);
    const run = r.clipping[0];
    // within 0.25 m of the wall's sides (x 20..24): 19.75 to 24.125 (the sample step 0.125)
    expect(run?.fromS).toBe(19.75);
    expect(run?.toS).toBe(24.125);
    expect(run?.at[0]).toBe(19.75);
    expect(run?.keys).toEqual([1, 3]);
    // the wall's top at 18: from 15 the eye clears at 18.5 (0.5 m steps of the data): a 3.5 m lift
    expect(run?.liftM).toBe(3.5);
    expect(r.unchecked).toBe(0);
    expect(r.message).toMatch(/passes through solid ground \(within 0\.25 m\) in 1 place:/);
    expect(r.message).toMatch(/19\.75–24\.1\d s .*keyframes 1–3.*lift the camera ≥ 3\.5 m/);
    expect(r.message).toMatch(/allowClipping: true/);
  });

  it('passes a clear path (over the wall)', async () => {
    const check = new ClearanceCheck(
      fakeWorld(() => 19),
      path,
      cfg,
    );
    await check.probeUpTo(Infinity);
    const r = check.result();
    expect(r.clipping).toEqual([]);
    expect(r.message).toBe('');
    expect(r.samples).toBe(321);
  });

  it('finds each dip into the ground as its own run', async () => {
    // over the wall, but for dips under the ground's top (10) at t 5 and t 35
    const y = (t: number): number => (Math.abs(t - 5) < 1 || Math.abs(t - 35) < 1 ? 9.5 : 19);
    const check = new ClearanceCheck(fakeWorld(y), path, cfg);
    await check.probeUpTo(Infinity);
    const runs = check.result().clipping;
    expect(runs.map((r) => [r.fromS, r.toS].map((n) => Math.round(n * 10) / 10))).toEqual([
      [4.1, 5.9],
      [34.1, 35.9],
    ]);
    expect(runs[0]?.keys).toEqual([0, 1]);
  });

  it('counts unloaded eyes as unchecked, never as clear, and probes them again later', async () => {
    let far = 30;
    const world = fakeWorld(
      () => 15,
      40,
      (p) => p[0] < far,
    );
    const check = new ClearanceCheck(world, path, cfg);
    expect(await check.probeUpTo(Infinity)).toBe(81); // x 30..40: 81 samples
    expect(check.result().unchecked).toBe(81);
    far = 100;
    expect(await check.probeUpTo(Infinity)).toBe(0);
    expect(check.result().unchecked).toBe(0);
  });

  it('probes only the samples up to a time', async () => {
    const check = new ClearanceCheck(
      fakeWorld(() => 15),
      path,
      cfg,
    );
    await check.probeUpTo(10);
    expect(check.result().clipping).toEqual([]);
    expect(check.result().unchecked).toBe(240);
    await check.probeUpTo(25);
    expect(check.result().clipping).toHaveLength(1);
  });

  it('a lift no step clears says so', async () => {
    const check = new ClearanceCheck(
      fakeWorld(() => 15),
      path,
      { ...cfg, liftMaxM: 2 },
    );
    await check.probeUpTo(Infinity);
    expect(check.result().clipping[0]?.liftM).toBeNull();
    expect(check.result().message).toMatch(/no lift up to 2 m clears it/);
  });

  it("keyframe times are the path's, from its first", () => {
    expect(keyTimes({ keyframes: [{ t: 2 }, { t: 5 }] })).toEqual([0, 3]);
    expect(keyTimes('{"keyframes":[]}')).toEqual([]);
    expect(keyTimes({})).toEqual([]);
  });

  it('the quick pass flies the path in steps, waits for chunks, and reports the clip', async () => {
    let camX = 0;
    let wall = 0;
    let played = false;
    const advances: number[] = [];
    // the chunks within 5 m of the camera are loaded
    const world = fakeWorld(
      () => 15,
      40,
      (p) => Math.abs(p[0] - camX) <= 5,
    );
    const r = await runClearancePass(
      {
        clearance: world,
        theaterExact: () => undefined,
        theaterAdvance: (dt) => {
          advances.push(dt);
          camX += dt;
        },
        theaterPlay: () => {
          played = true;
        },
        theaterStop: () => undefined,
        theaterState: () => ({ time: camX, duration: 40, finished: camX >= 40 }),
        nextFrame: () => {
          wall += 16;
          return Promise.resolve(true);
        },
        wallMs: () => wall,
      },
      path,
      { ...cfg, passStepS: 1 },
    );
    expect(played).toBe(true);
    expect(advances).toHaveLength(40);
    expect(r.unchecked).toBe(0);
    expect(r.clipping).toHaveLength(1);
    expect(r.clipping[0]?.fromS).toBe(19.75);
  });

  it('a world that knows every eye (a local one) needs no pass: nothing is flown', async () => {
    const advances: number[] = [];
    let played = false;
    const r = await runClearancePass(
      {
        clearance: fakeWorld(() => 15),
        theaterExact: () => undefined,
        theaterAdvance: (dt) => advances.push(dt),
        theaterPlay: () => {
          played = true;
        },
        theaterStop: () => undefined,
        theaterState: () => null,
        nextFrame: () => Promise.resolve(true),
        wallMs: () => 0,
      },
      path,
      cfg,
    );
    expect(played).toBe(false);
    expect(advances).toEqual([]);
    expect(r.clipping).toHaveLength(1);
  });

  it('the settings validate (radius and steps in range)', () => {
    expect(CINEMATIC.clearance.radiusM).toBe(0.3);
    expect(() =>
      parseCinematic(
        withOverrides(data as Record<string, unknown>, { clearance: { radiusM: -1 } }),
      ),
    ).toThrow(/clearance.radiusM must be a number in \[0, 5\]/);
    expect(() =>
      parseCinematic(withOverrides(data as Record<string, unknown>, { clearance: { stepS: 0 } })),
    ).toThrow(/clearance.stepS/);
  });
});
