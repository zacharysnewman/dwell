import { describe, expect, it } from 'vitest';
import { EyeSmoother, eyeOf, type EyeSample } from './eye';

const DT = 1 / 60;
const sample = (feet: number, over: Partial<EyeSample> = {}): EyeSample => ({
  feet,
  crouched: false,
  grounded: true,
  velocityY: 0,
  eyeHeight: 1.62,
  crouchEyeHeight: 0.8,
  maxStepHeight: 0.55,
  ...over,
});

/** Runs `samples` through a smoother; returns the eye per tick. */
function run(samples: EyeSample[]): number[] {
  const eye = new EyeSmoother();
  return samples.map((s) => eye.tick(s, DT));
}

const largestTickChange = (eyes: number[]) =>
  Math.max(...eyes.slice(1).map((e, i) => Math.abs(e - (eyes[i] ?? e))));

describe('EyeSmoother', () => {
  it('follows the feet exactly on flat ground', () => {
    const eyes = run([sample(0), sample(0), sample(0)]);
    expect(eyes).toEqual([1.62, 1.62, 1.62]);
  });

  it('smooths a step up and a step down (ground snap) alike', () => {
    for (const rise of [0.5, -0.5]) {
      const samples = [...Array<number>(5).fill(0), ...Array<number>(30).fill(rise)].map((f) =>
        sample(f),
      );
      const eyes = run(samples);
      expect(largestTickChange(eyes)).toBeLessThan(0.12);
      expect(eyes.at(-1)).toBeCloseTo(1.62 + rise, 6);
    }
  });

  it('keeps up with running down a staircase (a step every 0.125 s)', () => {
    const samples = Array.from({ length: 60 }, (_, i) => sample(-0.5 * Math.floor(i / 7.5)));
    const eyes = run(samples);
    expect(largestTickChange(eyes)).toBeLessThan(0.12);
    samples.forEach((s, i) => {
      expect(Math.abs((eyes[i] ?? 0) - eyeOf(s))).toBeLessThan(0.55);
    });
  });

  it('does not smooth falls, jumps, or lifts', () => {
    const fall = Array.from({ length: 20 }, (_, i) => sample(-0.2 * i, { grounded: false }));
    run(fall).forEach((e, i) => {
      expect(e).toBeCloseTo(eyeOf(fall[i] ?? sample(0)), 6);
    });
    const lift = Array.from({ length: 20 }, (_, i) => sample(0.05 * i));
    run(lift).forEach((e, i) => {
      expect(e).toBeCloseTo(eyeOf(lift[i] ?? sample(0)), 6);
    });
  });

  it('lowers the eye smoothly when crouching on the ground', () => {
    const samples = [sample(0), ...Array.from({ length: 40 }, () => sample(0, { crouched: true }))];
    const eyes = run(samples);
    expect(largestTickChange(eyes)).toBeLessThan(0.2);
    expect(eyes[1]).toBeCloseTo(1.62, 1);
    expect(eyes.at(-1)).toBeCloseTo(0.8, 2);
  });

  it('crouching in mid-air (feet pulled up 0.9 m) moves the eye only continuously', () => {
    // Falling at 3 m/s; on tick 5 the feet jump up by the height difference.
    const vy = -3;
    const samples = Array.from({ length: 40 }, (_, i) => {
      const crouched = i >= 5;
      return sample(10 + vy * DT * i + (crouched ? 0.9 : 0), {
        grounded: false,
        crouched,
        velocityY: vy,
      });
    });
    const eyes = run(samples);
    // Never more than the fall itself plus a little eye settling per tick.
    expect(largestTickChange(eyes)).toBeLessThan(Math.abs(vy) * DT + 0.02);
    expect(eyes.at(-1)).toBeCloseTo(eyeOf(samples.at(-1) ?? sample(0)), 2);
  });

  it('a quick crouch tap dips and recovers without a jump', () => {
    const samples = [
      sample(0),
      sample(0, { crouched: true }),
      sample(0, { crouched: true }),
      ...Array.from({ length: 40 }, () => sample(0)),
    ];
    const eyes = run(samples);
    expect(largestTickChange(eyes)).toBeLessThan(0.2);
    expect(eyes.at(-1)).toBeCloseTo(1.62, 2);
  });

  it('a teleport is not smoothed', () => {
    const eyes = run([sample(0), sample(30, { crouched: true })]);
    expect(eyes[1]).toBeCloseTo(30.8, 6);
  });
});
