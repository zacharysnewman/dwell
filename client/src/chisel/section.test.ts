import { describe, expect, it } from 'vitest';
import {
  SECTION_SIZES,
  exaggeration,
  eyeOf,
  lookAt,
  mul,
  perspective,
  sectionCorner,
  sectionFromParams,
  sectionPasses,
  sectionToParams,
  type SectionView,
} from './section';

describe('Chisel 3D section', () => {
  it('builds each square coarse first, then fine, covering the whole square', () => {
    expect(sectionPasses(1024)).toEqual([
      { n: 256, texel: 4 },
      { n: 512, texel: 2 },
    ]);
    expect(sectionPasses(256)[1]).toEqual({ n: 256, texel: 1 });
    for (const size of SECTION_SIZES) {
      const ps = sectionPasses(size);
      expect(ps[0]?.texel ?? 0).toBeGreaterThan(ps[1]?.texel ?? 0);
      for (const p of ps) {
        expect(p.n * p.texel).toBe(size);
        expect(p.n).toBeLessThanOrEqual(512);
      }
    }
    expect(sectionCorner({ on: true, x: 500, z: -300, size: 1024 })).toEqual({
      x0: -12,
      z0: -812,
    });
    expect(exaggeration(1024)).toBe(1);
    expect(exaggeration(8192)).toBeGreaterThan(1);
  });

  it('round-trips its state through the URL, and ignores a bad size', () => {
    const v: SectionView = { on: true, x: -470, z: 860, size: 2048 };
    const p = new URLSearchParams('seed=7');
    sectionToParams(v, p);
    expect(sectionFromParams(p, { on: false, x: 0, z: 0, size: 1024 })).toEqual(v);
    sectionToParams({ ...v, on: false }, p);
    expect(sectionFromParams(p, v).on).toBe(false);
    const bad = new URLSearchParams('ssize=999&sx=abc');
    expect(sectionFromParams(bad, v)).toEqual({ ...v, on: false });
  });

  it('projects the orbit target to the centre of the screen', () => {
    const o = {
      target: [100, 40, 200] as [number, number, number],
      yaw: 0.7,
      pitch: 0.5,
      dist: 900,
    };
    const m = mul(perspective(0.8, 4 / 3, 1, 5000), lookAt(eyeOf(o), o.target, [0, 1, 0]));
    const [x, y, z] = o.target;
    const clip = [0, 1, 2, 3].map(
      (r) => (m[r] ?? 0) * x + (m[4 + r] ?? 0) * y + (m[8 + r] ?? 0) * z + (m[12 + r] ?? 0),
    );
    const w = clip[3] ?? 1;
    expect((clip[0] ?? 1) / w).toBeCloseTo(0, 5);
    expect((clip[1] ?? 1) / w).toBeCloseTo(0, 5);
    const depth = (clip[2] ?? 0) / w;
    expect(depth).toBeGreaterThan(0);
    expect(depth).toBeLessThan(1);
  });
});
