// Creative flight toggle (PLAYER_CONTROLLER.md §6.7). Flight is a mode the client holds: while it
// is on, every input frame carries the fly bit, and the shared controller flies the player. The
// server clears the bit for players its flight policy excludes; Welcome says whether this one may.

import { clampFlySpeedLevel } from './flightSpeed';

/** Two jump presses within this many ms toggle flight. */
export const DOUBLE_TAP_MS = 300;

export class FlightToggle {
  /** The server lets this player fly (Welcome's flight flag). */
  private mayFly = true;
  private on = false;
  private lastPress = -Infinity;
  private level = 0;
  /** Flight turned on or off. */
  onChange: ((flying: boolean) => void) | null = null;
  /** The flight speed level changed (predict/flightSpeed.ts). */
  onSpeedChange: ((level: number) => void) | null = null;

  /** The flight speed level sent with every input (0: normal). */
  get speedLevel(): number {
    return this.level;
  }

  set speedLevel(level: number) {
    const l = clampFlySpeedLevel(level);
    if (l === this.level) return;
    this.level = l;
    this.onSpeedChange?.(l);
  }

  get flying(): boolean {
    return this.mayFly && this.on;
  }

  get allowed(): boolean {
    return this.mayFly;
  }

  set allowed(v: boolean) {
    const was = this.flying;
    this.mayFly = v;
    if (this.flying !== was) this.onChange?.(this.flying);
  }

  set(on: boolean): void {
    const was = this.flying;
    this.on = on && this.mayFly;
    if (this.flying !== was) this.onChange?.(this.flying);
  }

  toggle(): void {
    this.set(!this.on);
  }

  /** A jump press (not a key repeat) at `nowMs`; the second of a quick pair toggles flight. */
  jumpPressed(nowMs: number): void {
    if (nowMs - this.lastPress <= DOUBLE_TAP_MS) {
      this.lastPress = -Infinity;
      this.toggle();
    } else {
      this.lastPress = nowMs;
    }
  }
}
