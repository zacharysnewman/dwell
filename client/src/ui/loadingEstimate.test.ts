import { describe, expect, it } from 'vitest';
import { LOADING_ESTIMATE } from './loading.gen';
import {
  BarFill,
  SecondsLabel,
  calibration,
  deviceFactor,
  estimate,
  readDevice,
  roundSeconds,
  stageGuessS,
  type DeviceRead,
} from './loadingEstimate';
import { StageTracker } from './loadingStages';

const T = LOADING_ESTIMATE;
/** A stage's cost entry (the test fails if the data has none). */
const C = (id: keyof typeof T.stages) => {
  const c = T.stages[id];
  if (!c) throw new Error(`no estimate for ${id}`);
  return c;
};
const desktop: DeviceRead = { cores: 8, phone: false, cpuAdapter: false, workers: 4 };
const phone: DeviceRead = { cores: 6, memoryGB: 3, phone: true, cpuAdapter: false, workers: 2 };
const software: DeviceRead = { cores: 4, phone: false, cpuAdapter: true, workers: 2 };

/** A tracker's runs and history for `estimate`. */
const input = (t: StageTracker, device: DeviceRead, nowMs: number, remote = false) => ({
  runs: t.runsNow(),
  history: t.history,
  device,
  remote,
  nowMs,
});

describe('the opening guess per device', () => {
  it('reads the browser: cores and memory where said, the workers at least one', () => {
    expect(readDevice({ hardwareConcurrency: 8, deviceMemory: 8 }, false, 4)).toEqual({
      cores: 8,
      memoryGB: 8,
      phone: false,
      cpuAdapter: false,
      workers: 4,
    });
    expect(readDevice({}, true, 0)).toEqual({ phone: true, cpuAdapter: false, workers: 1 });
  });

  it('is the reference desktop at 1, slower on a phone, low memory and a CPU adapter', () => {
    expect(deviceFactor(desktop)).toBe(1);
    expect(deviceFactor({ phone: false, cpuAdapter: false, workers: 4 })).toBe(1);
    // 6 cores: 8/6; a phone ×2; 3 GB under 4: ×1.4.
    expect(deviceFactor(phone)).toBeCloseTo(
      (8 / 6) * T.device.phoneFactor * T.device.lowMemoryFactor,
    );
    expect(deviceFactor(software)).toBeCloseTo(2 * T.device.cpuFactor);
    // Many cores are clamped: no device is guessed faster than minFactor.
    expect(deviceFactor({ ...desktop, cores: 64 })).toBe(T.device.minFactor);
  });

  it('adds the stages to come for the route, the terrain by its near ring over the workers', () => {
    const t = new StageTracker();
    const local = estimate(input(t, desktop, 0)).leftS;
    const remote = estimate(input(t, desktop, 0, true)).leftS;
    const cost = (id: keyof typeof T.stages) => stageGuessS(id, C(id), 1, 4);
    // Joining a server: no world file, server or spawn search; its connection instead.
    expect(local - remote).toBeCloseTo(
      cost('worldFile') + cost('server') + cost('spawn') - cost('connect'),
    );
    expect(cost('terrain')).toBeCloseTo(
      C('terrain').baseS + (T.ringChunks * (C('terrain').perItemS ?? 0)) / 4,
    );
    // A phone guesses longer than the desktop, a software adapter longer still per stage.
    expect(estimate(input(t, phone, 0)).leftS).toBeGreaterThan(local);
  });

  it('counts only the stages after the latest begun (a respawn waits for the terrain alone)', () => {
    const t = new StageTracker();
    t.progress('terrain', 0, 179, 0);
    const left = estimate(input(t, desktop, 0)).leftS;
    expect(left).toBeCloseTo(stageGuessS('terrain', C('terrain'), 1, 4, 179));
  });
});

describe('refinement as rates arrive', () => {
  it('a download runs at its measured rate, not the guess', () => {
    const t = new StageTracker();
    t.begin('game', 0);
    t.progress('game', 1e6, 9e6, 1000); // 1 MB/s measured, 8 MB to go
    const runs = t.runsNow();
    const e = estimate({ runs, history: [], device: desktop, remote: false, nowMs: 1000 });
    const rest = estimate({
      runs: [],
      history: [],
      device: desktop,
      remote: false,
      nowMs: 0,
    }).leftS;
    const gameGuess = stageGuessS('game', C('game'), 1, 4);
    expect(e.leftS).toBeCloseTo(8 + (rest - gameGuess));
    expect(e.elapsedS).toBe(1);
  });

  it('the terrain blends to its measured time per chunk over its first chunks', () => {
    const t = new StageTracker();
    t.begin('terrain', 0);
    // 4 chunks (minSamples) in 2 s: 0.5 s a chunk measured, 175 to go.
    t.progress('terrain', T.minSamples, 179, 2000);
    expect(estimate(input(t, desktop, 2000)).leftS).toBeCloseTo(
      (179 - T.minSamples) * (2 / T.minSamples),
    );
    // One chunk: a quarter measured, three quarters the guess.
    const u = new StageTracker();
    u.begin('terrain', 0);
    u.progress('terrain', 1, 179, 2000);
    const guessPer = (C('terrain').perItemS ?? 0) / 4;
    const w = 1 / T.minSamples;
    expect(estimate(input(u, desktop, 2000)).leftS).toBeCloseTo(178 * (w * 2 + (1 - w) * guessPer));
  });

  it('a slow device measured on the stages done slows the guesses still to come', () => {
    const t = new StageTracker();
    t.begin('graphics', 0);
    t.end('graphics', 4000); // 10× its 0.4 s guess
    // Damped: 10× slower measured rescales the rest by 10^calibrationPower, not 10.
    expect(calibration(t.history, desktop)).toBeCloseTo(10 ** T.calibrationPower);
    // One fast stage likewise only nudges them, within the clamp.
    const fast = new StageTracker();
    fast.begin('graphics', 0);
    fast.end('graphics', 4);
    expect(calibration(fast.history, desktop)).toBeCloseTo(
      Math.max(0.25, 0.01 ** T.calibrationPower),
    );
    const fresh = estimate({
      runs: [],
      history: [],
      device: desktop,
      remote: false,
      nowMs: 0,
    }).leftS;
    expect(
      estimate({ runs: [], history: t.history, device: desktop, remote: false, nowMs: 0 }).leftS,
    ).toBeGreaterThan(fresh);
  });

  it('a running stage without progress past its guess still has some time left', () => {
    const t = new StageTracker();
    t.begin('spawn', 0);
    const guess = C('spawn').baseS;
    const at = (ms: number) =>
      estimate(input(t, desktop, ms)).leftS - estimate(input(t, desktop, 0)).leftS + guess;
    expect(at(500)).toBeCloseTo(guess - 0.5);
    expect(at(10_000)).toBeCloseTo(T.overrunFraction * 10);
  });
});

describe('the bar', () => {
  it('never goes backward when the estimate grows, and holds below full until it opens', () => {
    const bar = new BarFill();
    let last = 0;
    const steps: Array<[number, number]> = [
      [1, 4],
      [2, 3],
      [2.5, 20], // the estimate jumped up: the bar waits, it does not shrink
      [3, 18],
      [10, 1],
      [20, 0],
      [30, 0],
    ];
    for (const [elapsedS, leftS] of steps) {
      for (let i = 0; i < 20; i++) {
        const f = bar.update({ elapsedS, leftS }, 0.1);
        expect(f).toBeGreaterThanOrEqual(last);
        expect(f).toBeLessThanOrEqual(T.holdFraction);
        last = f;
      }
    }
    expect(bar.holding).toBe(true);
    expect(last).toBeLessThan(1);
    bar.reset();
    expect(bar.shown).toBe(0);
    expect(bar.holding).toBe(false);
  });

  it('eases toward elapsed / (elapsed + left) rather than jumping', () => {
    const bar = new BarFill();
    const once = bar.update({ elapsedS: 5, leftS: 5 }, 0.05);
    expect(once).toBeGreaterThan(0);
    expect(once).toBeLessThan(0.5);
    for (let i = 0; i < 200; i++) bar.update({ elapsedS: 5, leftS: 5 }, 0.05);
    expect(bar.shown).toBeCloseTo(0.5, 3);
    expect(bar.holding).toBe(false);
  });
});

describe('the "About N s" label', () => {
  it('rounds whole seconds, steps of 5 above roundAboveS', () => {
    expect(roundSeconds(3.4)).toBe(3);
    expect(roundSeconds(1.2)).toBe(1);
    expect(roundSeconds(12)).toBe(10);
    expect(roundSeconds(13)).toBe(15);
  });

  it('changes at most every labelEveryS, only past the hysteresis, and hides under a second', () => {
    const l = new SecondsLabel();
    expect(l.update(6.2, 0)).toBe('About 6 s');
    // Too soon, however it moved.
    expect(l.update(3, 100)).toBe('About 6 s');
    // Late enough, but within the hysteresis: kept.
    expect(l.update(6.2 - T.labelHysteresisS / 2, 1000)).toBe('About 6 s');
    // Moved past it: shown.
    expect(l.update(4.4, 2000)).toBe('About 4 s');
    expect(l.update(0.6, 3000)).toBe('');
    l.reset();
    expect(l.update(8, 3100)).toBe('About 8 s');
  });
});
