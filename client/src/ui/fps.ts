// Frames per second for the status line: frames counted over each full second (a steady figure
// rather than one that flickers every frame).

export class FpsMeter {
  private windowStart: number | null = null;
  private frames = 0;
  private value: number | null = null;

  /** Call once per rendered frame with its timestamp (ms). */
  frame(nowMs: number): void {
    if (this.windowStart === null) {
      this.windowStart = nowMs;
      return;
    }
    this.frames++;
    const elapsed = nowMs - this.windowStart;
    if (elapsed >= 1000) {
      this.value = (this.frames * 1000) / elapsed;
      this.frames = 0;
      this.windowStart = nowMs;
    }
  }

  /** The last full second's rate, or null before one has passed. */
  get fps(): number | null {
    return this.value;
  }
}
