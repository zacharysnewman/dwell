// The dev camera (ARCHITECTURE.md §6.6): a client-side free-fly camera detached from the player's
// body, for looking at the whole-world view before real altitude exists. It only moves what the
// client renders and asks the LOD system for; the body stays where it is (it gets no input) and
// the server keeps streaming full-detail chunks around it. F8 or `?devcam=1` toggles it.
import type { Vec3 } from '../protocol/messages';
import type { PlayerInputState } from '../predict/input';

/** Speed at the ground (m/s), and how much faster per metre of altitude (1/s). */
const BASE_SPEED = 12;
const SPEED_PER_METRE = 1.2;
/** Run (Shift) multiplies the speed. */
const RUN_FACTOR = 4;
/** High enough to see the whole 8,192 km disc (its radius at a 37.5° half field of view, ×2). */
export const MAX_ALTITUDE = 24_000_000;

export function devCameraSpeed(y: number, running: boolean): number {
  const altitude = Math.max(0, y);
  return (BASE_SPEED + altitude * SPEED_PER_METRE) * (running ? RUN_FACTOR : 1);
}

export class DevCamera {
  active = false;
  position: Vec3 = [0, 0, 0];

  /** Turns the camera on at `from` (the player's eye) or off. */
  toggle(from: Vec3): void {
    this.active = !this.active;
    if (this.active) this.position = [...from];
  }

  /**
   * Moves along the view (yaw/pitch in degrees, as the renderer's camera): move.y forward, move.x
   * right, jump up, crouch down; the speed grows with altitude so the climb to the whole-disc view
   * takes seconds, not hours.
   */
  update(dt: number, input: PlayerInputState, yawDeg: number, pitchDeg: number): void {
    if (!this.active) return;
    const yaw = (yawDeg * Math.PI) / 180;
    const pitch = (pitchDeg * Math.PI) / 180;
    const forward: Vec3 = [
      Math.sin(yaw) * Math.cos(pitch),
      Math.sin(pitch),
      Math.cos(yaw) * Math.cos(pitch),
    ];
    const right: Vec3 = [-Math.cos(yaw), 0, Math.sin(yaw)];
    const up = (input.jump ? 1 : 0) - (input.crouch ? 1 : 0);
    const speed = devCameraSpeed(this.position[1], input.run) * dt;
    for (let a = 0; a < 3; a++) {
      this.position[a] =
        (this.position[a] ?? 0) +
        speed *
          ((forward[a] ?? 0) * input.moveY + (right[a] ?? 0) * input.moveX + (a === 1 ? up : 0));
    }
    this.position[1] = Math.min(MAX_ALTITUDE, this.position[1]);
  }
}
