// The camera turning over when the player crosses the midplane (BIFACIAL_WORLD.md §6): the heading
// turns by 180° at once and the view is drawn rotated about its right axis by π, easing out to 0
// over FLIP_SECONDS, so it starts exactly at the old view and turns over nose first. Pure, so tests
// pin it; the renderer applies the angle (`Renderer.setCamera`).
import type { FaceSign } from '../world/face';

/** How long the camera takes to turn over. */
export const FLIP_SECONDS = 0.5;

const smoothstep = (t: number): number => {
  const x = Math.min(1, Math.max(0, t));
  return x * x * (3 - 2 * x);
};

export class FaceFlip {
  private face: FaceSign | null = null;
  private drawFace: FaceSign | null = null;
  /** Seconds since the drawn face last changed (FLIP_SECONDS or more: at rest). */
  private elapsed = FLIP_SECONDS;

  /** Per simulation tick with the predicted face: degrees to add to the heading (0 or 180). */
  tick(face: FaceSign): number {
    const turn = this.face !== null && face !== this.face ? 180 : 0;
    this.face = face;
    return turn;
  }

  /** Per drawn frame: the camera's pitch-over angle (radians, π … 0) about its right axis. */
  draw(face: FaceSign, dtSeconds: number): number {
    if (this.drawFace !== null && face !== this.drawFace) this.elapsed = 0;
    this.drawFace = face;
    this.elapsed = Math.min(FLIP_SECONDS, this.elapsed + dtSeconds);
    return Math.PI * (1 - smoothstep(this.elapsed / FLIP_SECONDS));
  }
}
