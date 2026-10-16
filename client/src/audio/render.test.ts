import { describe, expect, it } from 'vitest';
import { softClip } from './mixer';
import { renderPlan } from './renderPlan';
import { SOUND_DATA } from './sounds.gen';
import { checkRender, encodeWav, renderStats } from './wav';

describe('offline renders', () => {
  it('plan every set event, event, action and UI sound, plus sequences', () => {
    const names = renderPlan(SOUND_DATA).map((j) => j.name);
    for (const [set, events] of Object.entries(SOUND_DATA.sets)) {
      const local = set.split(':')[1] ?? '';
      for (const e of Object.keys(events)) expect(names).toContain(`set_${local}_${e}`);
      expect(names).toContain(`steps_${local}_walk_x10`);
      expect(names).toContain(`break_${local}_x4`);
    }
    for (const k of Object.keys(SOUND_DATA.ui)) expect(names).toContain(`ui_${k}`);
    for (const k of Object.keys(SOUND_DATA.actions)) expect(names).toContain(`action_${k}`);
    expect(new Set(names).size).toBe(names.length);
    const walk = renderPlan(SOUND_DATA).find((j) => j.name === 'steps_stone_walk_x10');
    expect(walk?.plays.length).toBe(10);
  });

  it('check a render for silence, clipping and length', () => {
    const rate = 1000;
    const tone = new Float32Array(rate).map((_, i) => (i < 300 ? 0.5 * Math.sin(i) : 0));
    const s = renderStats([tone], rate);
    expect(s.peak).toBeCloseTo(0.5, 1);
    expect(s.audible).toBeCloseTo(0.3, 1);
    expect(checkRender(s, { minSeconds: 0.1, maxSeconds: 1 })).toEqual([]);
    expect(checkRender({ ...s, peak: 0 }, { minSeconds: 0, maxSeconds: 1 })[0]).toMatch(/silent/);
    expect(checkRender({ ...s, peak: 1.2 }, { minSeconds: 0, maxSeconds: 1 })[0]).toMatch(/clips/);
    expect(checkRender(s, { minSeconds: 0.5, maxSeconds: 1 })[0]).toMatch(/too short/);
  });

  it('encode 16-bit PCM WAV', () => {
    const wav = encodeWav([new Float32Array([0, 1, -1]), new Float32Array([0.5, 0, 0])], 48000);
    const view = new DataView(wav.buffer);
    expect(String.fromCharCode(...wav.subarray(0, 4))).toBe('RIFF');
    expect(String.fromCharCode(...wav.subarray(8, 12))).toBe('WAVE');
    expect(view.getUint16(22, true)).toBe(2);
    expect(view.getUint32(24, true)).toBe(48000);
    expect(wav.length).toBe(44 + 3 * 2 * 2);
    expect(view.getInt16(44 + 4 * 2, true)).toBe(-32768);
  });

  it('limit softly: linear to the knee, never past full scale', () => {
    expect(softClip(0.5)).toBe(0.5);
    expect(softClip(-0.69)).toBe(-0.69);
    expect(softClip(0.9)).toBeLessThan(0.9);
    expect(softClip(100)).toBeLessThanOrEqual(1);
    expect(softClip(-100)).toBeGreaterThanOrEqual(-1);
  });
});
