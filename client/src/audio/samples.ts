// Recorded samples (docs/AUDIO.md §2b): a sound's `samples` are decoded once, on the first gesture,
// and kept; each play draws one variation (never the previous one, as the layers' alternatives),
// with a pitch and level drawn within the data's ranges. A sound whose samples are not loaded (still
// loading, failed, or the data says `play: "procedural"`) plays its procedural layers instead.
// Pure apart from the loader, which the engine gives the AudioContext's decode.
import type { SoundData, SoundDef } from './types';
import type { Rng } from './variation';

/** A decoded sample: what the engine needs of an AudioBuffer. */
export interface Decoded {
  duration: number;
}

/** One play of a sample: the file, its buffer, its playback rate and its level. */
export interface SamplePlay<B extends Decoded> {
  file: string;
  buffer: B;
  /** Playback rate (pitch): 2^(semitones / 12). */
  rate: number;
  gain: number;
}

/** Whether a sound plays its samples (it has them and the data does not choose its layers). */
export function usesSamples(sound: SoundDef): boolean {
  return !!sound.samples && sound.play !== 'procedural';
}

export class SampleBank<B extends Decoded> {
  private readonly buffers = new Map<string, B>();
  private readonly last = new Map<string, string>();
  private started = false;

  constructor(private readonly rng: Rng) {}

  /** Loads every sample the data plays (once); a file that fails stays procedural. */
  load(data: SoundData, fetchDecode: (file: string) => Promise<B>): Promise<void> {
    if (this.started) return Promise.resolve();
    this.started = true;
    const files = new Set<string>();
    for (const s of Object.values(data.sounds))
      if (usesSamples(s)) for (const f of s.samples?.files ?? []) files.add(f);
    for (const b of Object.values(data.ambience.beds)) for (const f of b.files) files.add(f);
    return Promise.all(
      [...files].map((f) =>
        fetchDecode(f).then(
          (b) => {
            this.buffers.set(f, b);
          },
          () => undefined,
        ),
      ),
    ).then(() => undefined);
  }

  /** Puts a decoded sample in the bank (tests; the loader). */
  set(file: string, buffer: B): void {
    this.buffers.set(file, buffer);
  }

  /** A decoded file (an ambience bed's loop), if loaded. */
  get(file: string): B | undefined {
    return this.buffers.get(file);
  }

  get loaded(): number {
    return this.buffers.size;
  }

  /** The sample to play for sound `id`, or null: play its procedural layers. */
  pick(id: string, sound: SoundDef): SamplePlay<B> | null {
    if (!usesSamples(sound) || !sound.samples) return null;
    const ready = sound.samples.files.filter((f) => this.buffers.has(f));
    if (ready.length === 0) return null;
    const prev = this.last.get(id);
    const choices = ready.length > 1 ? ready.filter((f) => f !== prev) : ready;
    const file = choices[Math.floor(this.rng() * choices.length)] ?? ready[0];
    const buffer = file === undefined ? undefined : this.buffers.get(file);
    if (file === undefined || !buffer) return null;
    this.last.set(id, file);
    const semis = sound.samples.pitch ?? 0;
    const rate = 2 ** (((this.rng() * 2 - 1) * semis) / 12);
    return { file, buffer, rate, gain: sound.samples.gain ?? 1 };
  }
}
