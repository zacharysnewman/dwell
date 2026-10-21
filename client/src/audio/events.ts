// The game's sound events (docs/AUDIO.md §5): decoding the numbers the Rust client hands over
// (`takeSounds`: 7 per event), and mapping each to the sounds it plays — the block's sound set
// (block state → block → `sound`, else the data's fallback) for set events, the event's own sound,
// the gait's loudness for footsteps — the local player's steps alternating left and right, and
// continuous sounds (a slide's scrape) on each mover's voice: started, moved and stopped.
import { BLOCK_DEFS, STATE_DEFS, type BlockDef, type StateDef } from '../world/blocks.gen';
import type { SoundData } from './types';

export const FLOATS_PER_EVENT = 7;

export interface SoundEvent {
  kind: number;
  /** Block state id (0: none). */
  block: number;
  intensity: number;
  /** World position, for spatial events. */
  at: [number, number, number] | null;
  /** Index into the data's gaitOrder, for footsteps. */
  gait: number | null;
  /** The mover's voice for continuous sounds (0: the local player; others: their id). */
  voice: number | null;
}

export function decodeSounds(v: ArrayLike<number>): SoundEvent[] {
  const out: SoundEvent[] = [];
  for (let i = 0; i + FLOATS_PER_EVENT <= v.length; i += FLOATS_PER_EVENT) {
    const flags = v[i + 6] ?? 0;
    const gait = (flags >> 1) & 7;
    const voice = Math.floor(flags / 16);
    out.push({
      kind: v[i] ?? 0,
      block: v[i + 1] ?? 0,
      intensity: v[i + 2] ?? 1,
      at: flags & 1 ? [v[i + 3] ?? 0, v[i + 4] ?? 0, v[i + 5] ?? 0] : null,
      gait: gait ? gait - 1 : null,
      voice: voice ? voice - 1 : null,
    });
  }
  return out;
}

export interface Registry {
  states: readonly StateDef[];
  blocks: readonly BlockDef[];
}

const REGISTRY: Registry = { states: STATE_DEFS, blocks: BLOCK_DEFS };

/** A block state's sound set id: its block's `sound`, else the fallback. */
export function setOfState(state: number, data: SoundData, registry: Registry = REGISTRY): string {
  const s = registry.states[state];
  const block = s ? registry.blocks[s.block] : undefined;
  const set = block?.sound;
  return set && set in data.sets ? set : data.fallback;
}

export interface ResolvedPlay {
  sound: string;
  gain: number;
}

/** The sounds one event plays. */
export function soundsFor(
  e: SoundEvent,
  data: SoundData,
  registry: Registry = REGISTRY,
): ResolvedPlay[] {
  const def = data.events[e.kind];
  if (!def) return [];
  const gaitName = e.gait !== null ? data.gaitOrder[e.gait] : undefined;
  const gain = gaitName ? (data.gaits[gaitName]?.gain ?? 1) : 1;
  const out: ResolvedPlay[] = [];
  if (def.set) {
    const set = data.sets[setOfState(e.block, data, registry)];
    const sound =
      set?.[def.set] ?? (def.fallback === false ? undefined : data.sets[data.fallback]?.[def.set]);
    if (sound) out.push({ sound, gain });
  }
  if (def.sound) out.push({ sound: def.sound, gain: 1 });
  return out;
}

/** Stereo offset of the local player's footsteps, alternating left and right. */
export const STEP_PAN = 0.12;

export type Play = (
  sound: string,
  options: { intensity: number; at: [number, number, number] | null; pan?: number; gain: number },
) => void;

/** Continuous sounds by voice (the engine's loop voices). */
export interface Loops {
  start(
    voice: string,
    sound: string,
    options: { intensity: number; at: [number, number, number] | null; gain: number },
  ): void;
  /** False if the voice is not playing (it never started: a suspended or hidden context). */
  update(voice: string, intensity: number, at: [number, number, number] | null): boolean;
  stop(voice: string): void;
}

/** A loop voice's start, kept to restart it when an update finds it not playing. */
interface Started {
  sound: string;
  gain: number;
}

/** The engine's key for an event's voice. */
export const voiceKey = (voice: number | null): string => `mover:${String(voice ?? 0)}`;

/**
 * Plays decoded events; keeps the left/right alternation of the local player's steps. Loop
 * events (`loop` in the data): `start` starts the voice on the event's set sound (a new surface
 * restarts it), `update` moves its intensity and position, `stop` fades it and plays the event's
 * one-shot (the set's slide stop).
 */
export class EventPlayer {
  private foot = 1;
  /** Loop voices started and not stopped: the long ones (a torch, the wind) outlive a context
   * that could not start them (hidden, still resuming), so an update restarts them. */
  private readonly started = new Map<string, Started>();

  constructor(
    private readonly data: SoundData,
    private readonly play: Play,
    private readonly loops: Loops | null = null,
    private readonly registry: Registry = REGISTRY,
  ) {}

  playAll(events: SoundEvent[]): void {
    for (const e of events) {
      const def = this.data.events[e.kind];
      if (def?.loop) {
        const voice = voiceKey(e.voice);
        if (def.loop === 'update') {
          const s = this.started.get(voice);
          if (this.loops && !this.loops.update(voice, e.intensity, e.at) && s)
            this.loops.start(voice, s.sound, { intensity: e.intensity, at: e.at, gain: s.gain });
          continue;
        }
        if (def.loop === 'start') {
          const first = soundsFor(e, this.data, this.registry)[0];
          if (first) {
            this.started.set(voice, { sound: first.sound, gain: first.gain });
            this.loops?.start(voice, first.sound, {
              intensity: e.intensity,
              at: e.at,
              gain: first.gain,
            });
          } else this.started.delete(voice);
          continue;
        }
        this.started.delete(voice);
        this.loops?.stop(voice);
      }
      let pan: number | undefined;
      if (e.gait !== null && e.at === null) {
        this.foot = -this.foot;
        pan = this.foot * STEP_PAN;
      }
      for (const p of soundsFor(e, this.data, this.registry)) {
        this.play(p.sound, {
          intensity: e.intensity,
          at: e.at,
          gain: p.gain,
          ...(pan !== undefined ? { pan } : {}),
        });
      }
    }
  }
}
