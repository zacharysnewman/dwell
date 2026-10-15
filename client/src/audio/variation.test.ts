import { describe, expect, it } from 'vitest';
import { SOUND_DATA } from './sounds.gen';
import { intensityFactor, MIN_PITCH_STEP, mulberry32, Variation } from './variation';
import type { SoundDef } from './types';

const sound = (id: string): SoundDef => {
  const s = SOUND_DATA.sounds[id];
  if (!s) throw new Error(`no ${id}`);
  return s;
};

describe('variation', () => {
  it('is deterministic for a seed', () => {
    const a = new Variation(42);
    const b = new Variation(42);
    for (let i = 0; i < 20; i++) {
      expect(a.draw('dwell:stone_step', sound('dwell:stone_step'))).toEqual(
        b.draw('dwell:stone_step', sound('dwell:stone_step')),
      );
    }
    expect(new Variation(1).draw('dwell:stone_step', sound('dwell:stone_step'))).not.toEqual(
      new Variation(2).draw('dwell:stone_step', sound('dwell:stone_step')),
    );
  });

  it("keeps every draw within the sound's ranges", () => {
    const v = new Variation(7);
    for (const [id, s] of Object.entries(SOUND_DATA.sounds)) {
      if (s.variation === 'none') continue;
      const r = s.variation;
      for (let i = 0; i < 50; i++) {
        const p = v.draw(id, s);
        expect(Math.abs(p.pitch)).toBeLessThanOrEqual(r.pitch ?? 0);
        expect(Math.abs(p.gainDb)).toBeLessThanOrEqual(r.gain ?? 0);
        expect(Math.abs(p.cutoff - 1)).toBeLessThanOrEqual((r.cutoff ?? 0) + 1e-12);
        expect(Math.abs(p.time - 1)).toBeLessThanOrEqual((r.time ?? 0) + 1e-12);
        s.layers.forEach((layer, k) => {
          const lp = p.layers[k];
          if (!lp) throw new Error('missing layer');
          if (layer.grains) {
            const [lo, hi] = layer.grains.count;
            expect(lp.bursts.length).toBeGreaterThanOrEqual(lo);
            expect(lp.bursts.length).toBeLessThanOrEqual(hi);
            for (const b of lp.bursts) {
              expect(b.at).toBeLessThanOrEqual(layer.grains.spread);
              expect(b.gain).toBeGreaterThanOrEqual(layer.grains.gain[0]);
              expect(b.gain).toBeLessThanOrEqual(layer.grains.gain[1]);
            }
          }
        });
      }
    }
  });

  it('never plays the same sound twice in a row', () => {
    const v = new Variation(3);
    const s = sound('dwell:stone_step');
    let last = v.draw('dwell:stone_step', s);
    for (let i = 0; i < 300; i++) {
      const next = v.draw('dwell:stone_step', s);
      expect(Math.abs(next.pitch - last.pitch)).toBeGreaterThanOrEqual(MIN_PITCH_STEP);
      // The round-robin alternative of a layer with alternatives changes every play.
      const alt = s.layers.findIndex((l) => l.alts);
      expect(next.layers[alt]?.alt).not.toBe(last.layers[alt]?.alt);
      last = next;
    }
  });

  it('draws UI sounds identically every time', () => {
    const v = new Variation(9);
    for (const id of Object.values(SOUND_DATA.ui)) {
      const s = sound(id);
      const first = v.draw(id, s);
      for (let i = 0; i < 5; i++) expect(v.draw(id, s)).toEqual(first);
      expect(first.pitch).toBe(0);
      expect(first.gainDb).toBe(0);
    }
  });

  it('scales a landing by its intensity, from the data', () => {
    const s = sound('dwell:stone_land');
    expect(intensityFactor(s, 'gain', 0)).toBe(s.intensity?.gain?.[0]);
    expect(intensityFactor(s, 'gain', 1)).toBe(s.intensity?.gain?.[1]);
    expect(intensityFactor(s, 'gain', 0.2)).toBeLessThan(intensityFactor(s, 'gain', 0.8));
    expect(intensityFactor(sound('dwell:ui_click'), 'gain', 0)).toBe(1);
  });

  it('generates floats in [0, 1)', () => {
    const r = mulberry32(5);
    for (let i = 0; i < 1000; i++) {
      const x = r();
      expect(x).toBeGreaterThanOrEqual(0);
      expect(x).toBeLessThan(1);
    }
  });
});
