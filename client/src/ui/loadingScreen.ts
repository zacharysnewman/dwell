// The loading screen on joining a world and on respawning (ARCHITECTURE.md §6.3): the Rust client
// decides when it shows and what it reads (`Client.hud().loading`: the near ring's chunks drawn out
// of those needed, or null once open, from shared/look/loading.json's radius and timeout); this
// draws it over the view. It is the first thing the game page shows: up before the client starts
// (no HUD yet, so `loading` is undefined) and the HUD hidden under it until it opens. When it opens
// it turns white and fades out (`LOADING_FADE`, shared/look/loading.json's fade), uncovering the
// view and the HUD together: quick on a CPU adapter, and none (or quick) for prefers-reduced-motion.
// Opted out for testing with `?loading=off` or the `dwell.loading` store set to "off" (the e2e
// suite's storage does), it never shows. While it waits it lists the start's stages as they run,
// each with its time (`StageTracker`, loadingStages.ts; shared/look/loading.json `stages`), so a
// slow one shows rather than the screen looking hung; the headline follows the latest. The bar is a
// time estimate for this device (loadingEstimate.ts): it fills by elapsed / (elapsed + left), reads
// "About N s", and at its hold point short of full a shimmer runs along it until the screen opens.
import type { KeyValueStore } from '../local/worldIndex';
import { LOADING_FADE } from './loading.gen';
import { BarFill, SecondsLabel, estimate, type DeviceRead } from './loadingEstimate';
import { PERFORMANCE_MARKS, StageTracker, stagesFromHud } from './loadingStages';

/** The dev store that turns the screen off ("off"); anything else, or nothing, leaves it on. */
export const LOADING_KEY = 'dwell.loading';

/** `Client.hud().loading`: null once open (undefined before the game starts). */
export interface LoadingHud {
  text: string;
  /** Its stage (shared/look/loading.json `stages`): `join`, then `terrain`; `worldMap`. */
  stage?: string;
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
  private readonly bar: HTMLDivElement;
  private readonly fill: HTMLDivElement;
  private readonly eta: HTMLDivElement;
  private readonly barFill = new BarFill();
  private readonly label = new SecondsLabel();
  private barAtMs: number | null = null;
  /** The device as read at the start (the estimate's opening guess); the adapter set later. */
  device: DeviceRead = { phone: false, cpuAdapter: false, workers: 4 };
  /** Joining a server: the local world's stages are not to come. */
  remote = false;
  private readonly list: HTMLOListElement;
  private readonly flow: LoadingFlow;
  private readonly reducedMotion: boolean;
  private shown: string | null = null;
  private drawn: LoadingPhase | null = null;
  private ticker: ReturnType<typeof setInterval> | null = null;
  /** The start's stages (shared/look/loading.json `stages`): reported by id, listed with times. */
  readonly stages: StageTracker;
  /** Called once each time the screen opens (the fade begins). */
  onOpen: (() => void) | null = null;

  /** Up at once when `enabled`: the first thing the page shows. */
  constructor(
    parent: HTMLElement,
    enabled: boolean,
    reducedMotion: boolean,
    stages: StageTracker = new StageTracker(undefined, undefined, PERFORMANCE_MARKS),
  ) {
    this.stages = stages;
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
    this.bar = document.createElement('div');
    this.bar.className = 'ls-bar';
    this.fill = document.createElement('div');
    this.fill.className = 'ls-fill';
    this.fill.style.width = '0%';
    this.bar.append(this.fill);
    // The time left as estimated ("About 4 s"), hidden under a second.
    this.eta = document.createElement('div');
    this.eta.className = 'ls-eta';
    // The stages so far, each with its time (read by the screen, not announced line by line).
    this.list = document.createElement('ol');
    this.list.className = 'ls-stages';
    this.list.setAttribute('aria-hidden', 'true');
    sheet.append(title, this.text, this.bar, this.eta, this.list);
    this.root.append(sheet);
    parent.append(this.root);
    this.draw(1);
    // The running stage's time counts up even while the page has no frames yet (the WASM load).
    if (enabled) {
      this.ticker = setInterval(() => {
        this.drawStages(performance.now());
      }, 100);
    }
  }

  /** The adapter is a CPU (software) one: the quick fade. */
  setCpuAdapter(cpu: boolean): void {
    this.flow.durationS = fadeDurationS(LOADING_FADE, cpu, this.reducedMotion);
    this.device = { ...this.device, cpuAdapter: cpu };
  }

  /** The latest frame's loading state (`Client.hud()`); `nowMs` the frame's clock. */
  frame(f: LoadingFrame, nowMs: number): void {
    if (f.loading) stagesFromHud(this.stages, f.loading, nowMs);
    const before = this.flow.openings;
    const opacity = this.flow.update(f, nowMs / 1000);
    if (this.flow.openings !== before) {
      // Opened: this wait's stages end (a respawn's wait lists its own).
      this.stages.clear(nowMs);
      this.barFill.reset();
      this.label.reset();
      this.barAtMs = null;
      this.onOpen?.();
    }
    this.draw(opacity);
  }

  /** The game did not start (an error, the menu): the screen goes, the page shows what it says. */
  abort(): void {
    this.stages.endAll(performance.now());
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
      if (phase === 'open' && this.ticker !== null) {
        clearInterval(this.ticker);
        this.ticker = null;
      }
    }
    this.root.style.opacity = phase === 'fading' ? opacity.toFixed(3) : '';
    // Opened: the bar completes under the fade.
    if (phase === 'fading') {
      this.fill.style.width = '100%';
      this.bar.classList.remove('ls-holding');
    }
    if (phase === 'waiting') this.drawStages(performance.now());
  }

  /** The bar and the time left at `nowMs`: the estimate, eased; the shimmer at the hold point. */
  private drawBar(nowMs: number): void {
    const e = estimate({
      runs: this.stages.runsNow(),
      history: this.stages.history,
      device: this.device,
      remote: this.remote,
      nowMs,
    });
    const dtS = this.barAtMs === null ? 0 : (nowMs - this.barAtMs) / 1000;
    this.barAtMs = nowMs;
    const shown = this.barFill.update(e, dtS);
    this.fill.style.width = `${(shown * 100).toFixed(1)}%`;
    this.bar.classList.toggle('ls-holding', this.barFill.holding);
    this.eta.textContent = this.label.update(e.leftS, nowMs);
  }

  /** The headline (the running stage), the bar (the estimate) and the list, at `nowMs`. */
  private drawStages(nowMs: number): void {
    if (this.flow.phase !== 'waiting') return;
    this.drawBar(nowMs);
    const lines = this.stages.lines(nowMs);
    const current = this.stages.current();
    const head = lines.find((l) => l.id === current?.id);
    const key = lines.map((l) => `${l.text}|${l.progress}|${l.time}|${String(l.slow)}`).join(';');
    if (key === this.shown) return;
    this.shown = key;
    if (head) this.text.textContent = head.progress ? `${head.text} ${head.progress}` : head.text;
    // The list, its rows kept and updated in place.
    while (this.list.children.length > lines.length) this.list.lastElementChild?.remove();
    lines.forEach((l, i) => {
      let row = this.list.children[i] as HTMLLIElement | undefined;
      if (!row) {
        row = document.createElement('li');
        row.append(
          Object.assign(document.createElement('span'), { className: 'ls-stage-name' }),
          Object.assign(document.createElement('span'), { className: 'ls-stage-progress' }),
          Object.assign(document.createElement('span'), { className: 'ls-stage-time' }),
        );
        this.list.append(row);
      }
      row.dataset.stage = l.id;
      row.classList.toggle('ls-running', l.running);
      row.classList.toggle('ls-slow', l.slow);
      const [name, progress, time] = row.children;
      if (name) name.textContent = l.text;
      if (progress) progress.textContent = l.progress;
      if (time) time.textContent = l.time;
    });
  }

  /** Waiting or fading: something covers the view. */
  get visible(): boolean {
    return !this.root.hidden;
  }

  get phase(): LoadingPhase {
    return this.flow.phase;
  }

  /** The bar's state (the bench and `__dwell`): its fill, the label, and whether it holds. */
  get barState(): { fill: number; label: string; holding: boolean } {
    return { fill: this.barFill.shown, label: this.eta.textContent, holding: this.barFill.holding };
  }
}
