// The ambience (docs/AUDIO.md §2c): looping beds — wind, the sea, a river, a stream, birds, leaves —
// under the Ambience volume, each at its data's gain times every condition's ramp over the
// listener's surroundings (the Rust game's probe, `ambience()`: 10 numbers in INPUTS order), never
// above the data's cap, gliding to each new level over `fadeS`. Pure apart from the output the
// engine gives it (loops on the ambience bus), so the levels and the start/stop/crossfade are
// tested without an AudioContext.
import type { AmbienceDef, BedCondition, BedDef } from './types';

/** The surroundings, in the order the Rust probe hands them (crates/dwell-game ambience.rs). */
export const INPUTS = [
  'altitude',
  'height',
  'open',
  'waterShare',
  'waterM',
  'flow',
  'trees',
  'day',
  'fireM',
  'underwater',
] as const;
export type Input = (typeof INPUTS)[number];
export type Surroundings = Record<Input, number>;

/** Below this a bed is silent: it is not started, and one playing is stopped. */
export const SILENT = 0.003;

/** The surroundings from the probe's numbers; null when there are none yet (not playing). */
export function decodeSurroundings(v: ArrayLike<number>): Surroundings | null {
  if (v.length < INPUTS.length) return null;
  const out = {} as Surroundings;
  INPUTS.forEach((k, i) => {
    out[k] = v[i] ?? 0;
  });
  return out;
}

/** A condition's share: 0 at `from`, 1 at `full` (either way round), at least `floor`. */
export function ramp(c: BedCondition, x: number): number {
  const t = Math.min(1, Math.max(0, (x - c.from) / (c.full - c.from)));
  return Math.max(c.floor ?? 0, t);
}

/** A bed's level in these surroundings (before the Ambience volume): capped. */
export function bedGain(bed: BedDef, s: Surroundings, cap: number): number {
  let g = bed.gain;
  for (const c of bed.when) g *= ramp(c, s[c.input as Input]);
  return Math.min(cap, g);
}

/** Every bed's level. */
export function bedGains(def: AmbienceDef, s: Surroundings): Record<string, number> {
  const out: Record<string, number> = {};
  for (const [id, bed] of Object.entries(def.beds)) out[id] = bedGain(bed, s, def.cap);
  return out;
}

/** What the player drives: a looping bed's voice on the ambience bus. */
export interface BedOutput {
  /** Starts the bed's loop silent; false if it cannot play yet (no context, not decoded). */
  start(id: string, files: string[]): boolean;
  /** Glides its level toward `gain` with time constant `fadeS`. */
  level(id: string, gain: number, fadeS: number): void;
  /** Fades it out over `fadeS` and frees it. */
  stop(id: string, fadeS: number): void;
}

export class AmbiencePlayer {
  private readonly playing = new Set<string>();

  constructor(
    private readonly def: AmbienceDef,
    private readonly out: BedOutput,
  ) {}

  /** Moves every bed toward its level in `s` (null: none yet, everything fades out). */
  update(s: Surroundings | null): void {
    const gains = s ? bedGains(this.def, s) : {};
    for (const [id, bed] of Object.entries(this.def.beds)) {
      const g = gains[id] ?? 0;
      const on = this.playing.has(id);
      if (g < SILENT) {
        if (on) {
          this.out.stop(id, this.def.fadeS);
          this.playing.delete(id);
        }
        continue;
      }
      if (!on) {
        if (!this.out.start(id, bed.files)) continue;
        this.playing.add(id);
      }
      this.out.level(id, g, this.def.fadeS);
    }
  }

  /** The beds playing (the debug hook). */
  get beds(): string[] {
    return [...this.playing];
  }
}
