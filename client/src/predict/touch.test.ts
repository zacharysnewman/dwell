import { describe, expect, it } from 'vitest';
import { STICK_RADIUS, TOUCH_BUTTONS, TouchButtonState, stickOutput } from './touch';

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
});
