// The audio engine (docs/AUDIO.md §6): one AudioContext per page, created and resumed on the
// first user gesture (browser autoplay rules; iOS Safari's "interrupted" state), the mixer,
// volumes, muting while the tab is hidden, plays on the effects or UI bus, spatial plays for other
// players through a PannerNode, and the listener at the camera. Plain TS: the main menu (the
// launcher, no WASM) uses it too.
import { buildMixer, DEFAULT_VOLUMES, soundInput, type Mixer, type Volumes } from './mixer';
import { SOUND_DATA } from './sounds.gen';
import { makeNoise, playSound, startLoop, type LoopVoice, type NoiseBank } from './synth';
import { Variation } from './variation';
import type { SoundData } from './types';

export type Vec3 = [number, number, number];

export interface PlayRequest {
  bus: 'effects' | 'ui';
  /** 0–1 (a landing's impact). */
  intensity?: number;
  /** World position for a spatial play; none: at the listener. */
  at?: Vec3 | null;
  /** Stereo position for a non-spatial play (footsteps left and right). */
  pan?: number;
  gain?: number;
}

/** Spatial plays farther than this are not played (m). */
export const MAX_DISTANCE = 48;
/** At most this many plays sound at once; more are dropped. */
const MAX_VOICES = 24;

export class AudioEngine {
  private ctx: AudioContext | null = null;
  private mixer: Mixer | null = null;
  private noise: NoiseBank | null = null;
  private readonly variation = new Variation((Date.now() & 0xffff) + 1);
  private volumes: Volumes = DEFAULT_VOLUMES;
  private readonly ends: number[] = [];
  /** Loop voices by mover (a slide's scrape). */
  private readonly loops = new Map<string, { voice: LoopVoice; panner: PannerNode | undefined }>();
  private listener: Vec3 = [0, 0, 0];
  private installed = false;
  /** The latest plays' ids, newest last (the `window.__dwell.sounds()` debug hook). */
  readonly recent: string[] = [];
  private hidden = false;

  constructor(
    private readonly data: SoundData = SOUND_DATA,
    private readonly mobile = false,
  ) {}

  /** Listens for the first gestures (to start audio) and for the tab hiding. Idempotent. */
  install(doc: Document = document): void {
    if (this.installed) return;
    this.installed = true;
    const unlock = (): void => {
      this.unlock();
    };
    for (const type of ['pointerdown', 'keydown', 'touchend', 'click'])
      doc.addEventListener(type, unlock, { capture: true, passive: true });
    doc.addEventListener('visibilitychange', () => {
      this.hidden = doc.visibilityState === 'hidden';
      if (!this.ctx) return;
      if (this.hidden) void this.ctx.suspend().catch(() => undefined);
      else void this.ctx.resume().catch(() => undefined);
    });
  }

  /** Creates or resumes the context; must run in a user gesture the first time. */
  unlock(): void {
    try {
      if (!this.ctx) {
        // Older Safari names it webkitAudioContext.
        const w = window as unknown as {
          AudioContext?: typeof AudioContext;
          webkitAudioContext?: typeof AudioContext;
        };
        const Ctx = w.AudioContext ?? w.webkitAudioContext;
        if (!Ctx) return;
        const ctx = new Ctx({ latencyHint: 'interactive' });
        this.ctx = ctx;
        this.mixer = buildMixer(ctx, this.volumes);
        this.noise = makeNoise(ctx);
        // iOS starts a context only from a sound played in the gesture: a silent one.
        const silent = ctx.createBufferSource();
        silent.buffer = ctx.createBuffer(1, 1, ctx.sampleRate);
        silent.connect(ctx.destination);
        silent.start();
      }
      // 'interrupted' (iOS, after a call or another app's audio) resumes the same way.
      if (this.ctx.state !== 'running' && !this.hidden)
        void this.ctx.resume().catch(() => undefined);
    } catch {
      // No audio on this page: everything else still works.
    }
  }

  get running(): boolean {
    return this.ctx?.state === 'running';
  }

  setVolumes(v: Volumes): void {
    this.volumes = v;
    this.mixer?.setVolumes(v);
  }

  /** The camera: position, forward and up (unit vectors). */
  setListener(position: Vec3, forward: Vec3, up: Vec3): void {
    this.listener = position;
    const l = this.ctx?.listener;
    if (!l) return;
    if ('positionX' in l) {
      l.positionX.value = position[0];
      l.positionY.value = position[1];
      l.positionZ.value = position[2];
      l.forwardX.value = forward[0];
      l.forwardY.value = forward[1];
      l.forwardZ.value = forward[2];
      l.upX.value = up[0];
      l.upY.value = up[1];
      l.upZ.value = up[2];
    } else {
      // Older Safari.
      const old = l as unknown as {
        setPosition(x: number, y: number, z: number): void;
        setOrientation(...v: number[]): void;
      };
      old.setPosition(...position);
      old.setOrientation(...forward, ...up);
    }
  }

  /** Plays a sound by id (`dwell:stone_step`); false if it was not played. */
  play(id: string, req: PlayRequest): boolean {
    const { ctx, mixer, noise } = this;
    const sound = this.data.sounds[id];
    // A context still resuming (the first gesture) plays once it runs; a hidden tab plays nothing.
    if (!ctx || !mixer || !noise || !sound || this.hidden || ctx.state === 'closed') return false;
    const at = req.at ?? null;
    if (at && distance(at, this.listener) > MAX_DISTANCE) return false;
    const now = ctx.currentTime;
    while (this.ends.length && (this.ends[0] ?? 0) <= now) this.ends.shift();
    if (this.ends.length >= MAX_VOICES) return false;
    let panner: PannerNode | undefined;
    if (at) {
      panner = this.panner(ctx);
      setPosition(panner, at);
    }
    const input = soundInput(ctx, mixer, req.bus, sound.reverb ?? 0, panner);
    const params = this.variation.draw(id, sound, req.intensity ?? 1);
    const end = playSound(ctx, noise, input, sound, params, now + 0.005, {
      ...(req.pan !== undefined && !at ? { pan: req.pan } : {}),
      ...(req.gain !== undefined ? { gain: req.gain } : {}),
    });
    this.ends.push(end);
    this.recent.push(id);
    if (this.recent.length > 64) this.recent.shift();
    this.ends.sort((a, b) => a - b);
    return true;
  }

  /**
   * Starts a loop sound on `voice` (a mover's slide), replacing what that voice played: a new
   * surface crossfades. Spatial when `req.at` is given; the panner follows `updateLoop`.
   */
  startLoop(voice: string, id: string, req: PlayRequest): boolean {
    const { ctx, mixer, noise } = this;
    const sound = this.data.sounds[id];
    if (!ctx || !mixer || !noise || !sound?.loop || this.hidden || ctx.state === 'closed') {
      return false;
    }
    this.stopLoop(voice);
    const at = req.at ?? null;
    let panner: PannerNode | undefined;
    if (at) {
      panner = this.panner(ctx);
      setPosition(panner, at);
    }
    const input = soundInput(ctx, mixer, req.bus, sound.reverb ?? 0, panner);
    const params = this.variation.draw(id, sound, req.intensity ?? 1);
    const v = startLoop(
      ctx,
      noise,
      input,
      sound,
      params,
      ctx.currentTime + 0.005,
      req.intensity ?? 1,
      {
        ...(req.gain !== undefined ? { gain: req.gain } : {}),
      },
    );
    this.loops.set(voice, { voice: v, panner });
    this.recent.push(id);
    if (this.recent.length > 64) this.recent.shift();
    return true;
  }

  /** Moves a loop voice's intensity (and position); false if it is not playing. */
  updateLoop(voice: string, intensity: number, at: Vec3 | null = null): boolean {
    const l = this.loops.get(voice);
    if (!l || !this.ctx) return false;
    l.voice.set(intensity, this.ctx.currentTime);
    if (l.panner && at) setPosition(l.panner, at);
    return true;
  }

  /** Fades a loop voice out. */
  stopLoop(voice: string): void {
    const l = this.loops.get(voice);
    if (!l || !this.ctx) return;
    l.voice.stop(this.ctx.currentTime);
    this.loops.delete(voice);
  }

  /** The loop voices playing (the debug hook). */
  get loopVoices(): string[] {
    return [...this.loops.keys()];
  }

  private panner(ctx: AudioContext): PannerNode {
    const panner = ctx.createPanner();
    panner.panningModel = this.mobile ? 'equalpower' : 'HRTF';
    panner.distanceModel = 'inverse';
    panner.refDistance = 2;
    panner.maxDistance = MAX_DISTANCE;
    panner.rolloffFactor = 1;
    return panner;
  }

  /** A UI sound of the menu vocabulary (`click`, `back`, …). */
  ui(kind: string): boolean {
    const id = this.data.ui[kind];
    return id ? this.play(id, { bus: 'ui' }) : false;
  }

  /** A game action's sound (`hotbar`, `fly_on`, …). */
  action(name: string): boolean {
    const id = this.data.actions[name];
    return id ? this.play(id, { bus: 'ui' }) : false;
  }
}

function setPosition(p: PannerNode, at: Vec3): void {
  p.positionX.value = at[0];
  p.positionY.value = at[1];
  p.positionZ.value = at[2];
}

function distance(a: Vec3, b: Vec3): number {
  return Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
}

let shared: AudioEngine | null = null;

/** The page's engine (created on first use; audio starts on the first gesture). */
export function audio(): AudioEngine {
  if (!shared) {
    const mobile =
      typeof navigator !== 'undefined' && /Android|iPhone|iPad|iPod/i.test(navigator.userAgent);
    shared = new AudioEngine(SOUND_DATA, mobile);
  }
  return shared;
}
