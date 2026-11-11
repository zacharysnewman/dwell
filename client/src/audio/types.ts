// The sound data's shape (docs/AUDIO.md §2): what shared/sounds/gen.mjs emits into sounds.gen.ts
// after validating shared/sounds/*.json (shared/sounds/validate.mjs holds the rules).

/** `crackle`: sparse decaying pops (a fire's crackle). */
export type NoiseColor = 'white' | 'pink' | 'brown' | 'crackle';
export type Wave = 'sine' | 'triangle' | 'square' | 'sawtooth';
export type FilterType = 'lowpass' | 'highpass' | 'bandpass' | 'peaking' | 'notch';

export interface FilterDef {
  type: FilterType;
  /** Hz. */
  freq: number;
  /** Hz at the end of the envelope (an exponential sweep). */
  to?: number;
  q?: number;
  /** dB (peaking). */
  gain?: number;
}

export interface EnvDef {
  /** Seconds to the peak (linear). */
  attack: number;
  /** Seconds at the peak. */
  hold?: number;
  /** Seconds from the peak to −60 dB (exponential). */
  decay: number;
}

export interface GrainsDef {
  /** How many bursts, drawn per play. */
  count: [number, number];
  /** Seconds over which the bursts fall. */
  spread: number;
  /** Each burst's gain, drawn. */
  gain: [number, number];
  /** ± semitones per burst. */
  pitch?: number;
}

/** A layer's fields an alternative may override (all but `source` and `alts`). */
export interface LayerFields {
  color?: NoiseColor;
  wave?: Wave;
  freq?: number;
  to?: number;
  /** Modal: [frequency ratio, gain, decay seconds] per mode. */
  partials?: [number, number, number][];
  filters?: FilterDef[];
  env?: EnvDef;
  gain?: number;
  at?: number;
  pan?: number;
  chance?: number;
  grains?: GrainsDef;
  /** Loop sounds: the layer's level wavers at `rate` Hz by ± `depth` / 2 (a scrape's grit). */
  tremolo?: { rate: number; depth: number };
}

export interface LayerDef extends LayerFields {
  source: 'noise' | 'osc' | 'modal';
  gain: number;
  /** Round-robin alternatives, merged over the layer. */
  alts?: LayerFields[];
}

export interface VariationDef {
  /** ± semitones. */
  pitch?: number;
  /** ± dB. */
  gain?: number;
  /** ± fraction of every filter frequency. */
  cutoff?: number;
  /** ± fraction of every envelope time. */
  time?: number;
}

export interface IntensityDef {
  gain?: [number, number];
  cutoff?: [number, number];
  time?: [number, number];
  /** Loop sounds: a factor on every source's rate (and tremolo), as the intensity moves. */
  pitch?: [number, number];
}

export interface SoundDef {
  gain: number;
  /** Send to the room reverb, 0–1. */
  reverb?: number;
  variation: VariationDef | 'none';
  intensity?: IntensityDef;
  /** Sounds until stopped (a slide's scrape): sustained layers whose levels follow intensity. */
  loop?: boolean;
  /** The procedural sound: played when it has no samples, they are not loaded, or `play` says so. */
  layers: LayerDef[];
  /** Recorded variations (AUDIO.md §2b): one drawn per play, never the previous one. */
  samples?: SamplesDef;
  /** With samples: which to play (default `samples`). */
  play?: 'samples' | 'procedural';
}

export interface SamplesDef {
  /** Published names under the site's `media/` (`stone-step-1-<hash>.m4a`). */
  files: string[];
  /** Level (default 1), times the play's gain. */
  gain?: number;
  /** ± semitones per play. */
  pitch?: number;
  /**
   * The share sent to the room (default 0: dry). Recordings carry their own room in them, so the
   * sound's `reverb` (for its procedural layers) would add a second, echoey room on top (owner,
   * 2026-11-10: "they've got some reverb-like feel").
   */
  reverb?: number;
}

export type SetEvent =
  | 'step'
  | 'land'
  | 'break'
  | 'place'
  | 'hit'
  | 'enter'
  | 'exit'
  | 'swim'
  | 'slide'
  | 'slide_stop'
  | 'burn'
  | 'explode'
  | 'impact'
  | 'brush';
/** An event's part in a continuous sound: start the mover's voice, update it, stop it. */
export type LoopPart = 'start' | 'update' | 'stop';
export type Gait = 'walk' | 'run' | 'crouch' | 'climb' | 'swim';
export type UiKind =
  'hover' | 'click' | 'back' | 'tab' | 'toggle_on' | 'toggle_off' | 'slider' | 'error' | 'success';

export interface EventDef {
  name: string;
  /** The block set's sound for this event. */
  set?: SetEvent;
  /** A sound played too (or instead). */
  sound?: string;
  speed?: [number, number];
  loop?: LoopPart;
  /** false: a set without this event's sound plays nothing (not the fallback set's). */
  fallback?: false;
  /** A creature's event (AUDIO.md §2d): the event's `block` is the model's index in `creatures`. */
  creature?: 'step' | 'startle' | 'call';
}

/** The ambient sounds' tunables (AUDIO.md §2), read by the Rust game. */
export interface AmbientDef {
  fire: { radiusM: number; max: number };
  /** Waterfalls (docs/WATER_SURFACE_GRID.md §2e): within `radiusM`, the nearest `max`, full at `fullM3s`. */
  falls: { radiusM: number; max: number; fullM3s: number };
}

/** A bed's condition (AUDIO.md §2c): 0 at `from`, 1 at `full`, at least `floor`. */
export interface BedCondition {
  input: string;
  from: number;
  full: number;
  floor?: number;
}

/** A looping ambience bed: its files (published names), level and conditions. */
export interface BedDef {
  files: string[];
  gain: number;
  when: BedCondition[];
}

/** The ambience (AUDIO.md §2c). */
export interface AmbienceDef {
  /** The Ambience volume's default. */
  volume: number;
  /** The beds' glide time constant (s). */
  fadeS: number;
  /** No bed is louder than this (before the Ambience volume). */
  cap: number;
  probe: { radiusM: number; trees: string[] };
  beds: Record<string, BedDef>;
}

export type ControlSounds = string[] | { silent: string };

export interface SoundData {
  /** The set used by blocks without a `sound`, and for events a set lacks. */
  fallback: string;
  gaits: Record<string, { stride: number; gain: number }>;
  /** Gaits in the Rust game's numbering. */
  gaitOrder: string[];
  /** In the Rust game's numbering (SoundKind). */
  events: EventDef[];
  sets: Record<string, Partial<Record<SetEvent, string>>>;
  actions: Record<string, string>;
  ui: Record<string, string>;
  controls: Record<string, ControlSounds>;
  ambient: AmbientDef;
  ambience: AmbienceDef;
  /** Each creature model's sounds (AUDIO.md §2d), in the Rust game's order. */
  creatures: {
    radiusM: number;
    models: Record<
      string,
      {
        stride: number;
        step?: string;
        startle?: string;
        call?: string;
        callEveryS?: [number, number];
      }
    >;
  };
  sounds: Record<string, SoundDef>;
  /** Every sample file shipped: its published name and its file in shared/sounds/samples. */
  sampleFiles: { file: string; source: string }[];
  /** Their total size (bytes; the budget is validated). */
  sampleBytes: number;
}
