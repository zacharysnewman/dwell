import { describe, expect, it } from 'vitest';
import {
  isTap,
  STICK_RADIUS,
  TAP_MS,
  TAP_SLOP,
  TOUCH_BUTTONS,
  TouchButtonState,
  stickOutput,
} from './touch';

describe('touch stick', () => {
  it('maps drags to a move vector: up is forward, right is right', () => {
    const up = stickOutput(0, -STICK_RADIUS);
    expect(up.moveX).toBeCloseTo(0);
    expect(up.moveY).toBeCloseTo(1);
    const right = stickOutput(STICK_RADIUS, 0);
    expect(right.moveX).toBeCloseTo(1);
    expect(right.moveY).toBeCloseTo(0);
  });

  it('has a dead zone, clamps to the unit circle, and runs when dragged past the ring', () => {
    expect(stickOutput(3, 2)).toEqual({ moveX: 0, moveY: 0, run: false });
    const far = stickOutput(STICK_RADIUS * 2, -STICK_RADIUS * 2);
    expect(Math.hypot(far.moveX, far.moveY)).toBeCloseTo(1);
    expect(far.run).toBe(true);
    expect(stickOutput(0, -STICK_RADIUS).run).toBe(false);
    const half = stickOutput(0, -STICK_RADIUS / 2);
    expect(half.moveY).toBeGreaterThan(0.3);
    expect(half.moveY).toBeLessThan(0.5);
  });
});

describe('touch buttons', () => {
  const pressAndRelease = (b: TouchButtonState) => [b.press(), b.release()];

  it('crouch is held, not toggled: releasing the button stands up', () => {
    const crouch = new TouchButtonState(TOUCH_BUTTONS.crouch.mode);
    expect(pressAndRelease(crouch)).toEqual([true, false]);
    expect(pressAndRelease(crouch)).toEqual([true, false]);
  });

  it('jump is held and run latches on and off per tap', () => {
    const jump = new TouchButtonState(TOUCH_BUTTONS.jump.mode);
    expect(pressAndRelease(jump)).toEqual([true, false]);
    const run = new TouchButtonState(TOUCH_BUTTONS.run.mode);
    expect(pressAndRelease(run)).toEqual([true, true]);
    expect(pressAndRelease(run)).toEqual([false, false]);
  });

  it('the Break/Place button latches: one tap switches to placing, the next back', () => {
    const edit = new TouchButtonState(TOUCH_BUTTONS.edit.mode);
    expect(pressAndRelease(edit)).toEqual([true, true]);
    expect(pressAndRelease(edit)).toEqual([false, false]);
  });
});

describe('touch taps on the view (§6.5)', () => {
  it('a short touch that barely moves is a tap; a drag or a long press is not', () => {
    expect(isTap(0, 0, 80)).toBe(true);
    expect(isTap(TAP_SLOP, 0, TAP_MS)).toBe(true);
    expect(isTap(TAP_SLOP + 1, 0, 80)).toBe(false);
    expect(isTap(3, 4, TAP_MS + 1)).toBe(false);
  });
});
