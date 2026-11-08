import { describe, expect, it } from 'vitest';
import { columnShots, shotPoint, shotYaws, stripSize, type Capture } from './panoramas.ts';

const CAPTURE: Capture = {
  steps: 16,
  sizePx: 1080,
  fovDeg: 90,
  span: 0.9,
  quality: 0.8,
  settleS: 90,
};
const N = CAPTURE.steps;
const STEP = (2 * Math.PI) / N;

describe('the panorama stitch', () => {
  it('takes a column at a shot centre from that shot alone', () => {
    for (let k = 0; k < N; k++) {
      const parts = columnShots(k * STEP, N).filter((p) => p.weight > 1e-9);
      expect(parts).toHaveLength(1);
      expect(parts[0].k).toBe(k);
      expect(parts[0].weight).toBeCloseTo(1, 9);
      expect(parts[0].phi).toBeCloseTo(0, 9);
    }
  });

  it('blends the two shots evenly halfway between them, the weights always summing to 1', () => {
    const mid = columnShots(2.5 * STEP, N);
    expect(mid.map((p) => p.k)).toEqual([2, 3]);
    expect(mid[0].weight).toBeCloseTo(0.5, 12);
    expect(mid[0].phi).toBeCloseTo(STEP / 2, 12);
    expect(mid[1].phi).toBeCloseTo(-STEP / 2, 12);
    for (let i = 0; i < 1000; i++) {
      const parts = columnShots((i / 1000) * 2 * Math.PI, N);
      expect(parts[0].weight + parts[1].weight).toBeCloseTo(1, 12);
      // Every ray is well inside both shots' view (±fov/2).
      for (const p of parts) expect(Math.abs(p.phi)).toBeLessThanOrEqual(STEP + 1e-12);
    }
  });

  it('changes smoothly across a shot boundary: no sharp join', () => {
    const e = 1e-9;
    for (let k = 0; k < N; k++) {
      const before = columnShots(k * STEP - e, N);
      const after = columnShots(k * STEP + e, N);
      // Just before shot k's centre nearly all weight is already shot k's, as just after.
      const w = (ps: { k: number; weight: number }[]) =>
        ps.filter((p) => p.k === k).reduce((a, p) => a + p.weight, 0);
      expect(w(before)).toBeCloseTo(1, 6);
      expect(w(after)).toBeCloseTo(1, 6);
    }
  });

  it("meets itself at the strip's ends", () => {
    const { width } = stripSize(CAPTURE);
    const first = columnShots((0.5 / width) * 2 * Math.PI, N);
    const last = columnShots(((width - 0.5) / width) * 2 * Math.PI, N);
    // The last column leans on shot 0 as the first does, half a column the other side of it.
    const w0 = (ps: { k: number; weight: number }[]) =>
      ps.filter((p) => p.k === 0).reduce((a, p) => a + p.weight, 0);
    expect(w0(first)).toBeCloseTo(1, 4);
    expect(w0(last)).toBeCloseTo(1, 4);
    const at = (ps: { k: number; phi: number }[]) => ps.find((p) => p.k === 0)?.phi ?? NaN;
    expect(at(first)).toBeCloseTo(-at(last), 12);
  });

  it('reprojects one ray to the same direction from both shots it blends', () => {
    const { focal } = stripSize(CAPTURE);
    const c = CAPTURE.sizePx / 2;
    for (const theta of [0.3, 1.1, 2.9, 4.4, 6.0]) {
      for (const t of [-0.8, 0, 0.5]) {
        const dirs = columnShots(theta, N).map((p) => {
          const [x, y] = shotPoint(p.phi, t, focal, CAPTURE.sizePx);
          // Back from the shot's pixel to a world direction: its yaw round and its rise.
          const dx = (x + 0.5 - c) / focal;
          const dy = (c - (y + 0.5)) / focal;
          return [p.k * STEP + Math.atan(dx), dy / Math.hypot(1, dx)];
        });
        const a = dirs[0];
        const b = dirs[1];
        const wrap = (v: number) => ((v % (2 * Math.PI)) + 2 * Math.PI) % (2 * Math.PI);
        expect(wrap(a[0])).toBeCloseTo(wrap(theta), 9);
        expect(wrap(b[0])).toBeCloseTo(wrap(theta), 9);
        expect(a[1]).toBeCloseTo(t, 9);
        expect(b[1]).toBeCloseTo(t, 9);
      }
    }
  });

  it('turns each shot one step right of the last', () => {
    const yaws = shotYaws(
      {
        name: 'x',
        label: 'x',
        seed: 0,
        position: [0, 0, 0],
        yaw: 30,
        timeOfDay: 0.3,
        image: 'x.webp',
      },
      CAPTURE,
    );
    expect(yaws).toHaveLength(N);
    expect(yaws[0]).toBe(30);
    expect(yaws[1]).toBeCloseTo(30 - 22.5, 12);
  });

  it('runs in the page as written (the functions are self-contained)', () => {
    // eslint-disable-next-line @typescript-eslint/no-implied-eval
    const make = new Function(`return ${columnShots.toString()}`) as () => typeof columnShots;
    const fn = make();
    expect(fn(2.5 * STEP, N)).toEqual(columnShots(2.5 * STEP, N));
  });
});
