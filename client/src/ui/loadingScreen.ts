// The loading screen on joining a world and on respawning (ARCHITECTURE.md §6.3): the Rust client
// decides when it shows and what it reads (`Client.hud().loading`: the near ring's chunks drawn out
// of those needed, or null once open, from shared/look/loading.json's radius and timeout); this
// draws it over the view. It is the first thing the game page shows: up before the client starts
// (no HUD yet, so `loading` is undefined) and the HUD hidden under it until it opens. When it opens
// it turns white and fades out (`LOADING_FADE`, shared/look/loading.json's fade), uncovering the
// view and the HUD together: quick on a CPU adapter, and none (or quick) for prefers-reduced-motion.
// Opted out for testing with `?loading=off` or the `dwell.loading` store set to "off" (the e2e
// suite's storage does), it never shows.
import type { KeyValueStore } from '../local/worldIndex';
import { LOADING_FADE } from './loading.gen';

/** The dev store that turns the screen off ("off"); anything else, or nothing, leaves it on. */
export const LOADING_KEY = 'dwell.loading';

/** `Client.hud().loading`: null once open (undefined before the game starts). */
export interface LoadingHud {
  text: string;
  ready: number;
  needed: number;
}

/** Whether the loading screen is on for this page: off for `?loading=off` or the store's "off". */
export function loadingScreenEnabled(search: string, store: KeyValueStore | null): boolean {
  if (new URLSearchParams(search).get('loading') === 'off') return false;
  let stored: string | null;
  try {
    stored = store?.getItem(LOADING_KEY) ?? null;
  } catch {
    stored = null; // storage blocked: the default
  }
  if (stored === null) return true;
  try {
    return JSON.parse(stored) !== 'off';
  } catch {
    return stored !== 'off';
  }
}

/** The bar's fill, 0–1 (an empty ring is full). */
export function loadingFraction(h: LoadingHud): number {
  return h.needed > 0 ? Math.min(1, Math.max(0, h.ready / h.needed)) : 1;
}

/** The fade's shape: `LOADING_FADE`'s fields. */
export interface FadeTunables {
  holdS: number;
  durationS: number;
  easePower: number;
  cpuDurationS: number;
  reducedMotionDurationS: number;
}

/** How long the fade lasts here (s): quick on a CPU adapter, and for reduced motion. */
export function fadeDurationS(f: FadeTunables, cpu: boolean, reducedMotion: boolean): number {
  let d = f.durationS;
  if (cpu) d = Math.min(d, f.cpuDurationS);
  if (reducedMotion) d = Math.min(d, f.reducedMotionDurationS);
  return d;
}

/**
 * The white cover's opacity `t` seconds after the screen opened: 1 for the hold, then an ease-out
 * to 0 over `durationS` ((1 − x)^easePower: the view comes up quickly, then settles). No fade
 * (durationS 0) has no hold either.
 */
export function fadeOpacity(t: number, f: FadeTunables, durationS: number): number {
  if (durationS <= 0) return 0;
  if (t < f.holdS) return 1;
  const x = Math.min(1, Math.max(0, (t - f.holdS) / durationS));
  return (1 - x) ** f.easePower;
}

/** What the screen shows: waiting (the readout, HUD hidden), fading (white, going), or open. */
export type LoadingPhase = 'waiting' | 'fading' | 'open';

/** `Client.hud()`'s loading fields: undefined before the game starts. */
export interface LoadingFrame {
  loading?: LoadingHud | null;
  loadingOpens?: number;
}

/**
 * The screen's states, frame by frame: waiting from the start (enabled) and while the client says
 * so, fading from white once it opens (the screen went, or the client's opening count rose: a
 * respawn whose ring was already loaded opens in its first frame), then open.
 */
export class LoadingFlow {
  phase: LoadingPhase;
  /** The client's readout while waiting (null before it has one). */
  hud: LoadingHud | null = null;
  /** How many times it has opened (each begins a fade, or opens at once with none). */
  openings = 0;
  private opens = 0;
  private fadeStart = 0;
  private aborted = false;

  constructor(
    enabled: boolean,
    private readonly fade: FadeTunables,
    public durationS: number,
  ) {
    this.phase = enabled ? 'waiting' : 'open';
  }

  /** This frame's state from the client; `nowS` a clock in seconds. Returns the cover's opacity. */
  update(f: LoadingFrame, nowS: number): number {
    if (this.aborted) return 0;
    const opens = f.loadingOpens ?? this.opens;
    if (f.loading) {
      this.phase = 'waiting';
      this.hud = f.loading;
    } else if (f.loading === null && (this.phase === 'waiting' || opens > this.opens)) {
      this.phase = 'fading';
      this.fadeStart = nowS;
      this.openings += 1;
    }
    if (f.loading !== undefined) this.opens = opens;
    if (this.phase === 'fading') {
      const o = fadeOpacity(nowS - this.fadeStart, this.fade, this.durationS);
      if (o <= 0) this.phase = 'open';
      return o;
    }
    return this.phase === 'waiting' ? 1 : 0;
  }

  /** The game did not start (an error, the menu): nothing to cover. */
  abort(): void {
    this.aborted = true;
    this.phase = 'open';
  }
}

/** The body's class while waiting: the HUD is hidden under the screen (style.css). */
export const WAITING_CLASS = 'loading-wait';

export class LoadingScreen {
  private readonly root: HTMLDivElement;
  private readonly text: HTMLDivElement;
  private readonly fill: HTMLDivElement;
  private readonly flow: LoadingFlow;
  private readonly reducedMotion: boolean;
  private shown: string | null = null;
  private drawn: LoadingPhase | null = null;
  /** Called once each time the screen opens (the fade begins). */
  onOpen: (() => void) | null = null;

  /** Up at once when `enabled`: the first thing the page shows. */
  constructor(parent: HTMLElement, enabled: boolean, reducedMotion: boolean) {
    this.flow = new LoadingFlow(
      enabled,
      LOADING_FADE,
      fadeDurationS(LOADING_FADE, false, reducedMotion),
    );
    this.reducedMotion = reducedMotion;
    this.root = document.createElement('div');
    this.root.id = 'loading-screen';
    this.root.setAttribute('role', 'status');
    this.root.setAttribute('aria-live', 'polite');
    this.root.style.setProperty('--ls-fade', LOADING_FADE.color);
    const sheet = document.createElement('div');
    sheet.className = 'ls-sheet';
    const title = document.createElement('div');
    title.className = 'ls-title';
    title.textContent = 'Dwell';
    this.text = document.createElement('div');
    this.text.className = 'ls-text';
    this.text.textContent = 'Starting…';
    const bar = document.createElement('div');
    bar.className = 'ls-bar';
    this.fill = document.createElement('div');
    this.fill.className = 'ls-fill';
    this.fill.style.width = '0%';
    bar.append(this.fill);
    sheet.append(title, this.text, bar);
    this.root.append(sheet);
    parent.append(this.root);
    this.draw(1);
  }

  /** The adapter is a CPU (software) one: the quick fade. */
  setCpuAdapter(cpu: boolean): void {
    this.flow.durationS = fadeDurationS(LOADING_FADE, cpu, this.reducedMotion);
  }

  /** The latest frame's loading state (`Client.hud()`); `nowMs` the frame's clock. */
  frame(f: LoadingFrame, nowMs: number): void {
    const before = this.flow.openings;
    const opacity = this.flow.update(f, nowMs / 1000);
    if (this.flow.openings !== before) this.onOpen?.();
    this.draw(opacity);
  }

  /**
   * What the page is doing before the client has a readout (shaping a local world's maps, ADR
   * 0049): shown while waiting without one.
   */
  note(text: string): void {
    if (this.flow.phase !== 'waiting' || this.flow.hud) return;
    this.shown = null;
    this.text.textContent = text;
  }

  /** The game did not start (an error, the menu): the screen goes, the page shows what it says. */
  abort(): void {
    this.flow.abort();
    this.draw(0);
  }

  private draw(opacity: number): void {
    const phase = this.flow.phase;
    if (phase !== this.drawn) {
      this.drawn = phase;
      this.root.hidden = phase === 'open';
      this.root.classList.toggle('ls-fading', phase === 'fading');
      document.body.classList.toggle(WAITING_CLASS, phase === 'waiting');
    }
    this.root.style.opacity = phase === 'fading' ? opacity.toFixed(3) : '';
    const h = this.flow.hud;
    if (phase !== 'waiting' || !h) return;
    const key = `${h.text}|${String(h.ready)}|${String(h.needed)}`;
    if (key === this.shown) return;
    this.shown = key;
    this.text.textContent = h.text;
    this.fill.style.width = `${(loadingFraction(h) * 100).toFixed(1)}%`;
  }

  /** Waiting or fading: something covers the view. */
  get visible(): boolean {
    return !this.root.hidden;
  }

  get phase(): LoadingPhase {
    return this.flow.phase;
  }
}
