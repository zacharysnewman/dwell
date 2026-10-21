// The music (docs/AUDIO.md §6a): the track the data picks (shared/music/*.json), streamed by an
// <audio> element (never decoded whole) and, once the page's AudioContext runs, routed into the
// mixer's music bus. Browsers block sound until a user gesture: the player tries to start at once,
// and if `play()` is refused it waits for the first pointer, key or touch gesture and starts then,
// inside it (iOS Safari plays media and resumes a context only there). It fades in, loops, pauses
// while the tab is hidden and fades back in on return, is silent (paused) at volume 0, and picks up
// where the previous page left it (the main menu → the game). Plain TS: the main menu has it too.
import type { Volumes } from './mixer';
import type { MusicData, MusicWhen, TrackDef } from './musicTypes';

/** The dev store that turns the music off ("off"; the e2e suite and the bench set it). */
export const MUSIC_KEY = 'dwell.music';
/** Session store: the track and where it was when the previous page hid (per tab). */
export const MUSIC_POSITION_KEY = 'dwell.musicAt';
/** The gestures that may start it (as the engine's unlock). */
export const GESTURES = ['pointerdown', 'keydown', 'touchend', 'click'] as const;
/** The fade back in after the tab was hidden or the volume raised from 0 (s). */
export const RESUME_FADE_S = 1;
/** A position saved longer ago than this (s) is not picked up: the track starts over. */
export const POSITION_MAX_AGE_S = 30;
/** Fades step this often (ms). */
const FADE_STEP_MS = 50;

type Store = Pick<Storage, 'getItem' | 'setItem'>;

/** Whether music is on for this page: off for `?music=off` or the store's "off". */
export function musicEnabled(search: string, store: Pick<Storage, 'getItem'> | null): boolean {
  if (new URLSearchParams(search).get('music') === 'off') return false;
  let stored: string | null;
  try {
    stored = store?.getItem(MUSIC_KEY) ?? null;
  } catch {
    stored = null;
  }
  if (stored === null) return true;
  try {
    return JSON.parse(stored) !== 'off';
  } catch {
    return stored !== 'off';
  }
}

/**
 * The track to play: the first whose `when` is among the conditions holding now. Only "always" is
 * understood so far; a later condition (a biome, the night, a menu) joins `holding` where it is
 * known.
 */
export function chooseTrack(
  data: MusicData,
  holding: readonly MusicWhen[] = ['always'],
): TrackDef | null {
  return data.tracks.find((t) => holding.includes(t.when)) ?? null;
}

/** What the player uses of an HTMLAudioElement. */
export interface MusicElement {
  src: string;
  loop: boolean;
  volume: number;
  preload: string;
  currentTime: number;
  readonly duration: number;
  readonly paused: boolean;
  play(): Promise<void>;
  pause(): void;
  addEventListener(type: string, fn: () => void): void;
}

/** What the player uses of the audio engine (engine.ts). */
export interface MusicHost {
  /** Creates or resumes the AudioContext (in a gesture). */
  unlock(): void;
  readonly running: boolean;
  onRunning(fn: () => void): void;
  connectMusic(el: MusicElement): ((level: number) => void) | null;
}

export interface MusicEnv {
  doc: Pick<Document, 'addEventListener' | 'removeEventListener' | 'visibilityState'>;
  makeElement(): MusicElement;
  /** Monotonic seconds (fades). */
  now(): number;
  /** Wall-clock ms (the saved position's age). */
  wall(): number;
  session: Store | null;
  setInterval(fn: () => void, ms: number): unknown;
  clearInterval(id: unknown): void;
}

export type MusicState = 'idle' | 'blocked' | 'playing' | 'paused' | 'muted' | 'stopped';

export class MusicPlayer {
  private el: MusicElement | null = null;
  /** The routed gain's setter; null while the element plays straight out (no context yet). */
  private level: ((v: number) => void) | null = null;
  private volume = 0;
  private master = 1;
  private _state: MusicState = 'idle';
  private hidden = false;
  private started = false;
  private everPlayed = false;
  private armed = false;
  private pending = false;
  /** The fade: from `fadeFrom` to `fadeTo` over `fadeS` from `fadeStart`. */
  private fadeFrom = 0;
  private fadeTo = 0;
  private fadeStart = 0;
  private fadeS = 0;
  private timer: unknown = null;
  /** Successful `play()` calls (the debug hook; the tests count gestures' starts). */
  plays = 0;

  private readonly gesture = (): void => {
    this.attempt(true);
  };

  constructor(
    readonly track: TrackDef,
    private readonly url: string,
    private readonly host: MusicHost,
    private readonly env: MusicEnv,
  ) {}

  get state(): MusicState {
    return this._state;
  }

  /** Whether it plays through the mixer (else straight out of the element). */
  get routed(): boolean {
    return this.level !== null;
  }

  get time(): number {
    return this.el?.currentTime ?? 0;
  }

  /** The output level now, 0–1 (before the master volume when routed). */
  get output(): number {
    return this.volume * this.track.volume * this.fade();
  }

  setVolumes(v: Pick<Volumes, 'master' | 'music'>): void {
    const was = this.volume;
    this.volume = v.music;
    this.master = v.master;
    if (!this.started) return;
    if (this.volume === 0) {
      if (this._state === 'playing' || this._state === 'blocked') {
        this.el?.pause();
        this.stopTimer();
        this._state = 'muted';
      }
      return;
    }
    if (was === 0 && this._state === 'muted') this.attempt(false);
    else this.apply();
  }

  /** Starts the music now if the browser allows it, else on the first gesture. Idempotent. */
  start(): void {
    if (this.started) return;
    this.started = true;
    const el = this.env.makeElement();
    this.el = el;
    el.preload = 'auto';
    el.loop = this.track.loop;
    el.volume = 0;
    el.addEventListener('loadedmetadata', () => {
      this.restorePosition();
    });
    el.addEventListener('timeupdate', () => {
      if (!this.track.loop) this.apply();
    });
    el.addEventListener('ended', () => {
      this.stopTimer();
      this._state = 'stopped';
    });
    el.src = this.url;
    this.env.doc.addEventListener('visibilitychange', () => {
      this.visibility();
    });
    this.host.onRunning(() => {
      this.route();
    });
    if (this.volume === 0) {
      this._state = 'muted';
      return;
    }
    this.attempt(false);
  }

  /** Fades out over the track's `fadeOutS`, then pauses (a later condition's change of track). */
  stop(): void {
    if (!this.started || this._state === 'stopped') return;
    this.disarm();
    this._state = 'stopped';
    this.fadeToward(0, this.track.fadeOutS);
  }

  private attempt(inGesture: boolean): void {
    const el = this.el;
    if (!el || this.hidden || this.volume === 0 || this._state === 'stopped' || this.pending) {
      return;
    }
    if (inGesture) {
      // Inside the gesture: the context resumes, and the element is routed into it now (silent
      // until the context runs, never a burst at full level).
      this.host.unlock();
      this.route(true);
    }
    this.pending = true;
    let p: Promise<void>;
    try {
      p = el.play();
    } catch (e) {
      p = Promise.reject(e instanceof Error ? e : new Error(String(e)));
    }
    p.then(
      () => {
        this.pending = false;
        if (this.hidden || this.volume === 0 || this._state === 'stopped') {
          el.pause();
          if (this.volume === 0 && this._state !== 'stopped') this._state = 'muted';
          return;
        }
        this.plays++;
        this._state = 'playing';
        this.disarm();
        this.fadeToward(
          1,
          this.everPlayed ? Math.min(RESUME_FADE_S, this.track.fadeInS) : this.track.fadeInS,
          0,
        );
        this.everPlayed = true;
      },
      () => {
        this.pending = false;
        if (this._state === 'stopped') return;
        this._state = 'blocked';
        this.arm();
      },
    );
  }

  /** Waits for the first gesture (once: removed when a play succeeds). */
  private arm(): void {
    if (this.armed) return;
    this.armed = true;
    for (const t of GESTURES) {
      this.env.doc.addEventListener(t, this.gesture, { capture: true, passive: true });
    }
  }

  private disarm(): void {
    if (!this.armed) return;
    this.armed = false;
    for (const t of GESTURES) this.env.doc.removeEventListener(t, this.gesture, { capture: true });
  }

  /** Into the mixer, once: when the context runs (or, in a gesture, as it starts). */
  private route(force = false): void {
    if (this.level || !this.el || (!force && !this.host.running)) return;
    this.level = this.host.connectMusic(this.el);
    if (this.level) {
      this.el.volume = 1;
      this.apply();
    }
  }

  private visibility(): void {
    const hidden = this.env.doc.visibilityState === 'hidden';
    if (hidden === this.hidden) return;
    this.hidden = hidden;
    if (hidden) {
      this.savePosition();
      if (this._state === 'playing') {
        this.el?.pause();
        this.stopTimer();
        this._state = 'paused';
      }
    } else if (this._state === 'paused' || this._state === 'blocked') {
      this.attempt(false);
    }
  }

  /** The fade's value now (0–1), times the tail's fade-out of a track that does not loop. */
  private fade(): number {
    const t = this.fadeS > 0 ? Math.min(1, (this.env.now() - this.fadeStart) / this.fadeS) : 1;
    let f = this.fadeFrom + (this.fadeTo - this.fadeFrom) * t;
    const el = this.el;
    if (el && !this.track.loop && Number.isFinite(el.duration) && this.track.fadeOutS > 0) {
      f *= Math.min(1, Math.max(0, (el.duration - el.currentTime) / this.track.fadeOutS));
    }
    return f;
  }

  private fadeToward(to: number, seconds: number, from = this.fade()): void {
    this.fadeFrom = from;
    this.fadeTo = to;
    this.fadeStart = this.env.now();
    this.fadeS = seconds;
    this.apply();
    this.stopTimer();
    if (seconds <= 0) {
      if (to === 0) this.el?.pause();
      return;
    }
    this.timer = this.env.setInterval(() => {
      this.apply();
      if (this.env.now() - this.fadeStart >= this.fadeS) {
        this.stopTimer();
        if (this.fadeTo === 0) this.el?.pause();
      }
    }, FADE_STEP_MS);
  }

  private stopTimer(): void {
    if (this.timer !== null) this.env.clearInterval(this.timer);
    this.timer = null;
  }

  private apply(): void {
    const el = this.el;
    if (!el) return;
    const out = this.output;
    if (this.level) this.level(out);
    else el.volume = Math.min(1, Math.max(0, out * this.master));
  }

  private savePosition(): void {
    const el = this.el;
    if (!el || !this.everPlayed) return;
    try {
      this.env.session?.setItem(
        MUSIC_POSITION_KEY,
        JSON.stringify({ id: this.track.id, t: el.currentTime, at: this.env.wall() }),
      );
    } catch {
      // Not kept: the next page starts the track over.
    }
  }

  private restorePosition(): void {
    const el = this.el;
    if (!el || el.currentTime > 0) return;
    let saved: unknown;
    try {
      saved = JSON.parse(this.env.session?.getItem(MUSIC_POSITION_KEY) ?? 'null');
    } catch {
      return;
    }
    if (typeof saved !== 'object' || saved === null) return;
    const s = saved as { id?: unknown; t?: unknown; at?: unknown };
    if (s.id !== this.track.id || typeof s.t !== 'number' || typeof s.at !== 'number') return;
    const age = (this.env.wall() - s.at) / 1000;
    if (!(age >= 0 && age <= POSITION_MAX_AGE_S)) return;
    let t = s.t + age;
    const d = el.duration;
    if (Number.isFinite(d) && d > 0) {
      if (this.track.loop) t %= d;
      else if (t >= d) return;
    }
    el.currentTime = t;
  }
}

/** The browser's environment for the player. */
export function browserMusicEnv(doc: Document = document): MusicEnv {
  let session: Store | null;
  try {
    session = typeof sessionStorage === 'undefined' ? null : sessionStorage;
  } catch {
    session = null;
  }
  return {
    doc,
    makeElement: () => new Audio(),
    now: () => performance.now() / 1000,
    wall: () => Date.now(),
    session,
    setInterval: (fn, ms) => window.setInterval(fn, ms),
    clearInterval: (id) => {
      window.clearInterval(id as number);
    },
  };
}
