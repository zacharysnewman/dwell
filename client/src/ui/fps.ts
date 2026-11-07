// Frames per second for the status line: frames drawn, counted over each full second (a steady
// figure rather than one that flickers every frame). An animation frame the game skipped (the GPU
// behind, render/pacing.rs) isn't one: counting every animation frame read 60 on a phone drawing 30
// (owner, 2026-11-06).

export class FpsMeter {
  private windowStart: number | null = null;
  private frames = 0;
  private animationFrames = 0;
  private value: number | null = null;
  private animationValue: number | null = null;

  /** Call once per animation frame with its timestamp (ms) and whether the game drew it. */
  frame(nowMs: number, drew: boolean): void {
    if (this.windowStart === null) {
      this.windowStart = nowMs;
      return;
    }
    if (drew) this.frames++;
    this.animationFrames++;
    const elapsed = nowMs - this.windowStart;
    if (elapsed >= 1000) {
      this.value = (this.frames * 1000) / elapsed;
      this.animationValue = (this.animationFrames * 1000) / elapsed;
      this.frames = 0;
      this.animationFrames = 0;
      this.windowStart = nowMs;
    }
  }

  /** The last full second's rate, or null before one has passed. */
  get fps(): number | null {
    return this.value;
  }

  /**
   * The browser's animation frames a second over the same second (the display's refresh, when
   * the page keeps up): drawn below it is frames the game skipped.
   */
  get animationFps(): number | null {
    return this.animationValue;
  }
}
