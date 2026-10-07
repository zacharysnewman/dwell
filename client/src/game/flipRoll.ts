// The camera turning over when the player crosses the midplane (BIFACIAL_WORLD.md §6): the view
// switches to the new face's frame at once and a roll of π about the view axis, easing out to 0
// over ROLL_SECONDS, makes it look as though the world turned over smoothly instead of snapping.
// Pure, so tests pin it; the renderer applies the roll (`Renderer.setCamera`).
import type { FaceSign } from '../world/face';

/** How long the camera takes to turn over. */
export const ROLL_SECONDS = 0.5;

const smoothstep = (t: number): number => {
  const x = Math.min(1, Math.max(0, t));
  return x * x * (3 - 2 * x);
};

export class FlipRoll {
  private face: FaceSign | null = null;
  /** Seconds since the face last changed (ROLL_SECONDS or more: at rest). */
  private elapsed = ROLL_SECONDS;

  /** Advances `dtSeconds` with the player on `face`; returns the roll (radians, π … 0). */
  update(face: FaceSign, dtSeconds: number): number {
    if (this.face !== null && face !== this.face) this.elapsed = 0;
    this.face = face;
    this.elapsed = Math.min(ROLL_SECONDS, this.elapsed + dtSeconds);
    return Math.PI * (1 - smoothstep(this.elapsed / ROLL_SECONDS));
  }
}
