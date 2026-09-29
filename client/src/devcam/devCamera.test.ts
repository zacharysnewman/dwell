import { describe, expect, it } from 'vitest';
import { IDLE_INPUT } from '../predict/input';
import { World } from '../protocol/constants.gen';
import { DevCamera, devCameraSpeed, MAX_ALTITUDE } from './devCamera';

describe('dev camera (§6.6)', () => {
  it('speeds up with altitude, so the whole disc is in reach within seconds', () => {
    expect(devCameraSpeed(0, false)).toBeLessThan(20);
    expect(devCameraSpeed(1e6, false)).toBeGreaterThan(1e6);
    const cam = new DevCamera();
    cam.toggle([10, 2, 10]);
    let t = 0;
    // Hold Space and Shift from the ground.
    while (cam.position[1] < 2 * World.worldRadius && t < 60) {
      cam.update(1 / 60, { ...IDLE_INPUT, jump: true, run: true }, 0, 0);
      t += 1 / 60;
    }
    expect(t).toBeLessThan(10);
    // Its horizontal position did not move, and altitude is capped.
    expect(cam.position[0]).toBe(10);
    for (let i = 0; i < 600; i++)
      cam.update(1 / 60, { ...IDLE_INPUT, jump: true, run: true }, 0, 0);
    expect(cam.position[1]).toBe(MAX_ALTITUDE);
  });

  it('flies along the view: forward at yaw 90° is +X; off keeps its position until toggled again', () => {
    const cam = new DevCamera();
    cam.update(1, { ...IDLE_INPUT, moveY: 1 }, 90, 0);
    expect(cam.position).toEqual([0, 0, 0]); // inactive
    cam.toggle([0, 0, 0]);
    cam.update(1, { ...IDLE_INPUT, moveY: 1 }, 90, 0);
    expect(cam.position[0]).toBeCloseTo(devCameraSpeed(0, false));
    expect(Math.abs(cam.position[2])).toBeLessThan(1e-9);
    cam.toggle([0, 0, 0]);
    expect(cam.active).toBe(false);
  });
});
