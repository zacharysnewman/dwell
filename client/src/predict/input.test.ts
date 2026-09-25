import { describe, expect, it } from 'vitest';
import { InputButtons } from '../protocol/constants.gen';
import {
  dequantizePitch,
  dequantizeYaw,
  IDLE_INPUT,
  quantizeInput,
  quantizePitch,
  quantizeYaw,
} from './input';

describe('input quantization (mirrors C++ player::QuantizeInput)', () => {
  it('quantizes moves, buttons, and view angles', () => {
    const f = quantizeInput(
      { ...IDLE_INPUT, moveY: 1, moveX: -0.5, jump: true, crouch: true, yaw: 90, pitch: -45 },
      7,
    );
    expect(f.seq).toBe(7);
    // (−0.5, 1) is longer than 1: clamped to the unit circle first.
    expect(f.moveX).toBe(Math.round((-0.5 / Math.hypot(0.5, 1)) * 127));
    expect(f.moveY).toBe(Math.round((1 / Math.hypot(0.5, 1)) * 127));
    expect(f.moveX * f.moveX + f.moveY * f.moveY).toBeLessThanOrEqual(127 * 127 + 254);
    expect(f.buttons).toBe(InputButtons.jump | InputButtons.crouch);
    expect(f.yaw).toBe(16384);
    expect(f.pitch).toBe(-16383); // −16383.5 rounds half up, like Math.round
  });

  it('wraps yaw into int16 and clamps pitch', () => {
    expect(quantizeYaw(180)).toBe(-32768);
    expect(quantizeYaw(270)).toBe(-16384);
    expect(quantizeYaw(-90)).toBe(-16384);
    expect(quantizeYaw(360)).toBe(0);
    expect(quantizePitch(120)).toBe(32767);
    expect(quantizePitch(-120)).toBe(-32767);
    expect(dequantizeYaw(16384)).toBe(90);
    expect(dequantizePitch(32767)).toBe(90);
  });
});
