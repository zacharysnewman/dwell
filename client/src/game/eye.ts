// First-person eye height smoothing (PLAYER_CONTROLLER.md §9). The controller moves the body in
// jumps the camera must not show: onto or down a step in one tick (step-up, ground snap), and by
// up to a body height when crouching or standing in mid-air (the head stays put, the feet move).
// Each jump is folded into an offset that then decays, so the eye always moves continuously.
// `EyeSmoother` runs once per sim tick; `EyeCamera` feeds it client states and interpolates its
// last two results for each drawn frame.
import { ControllerFlags } from '../protocol/constants.gen';
import type { ClientState } from '../sim/clientCore';

/** Step offset decay: a fixed speed (m/s, PPC SmoothSteps) plus a share of the offset (1/s). */
const STEP_SPEED = 4;
const STEP_RATE = 6;
/** Crouch offset decay rate (1/s). */
const CROUCH_RATE = 12;
/** A grounded per-tick feet change larger than this is a step, not riding a lift or a slope. */
const STEP_MIN = 0.1;
/** A crouch offset larger than this is a teleport (respawn), not a crouch. */
const CROUCH_MAX = 2;

export interface EyeSample {
  /** Feet height (drawn position). */
  feet: number;
  crouched: boolean;
  grounded: boolean;
  velocityY: number;
  eyeHeight: number;
  crouchEyeHeight: number;
  maxStepHeight: number;
}

export function eyeOf(s: EyeSample): number {
  return s.feet + (s.crouched ? s.crouchEyeHeight : s.eyeHeight);
}

export class EyeSmoother {
  private step = 0;
  private crouch = 0;
  private last: EyeSample | null = null;

  /** Advances one tick of `dt` seconds; returns the camera's eye height for that tick. */
  tick(s: EyeSample, dt: number): number {
    const stepDecay = (STEP_SPEED + Math.abs(this.step) * STEP_RATE) * dt;
    this.step = Math.abs(this.step) <= stepDecay ? 0 : this.step - Math.sign(this.step) * stepDecay;
    this.crouch *= Math.exp(-CROUCH_RATE * dt);

    const last = this.last;
    if (last) {
      const dFeet = s.feet - last.feet;
      if (s.crouched !== last.crouched) {
        // Everything but the tick's own motion is the crouch.
        this.crouch -= eyeOf(s) - eyeOf(last) - s.velocityY * dt;
      } else if (
        s.grounded &&
        last.grounded &&
        Math.abs(dFeet) > STEP_MIN &&
        Math.abs(dFeet) <= s.maxStepHeight + 0.05
      ) {
        this.step -= dFeet;
      }
    }
    if (Math.abs(this.step) > s.maxStepHeight + 0.05) this.step = 0;
    if (Math.abs(this.crouch) > CROUCH_MAX) this.crouch = 0;
    this.last = s;
    return eyeOf(s) + this.step + this.crouch;
  }
}

/** The parts of a predicted client state the camera reads. */
export type EyeState = Pick<
  ClientState,
  | 'position'
  | 'renderOffset'
  | 'halfHeight'
  | 'velocity'
  | 'controllerFlags'
  | 'eyeHeight'
  | 'crouchEyeHeight'
  | 'maxStepHeight'
>;

/** Feet height from a state's own half height: the capsule centre moves when crouching. */
export function eyeSampleOf(s: EyeState): EyeSample {
  return {
    feet: s.position[1] + s.renderOffset[1] - s.halfHeight,
    crouched: (s.controllerFlags & ControllerFlags.crouching) !== 0,
    grounded: (s.controllerFlags & ControllerFlags.grounded) !== 0,
    velocityY: s.velocity[1],
    eyeHeight: s.eyeHeight,
    crouchEyeHeight: s.crouchEyeHeight,
    maxStepHeight: s.maxStepHeight,
  };
}

/** The first-person camera's eye height: smoothed per tick, interpolated per frame. */
export class EyeCamera {
  private readonly smoother = new EyeSmoother();
  private previous: number | null = null;
  private current = 0;

  /** Advances one sim tick of `dt` seconds with the newly predicted state. */
  tick(s: EyeState, dt: number): void {
    const eye = this.smoother.tick(eyeSampleOf(s), dt);
    this.previous = this.previous === null ? eye : this.current;
    this.current = eye;
  }

  /** Eye height drawn `alpha` ∈ [0, 1) of the way from the previous tick to the current one. */
  draw(alpha: number): number {
    const previous = this.previous ?? this.current;
    return previous + (this.current - previous) * alpha;
  }
}
