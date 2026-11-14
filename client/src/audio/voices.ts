// Per-sound voice limits (docs/AUDIO.md §6, BUGS.md #103): a sound plays at most `voices` times at
// once and not again within `minGapS` of its last start. A play over the limit is dropped: a voice
// already sounding is never cut off or restarted (a restarted noise burst clicks). Pure, so the
// engine's rule is tested without an AudioContext.
import type { SoundDef } from './types';

/** A sound's limit when its data gives none. */
export const DEFAULT_SOUND_VOICES = 4;

export class VoiceLimits {
  /** By sound id: the end times of its plays still sounding. */
  private readonly ends = new Map<string, number[]>();
  /** By sound id: its last play's start. */
  private readonly last = new Map<string, number>();

  /** Whether a play of `id` starting at `now` (s) may sound. */
  admit(id: string, def: Pick<SoundDef, 'voices' | 'minGapS'>, now: number): boolean {
    const ends = (this.ends.get(id) ?? []).filter((e) => e > now);
    this.ends.set(id, ends);
    if (ends.length >= (def.voices ?? DEFAULT_SOUND_VOICES)) return false;
    const last = this.last.get(id);
    if (last !== undefined && now - last < (def.minGapS ?? 0)) return false;
    return true;
  }

  /** Records an admitted play of `id` from `now` until `end` (s). */
  started(id: string, now: number, end: number): void {
    const ends = this.ends.get(id) ?? [];
    ends.push(end);
    this.ends.set(id, ends);
    this.last.set(id, now);
  }

  /** Plays of `id` still sounding at `now` (tests, the debug hook). */
  playing(id: string, now: number): number {
    return (this.ends.get(id) ?? []).filter((e) => e > now).length;
  }
}
