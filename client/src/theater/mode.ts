// Theater mode in game (docs/THEATER_MODE.md §2, §6; T-2 and T-3): the toggle, the free-fly camera
// (keyboard and mouse, controller, touch), the HUD switch, and the toolbar with its three panels
// (presets, keyframes, recording) and the timeline strip. The character stays put: while the mode
// is on the shell's input goes to the camera (`Client.theaterFly`), not the body.

import { type Aspect, THEATER, type TheaterConfig } from './config';
import { setHud } from './hud';
import { KeyframeEditor, type PathJson, UPS, type Up, type Vec3 } from './keyframes';
import { LAYOUT_CLASS, PadNav, type TheaterLayout, layoutFor, timerText } from './layout';
import {
  ExactTake,
  download,
  extensionOf,
  fitOnScreen,
  recordSize,
  startTake,
  type Take,
} from './record';
import { PathStore, exportName, parseImported } from './store';
import { TheaterTouch } from './touch';

/** What theater mode needs from the WASM client. */
export interface TheaterModeClient {
  theaterFly(
    forward: number,
    right: number,
    up: number,
    yaw: number,
    pitch: number,
  ): ArrayLike<number>;
  theaterPlay(json: string): void;
  theaterStop(): void;
  theaterPause(paused: boolean): void;
  /** Frame-exact recording on or off: the path moves only by `theaterAdvance`'s steps. */
  theaterExact(on: boolean): void;
  theaterAdvance(dtS: number): void;
  theaterState(): unknown;
  /** The creatures and NPCs this client sees, as JSON `[{id, name, npc, at}]`. */
  theaterSubjects(): string;
  theaterPresets(): string;
  theaterPreset(name: string, params: string, subject: string): string;
  /** Holds the free camera's up to a face ('a', 'b') or the one it is over ('auto'). */
  theaterUp(up: string): void;
  releaseCamera(): void;
  setTimeOfDay(t?: number): void;
  creatures(): Float64Array | number[];
  placement(): unknown;
}

export interface TheaterModeHost {
  doc: Document;
  canvas: HTMLCanvasElement;
  client: TheaterModeClient;
  store: PathStore;
  /** The first-person arms as the player's setting has them (true) or hidden. */
  arms(shown: boolean): void;
  /** The touch controls shown (as the device wants them) or hidden. */
  touch(shown: boolean): void;
  /** Whether this is a touch screen (theater's own flying controls show on one). */
  touchDevice?: () => boolean;
  /** Renders the canvas at `size` (device px) for a take, or as the window again (null). */
  canvasSize(size: { width: number; height: number } | null): void;
  /** Where the path is made (saved with it as a hint). */
  where(): { world?: string; seed?: string };
  /** The game's sound for a take (effects and ambience, no music or UI), or null without audio. */
  audioTap(): { stream: MediaStream; close(): void } | null;
  /**
   * The panels' owner (ui/panels.ts), when there is one: opening frees the mouse, closing locks it
   * again; it shows the mode through `setShown` and asks `escape` first on Esc.
   */
  panels?: { open(): void; close(): void };
}

interface Preset {
  name: string;
  label?: string;
  move: string;
  params: Record<string, { default?: number; min: number; max: number }>;
}

/** A finished take: the file and its frame size. */
export interface TakeResult {
  blob: Blob;
  mime: string;
  width: number;
  height: number;
}

/** The keys the free camera reads (KeyboardEvent.code). */
const FLY_KEYS: Record<string, [number, number, number]> = {
  KeyW: [1, 0, 0],
  KeyS: [-1, 0, 0],
  KeyD: [0, 1, 0],
  KeyA: [0, -1, 0],
  Space: [0, 0, 1],
  KeyE: [0, 0, 1],
  KeyC: [0, 0, -1],
  KeyQ: [0, 0, -1],
};

/** The free camera's move this frame (metres along forward, right, up) from the held keys. */
export function flyMove(
  held: ReadonlySet<string>,
  speed: number,
  dt: number,
  run: boolean,
): [number, number, number] {
  const m: [number, number, number] = [0, 0, 0];
  for (const code of held) {
    const v = FLY_KEYS[code];
    if (v) for (let k = 0; k < 3; k++) m[k] = (m[k] ?? 0) + (v[k] ?? 0);
  }
  const len = Math.hypot(...m);
  if (len === 0) return m;
  const step = (speed * dt * (run ? THEATER.fly.runFactor : 1)) / len;
  return [m[0] * step, m[1] * step, m[2] * step];
}

/** The free camera's speed after `notches` of the wheel (negative: faster), within its range. */
export function wheelSpeed(speed: number, notches: number, c: TheaterConfig = THEATER): number {
  const v = speed * c.fly.wheelStep ** -notches;
  return Math.min(c.fly.maxSpeedMps, Math.max(c.fly.minSpeedMps, v));
}

const el = <K extends keyof HTMLElementTagNameMap>(
  doc: Document,
  tag: K,
  cls?: string,
  text?: string,
): HTMLElementTagNameMap[K] => {
  const e = doc.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
};

/** An `<option>` (not `new Option`: built from the panel's own document). */
const option = (
  doc: Document,
  label: string,
  value: string,
  selected = false,
): HTMLOptionElement => {
  const o = doc.createElement('option');
  o.textContent = label;
  o.value = value;
  o.selected = selected;
  return o;
};

/** A subject to follow, as `Client.theaterSubjects` lists it. */
export interface Subject {
  id: number;
  name: string;
  npc: boolean;
  at: Vec3;
}

/** The subjects nearest `point` first, at most `n`. */
export function nearestSubjects(list: readonly Subject[], point: Vec3, n = 12): Subject[] {
  const d = (s: Subject): number =>
    Math.hypot(s.at[0] - point[0], s.at[1] - point[1], s.at[2] - point[2]);
  return [...list].sort((a, b) => d(a) - d(b)).slice(0, n);
}

/** A path's target for a subject: `{creature: id}` or `{npc: id}` (dwell_theater `TargetRef`). */
export function subjectTarget(s: Subject): { creature: number } | { npc: number } {
  return s.npc ? { npc: s.id } : { creature: s.id };
}

/** What the panel is doing: editing, playing a path, or recording a take. */
export type TheaterBusy = 'idle' | 'playing' | 'recording';

export class TheaterMode {
  active = false;
  /** False when the host turned the mode off for guests (THEATER_MODE.md §7). */
  allowed = true;
  speed = THEATER.fly.speedMps;
  readonly editor = new KeyframeEditor('shot', THEATER.keyframes.gapS, 'ease');
  /** The panel (`#theater-bar`): a dialog in the menu's style, its layout by the viewport. */
  readonly bar: HTMLElement;
  private readonly held = new Set<string>();
  private look: [number, number] = [0, 0];
  private touchLook: { id: number; x: number; y: number } | null = null;
  private touchMove: { id: number; x: number; y: number; dx: number; dy: number } | null = null;
  private lastMs: number | null = null;
  private take: Take | null = null;
  /** A frame-exact take (record.ts `ExactTake`), and whether the frame it waits for was stepped. */
  private exact: ExactTake | null = null;
  private exactStepped = false;
  private exactFps = 30;
  /** The Record panel's frame-exact choice. */
  private exactChosen = false;
  private takeEndsWithPath = false;
  private takeSize = { width: 0, height: 0 };
  private takeStartMs = 0;
  private downloadTake = true;
  private takeDone: ((r: TakeResult) => void) | null = null;
  private lastTake: { blob: Blob; name: string } | null = null;
  private timeOfDay: number | undefined;
  /** The face whose up the camera holds (the keys and presets take it; 'auto' the eye's). */
  private up: Up = 'auto';
  private aspect: Aspect;
  private height = THEATER.record.defaultHeight;
  private fps = THEATER.record.defaultFps;
  private readonly openButton: HTMLButtonElement;
  private readonly panels = new Map<string, HTMLElement>();
  private readonly timeline: HTMLElement;
  private readonly status: HTMLElement;
  private readonly badge: HTMLElement;
  private readonly badgeText: HTMLElement;
  private readonly pauseButton: HTMLButtonElement;
  private readonly stopButton: HTMLButtonElement;
  private readonly body: HTMLElement;
  private readonly collapse: HTMLButtonElement;
  private readonly keyList: HTMLOListElement;
  private readonly downloadButton: HTMLButtonElement;
  private readonly presetsData: Preset[];
  private readonly nav = new PadNav();
  /** A phone's flying controls (touch.ts). */
  readonly touchPad: TheaterTouch;
  private playing = false;
  private paused = false;
  private layout: TheaterLayout | null = null;
  /** The subject the Presets panel aims at (a creature or NPC picked from the list or the crosshair). */
  private picked: Subject | null = null;

  constructor(private readonly host: TheaterModeHost) {
    this.aspect =
      THEATER.record.aspects.find((a) => a.name === THEATER.record.defaultAspect) ??
      (THEATER.record.aspects[0] as Aspect);
    try {
      this.presetsData = (
        JSON.parse(host.client.theaterPresets()) as { presets: Preset[] }
      ).presets;
    } catch {
      this.presetsData = [];
    }
    const { doc } = host;
    this.bar = el(doc, 'section', 'theater-bar');
    this.bar.id = 'theater-bar';
    this.bar.setAttribute('role', 'dialog');
    this.bar.setAttribute('aria-label', 'Theater');
    this.bar.hidden = true;
    this.status = el(doc, 'p', 'tb-status');
    this.status.setAttribute('role', 'status');
    this.timeline = el(doc, 'div', 'tb-timeline');
    this.timeline.setAttribute('aria-hidden', 'true');
    this.badge = el(doc, 'span', 'tb-badge');
    this.badge.hidden = true;
    this.badge.append(el(doc, 'span', 'tb-dot'));
    this.badgeText = el(doc, 'span', 'tb-badge-text');
    this.badge.append(this.badgeText);
    this.pauseButton = this.button(
      'Pause',
      () => {
        this.setPaused(!this.paused);
      },
      'tb-btn tb-pause',
    );
    this.stopButton = this.button(
      'Stop',
      () => {
        this.escape();
      },
      'tb-btn tb-stop',
    );
    this.body = el(doc, 'div', 'tb-body');
    this.body.id = 'theater-body';
    this.collapse = this.button(
      '',
      () => {
        const open = this.collapse.getAttribute('aria-expanded') !== 'true';
        this.collapse.setAttribute('aria-expanded', String(open));
        this.body.hidden = !open;
      },
      'tb-icon tb-collapse',
    );
    this.collapse.setAttribute('aria-label', 'Show or hide the controls');
    this.collapse.setAttribute('aria-expanded', 'true');
    this.collapse.setAttribute('aria-controls', this.body.id);
    this.keyList = el(doc, 'ol', 'tb-keys');
    this.keyList.setAttribute('aria-label', 'Keyframes');
    this.downloadButton = this.button('Download', () => {
      if (this.lastTake) download(doc, this.lastTake.blob, this.lastTake.name);
    });
    this.downloadButton.disabled = true;
    this.touchPad = new TheaterTouch(doc);
    this.build();
    // The touch screen's way in (a phone has no key): a button among the HUD's, hidden in the mode.
    this.openButton = el(doc, 'button', 'theater-open', 'Theater');
    this.openButton.type = 'button';
    this.openButton.id = 'theater-open';
    this.openButton.hidden = true;
    this.openButton.addEventListener('click', () => {
      this.enter();
    });
    doc.body.append(this.bar, this.touchPad.element, this.openButton);
  }

  /** Shows the touch button (a touch device, playing, the mode allowed). */
  showOpenButton(shown: boolean): void {
    const v = shown && this.allowed && !this.active;
    if (this.openButton.hidden === v) this.openButton.hidden = !v;
  }

  /** Whether the host lets this player use the mode; leaves it when it no longer may. */
  setAllowed(allowed: boolean): void {
    if (this.allowed === allowed) return;
    this.allowed = allowed;
    if (!allowed) this.exit();
  }

  /** What the panel is doing now. */
  get busy(): TheaterBusy {
    if (this.take) return 'recording';
    return this.playing ? 'playing' : 'idle';
  }

  /** Turns theater mode on or off (its key, Settings → Theater, the controller, the touch button). */
  toggle(): void {
    if (this.active) this.exit();
    else this.enter();
  }

  /** Opens the mode: through the panels' owner when there is one (it frees the mouse). */
  enter(): void {
    if (this.active || !this.allowed) return;
    if (this.host.panels) this.host.panels.open();
    else this.setShown(true);
  }

  /** Closes the mode: through the panels' owner (it locks the mouse again). */
  exit(): void {
    if (!this.active) return;
    if (this.host.panels) this.host.panels.close();
    else this.setShown(false);
  }

  /** The panel's view, as the panels' owner shows or hides it (ui/panels.ts `theater`). */
  setShown(open: boolean): boolean {
    if (open) {
      if (!this.allowed) return false;
      if (this.active) return true;
      this.active = true;
      this.held.clear();
      // From the player's eyes: a zero move places the view there.
      this.host.client.theaterFly(0, 0, 0, 0, 0);
      this.bar.hidden = false;
      setHud(this.host.doc, this.host, false);
      this.host.touch(false);
      this.show('keys');
      this.relayout(innerWidth, innerHeight);
      this.refresh();
      return true;
    }
    if (!this.active) return true;
    void this.stopTake();
    this.active = false;
    this.playing = false;
    this.paused = false;
    this.host.client.releaseCamera();
    this.host.client.setTimeOfDay(undefined);
    this.bar.hidden = true;
    this.touchPad.show(false);
    setHud(this.host.doc, this.host, true);
    this.host.touch(true);
    return true;
  }

  /**
   * Esc while the mode is open: stops a take or a playing path (the panel stays); false when
   * there is neither, and the panels' owner closes the mode.
   */
  escape(): boolean {
    if (!this.active) return false;
    if (this.take) {
      void this.stopTake();
      return true;
    }
    if (this.playing) {
      this.stop();
      return true;
    }
    return false;
  }

  /** Lays the panel out for a viewport of `width` × `height` CSS px (layout.ts `layoutFor`). */
  relayout(width: number, height: number): TheaterLayout {
    const l = layoutFor(width, height);
    if (l !== this.layout) {
      for (const [k, c] of Object.entries(LAYOUT_CLASS)) this.bar.classList.toggle(c, k === l);
      this.layout = l;
    }
    return l;
  }

  /** A key event while the mode is on; true when theater took it (the game must not see it). */
  key(code: string, down: boolean, shift: boolean): boolean {
    if (!this.active) return false;
    // Esc is the panels' owner's (it asks `escape` first).
    if (code === 'Escape') return true;
    if (down) this.held.add(code);
    else this.held.delete(code);
    if (shift) this.held.add('Shift');
    else this.held.delete('Shift');
    return true;
  }

  /** The mouse moved (pointer locked, or a drag on the view): the free camera turns. */
  mouse(dx: number, dy: number): void {
    if (!this.active) return;
    this.look[0] += dx * THEATER.fly.lookDegPerPx;
    this.look[1] -= dy * THEATER.fly.lookDegPerPx;
  }

  wheel(deltaY: number): void {
    if (!this.active) return;
    this.speed = wheelSpeed(this.speed, deltaY / 100);
    this.refresh();
  }

  /**
   * Touch on the view: a drag looks. (The stick and the Up and Down buttons, touch.ts, move; a
   * device without them, `touchDevice` unset, moves with the left half as a virtual stick.)
   */
  touchStart(id: number, x: number, y: number, width: number): void {
    if (!this.active) return;
    if (!this.host.touchDevice && x < width / 2) this.touchMove ??= { id, x, y, dx: 0, dy: 0 };
    else this.touchLook ??= { id, x, y };
  }
  touchMoveTo(id: number, x: number, y: number): void {
    const l = this.touchLook;
    if (l?.id === id) {
      this.mouse(x - l.x, y - l.y);
      l.x = x;
      l.y = y;
    }
    const m = this.touchMove;
    if (m?.id === id) {
      m.dx = Math.max(-1, Math.min(1, (x - m.x) / 60));
      m.dy = Math.max(-1, Math.min(1, (y - m.y) / 60));
    }
  }
  touchEnd(id: number): void {
    if (this.touchLook?.id === id) this.touchLook = null;
    if (this.touchMove?.id === id) this.touchMove = null;
  }

  /**
   * Each frame: flies the camera (when no path plays), reads the controller (the sticks fly, the
   * d-pad and A/B work the panel), keeps the badge's timer, ends a take with its path.
   */
  frame(now: number, pad: Gamepad | null): void {
    const dt = this.lastMs === null ? 0 : Math.min(0.1, (now - this.lastMs) / 1000);
    this.lastMs = now;
    if (!this.active) return;
    if (pad && !this.bar.hidden && this.nav.step(pad, this.bar) === 'close') {
      if (!this.escape()) this.exit();
      return;
    }
    const st = this.host.client.theaterState() as { time: number; finished: boolean } | null;
    // A frame-exact take ends once its last pose is captured.
    if (this.playing && (!st || st.finished) && !(this.exact && this.exactStepped)) {
      this.playing = false;
      this.paused = false;
      if (this.take && this.takeEndsWithPath) void this.stopTake();
      this.refresh();
    }
    if (this.exact) {
      // The next frame of the take: one frame of its rate, once the last is captured and the
      // encoder keeps up (a slow page just takes longer).
      if (this.playing && !this.exactStepped && this.exact.ready) {
        this.host.client.theaterAdvance(1 / this.exactFps);
        this.exactStepped = true;
      }
      this.badgeText.textContent = `REC ${timerText((this.exact.frames * 1000) / this.exactFps)} · ${String(this.exact.frames)} frames`;
    } else if (this.take) this.badgeText.textContent = `REC ${timerText(now - this.takeStartMs)}`;
    this.showTouch();
    if (this.playing) {
      this.drawTimeline(st?.time ?? 0);
      return;
    }
    const m = flyMove(this.held, this.speed, dt, this.held.has('Shift'));
    const t = this.touchPad.move(this.speed, dt);
    for (let k = 0; k < 3; k++) m[k] = (m[k] ?? 0) + (t[k] ?? 0);
    if (this.touchMove) {
      const step = this.speed * dt;
      m[0] -= this.touchMove.dy * step;
      m[1] += this.touchMove.dx * step;
    }
    if (pad) {
      const dz = THEATER.fly.deadzone;
      const ax = (i: number): number => {
        const v = pad.axes[i] ?? 0;
        return Math.abs(v) < dz ? 0 : v;
      };
      const step = this.speed * dt;
      m[0] -= ax(1) * step;
      m[1] += ax(0) * step;
      m[2] += ((pad.buttons[7]?.value ?? 0) - (pad.buttons[6]?.value ?? 0)) * step;
      this.look[0] += ax(2) * THEATER.fly.padLookDegPerS * dt;
      this.look[1] -= ax(3) * THEATER.fly.padLookDegPerS * dt;
    }
    const [yaw, pitch] = this.look;
    if (m.some((v) => v !== 0) || yaw !== 0 || pitch !== 0)
      this.host.client.theaterFly(m[0], m[1], m[2], yaw, pitch);
    this.look = [0, 0];
  }

  /**
   * After the game drew (or skipped) a frame: a frame-exact take captures the canvas once per
   * stepped pose that was drawn.
   */
  afterFrame(drawn: boolean): void {
    if (this.exact && drawn && this.exactStepped) {
      this.exact.capture(this.host.canvas);
      this.exactStepped = false;
    }
  }

  /** The phone's flying controls: on a touch screen while the mode is on and nothing plays. */
  private showTouch(): void {
    const shown = this.active && !this.playing && (this.host.touchDevice?.() ?? false);
    this.touchPad.show(shown);
    const pad = this.touchPad.element;
    for (const [k, c] of Object.entries(LAYOUT_CLASS)) pad.classList.toggle(c, k === this.layout);
    pad.dataset.sheet = this.body.hidden ? 'collapsed' : 'open';
  }

  /** The camera now, as `theaterFly` reports it. */
  private camera(): { position: Vec3; look: [number, number] } | null {
    const v = this.host.client.theaterFly(0, 0, 0, 0, 0);
    if (v.length < 5) return null;
    return {
      position: [v[0] ?? 0, v[1] ?? 0, v[2] ?? 0],
      look: [v[3] ?? 0, v[4] ?? 0],
    };
  }

  private play(path: PathJson): void {
    try {
      this.host.client.theaterPlay(JSON.stringify(path));
    } catch (e) {
      this.say(String(e));
      return;
    }
    this.playing = true;
    this.paused = false;
    this.showBusy();
  }

  private stop(): void {
    this.host.client.theaterStop();
    this.playing = false;
    this.paused = false;
    this.host.client.theaterFly(0, 0, 0, 0, 0);
    this.refresh();
  }

  private setPaused(paused: boolean): void {
    if (!this.playing) return;
    this.paused = paused;
    this.host.client.theaterPause(paused);
    this.showBusy();
  }

  /** The header's state: the badge (a red dot and the timer while recording), Pause and Stop. */
  private showBusy(): void {
    const busy = this.busy;
    this.bar.dataset.busy = busy;
    this.badge.hidden = busy === 'idle';
    this.badge.classList.toggle('tb-rec', busy === 'recording');
    if (busy === 'playing') this.badgeText.textContent = this.paused ? 'Paused' : 'Playing';
    this.pauseButton.hidden = !this.playing;
    this.pauseButton.textContent = this.paused ? 'Resume' : 'Pause';
    this.pauseButton.setAttribute('aria-label', this.paused ? 'Resume the path' : 'Pause the path');
    this.stopButton.hidden = busy === 'idle';
    this.stopButton.setAttribute(
      'aria-label',
      busy === 'recording' ? 'Stop recording' : 'Stop the path',
    );
  }

  /**
   * Starts a take; `path` plays it (the take ends with the path), else Esc (or `endTake`) ends
   * it. Resolves with the file when it ends (downloaded too when `download` is true); the
   * aspect, height and frame rate are the Record panel's unless given (the dev API's
   * `theaterMode('record')`).
   */
  record(
    path: PathJson | null,
    opts: {
      aspect?: string | undefined;
      height?: number | undefined;
      fps?: number | undefined;
      download?: boolean | undefined;
      /** The game's sound on the take (effects and ambience, never the music): off by default. */
      audio?: boolean | undefined;
      /**
       * Frame-exact (a path only): every frame rendered at the rate whatever the page's speed,
       * a silent WebM (record.ts `ExactTake`).
       */
      exact?: boolean | undefined;
    } = {},
  ): Promise<TakeResult | null> {
    const exact = opts.exact === true && path !== null;
    if (exact && !ExactTake.supported()) {
      this.say('This browser cannot record frame by frame (no WebCodecs).');
      return Promise.resolve(null);
    }
    if (!exact && typeof MediaRecorder === 'undefined') {
      this.say('This browser cannot record the canvas.');
      return Promise.resolve(null);
    }
    if (!this.active) this.enter();
    if (this.take || !this.active) return Promise.resolve(null);
    const aspect = THEATER.record.aspects.find((a) => a.name === opts.aspect) ?? this.aspect;
    const fps = opts.fps ?? this.fps;
    const size = recordSize(aspect, opts.height ?? this.height);
    this.downloadTake = opts.download !== false;
    this.host.canvasSize(size);
    const fit = fitOnScreen(size, { width: innerWidth, height: innerHeight });
    Object.assign(this.host.canvas.style, {
      position: 'fixed',
      left: `${String(fit.left)}px`,
      top: `${String(fit.top)}px`,
      width: `${String(fit.width)}px`,
      height: `${String(fit.height)}px`,
    });
    const tap = opts.audio && !exact ? this.host.audioTap() : null;
    if (exact) {
      this.exact = new ExactTake(size, fps, THEATER.record.bitsPerSecond);
      this.exactFps = fps;
      // The first pose (path time 0) is drawn and captured before any step.
      this.exactStepped = true;
      this.host.client.theaterExact(true);
      this.take = this.exact;
    } else
      this.take = startTake(
        this.host.canvas,
        fps,
        THEATER.record.mimeTypes,
        THEATER.record.bitsPerSecond,
        tap && {
          ...tap,
          mimeTypes: THEATER.record.audioMimeTypes,
          bitsPerSecond: THEATER.record.audioBitsPerSecond,
        },
      );
    this.takeStartMs = performance.now();
    this.takeEndsWithPath = path !== null;
    this.takeSize = size;
    if (path) this.play(path);
    this.showBusy();
    return new Promise((resolve) => {
      this.takeDone = resolve;
    });
  }

  /** Ends the take recording now (as Esc does). */
  endTake(): Promise<void> {
    return this.stopTake();
  }

  private async stopTake(): Promise<void> {
    const take = this.take;
    if (!take) return;
    this.take = null;
    if (this.exact) {
      this.exact = null;
      this.exactStepped = false;
      this.host.client.theaterExact(false);
    }
    if (this.playing) {
      this.host.client.theaterStop();
      this.playing = false;
      this.paused = false;
    }
    this.showBusy();
    const blob = await take.stop();
    for (const p of ['position', 'left', 'top', 'width', 'height'] as const)
      this.host.canvas.style[p] = '';
    this.host.canvasSize(null);
    const name = `${this.editor.name}.${extensionOf(take.mime)}`;
    this.lastTake = { blob, name };
    this.downloadButton.disabled = false;
    if (this.downloadTake) download(this.host.doc, blob, name);
    this.say(`Take ready: ${String(Math.round(blob.size / 1024))} kB. Download saves it.`);
    this.takeDone?.({ blob, mime: take.mime, ...this.takeSize });
    this.takeDone = null;
    this.refresh(false);
  }

  private say(text: string): void {
    this.status.textContent = text;
  }

  // ----- The panel -----

  private show(name: string): void {
    for (const [n, p] of this.panels) p.hidden = n !== name;
    for (const t of this.bar.querySelectorAll<HTMLElement>('.tb-tab'))
      t.setAttribute('aria-selected', String(t.dataset.panel === name));
  }

  private button(label: string, run: () => void, cls = 'tb-btn'): HTMLButtonElement {
    const b = el(this.host.doc, 'button', cls, label);
    b.type = 'button';
    b.addEventListener('click', (e) => {
      e.stopPropagation();
      run();
    });
    return b;
  }

  private select(
    label: string,
    options: string[],
    value: string,
    on: (v: string) => void,
  ): HTMLLabelElement {
    const { doc } = this.host;
    const s = el(doc, 'select', 'tb-select');
    s.setAttribute('aria-label', label);
    for (const o of options) s.append(option(doc, o, o, o === value));
    s.addEventListener('change', () => {
      on(s.value);
    });
    return this.field(label, s);
  }

  /** A labelled control: the label's text above (or beside) the control. */
  private field(label: string, control: HTMLElement): HTMLLabelElement {
    const l = el(this.host.doc, 'label', 'tb-field');
    l.append(el(this.host.doc, 'span', 'tb-label', label), control);
    return l;
  }

  /** A row of chips (one chosen): the menu's segmented choice. */
  private chips(
    label: string,
    options: { value: string; label: string }[],
    value: string,
    on: (v: string) => void,
  ): HTMLElement {
    const { doc } = this.host;
    const g = el(doc, 'div', 'tb-chips');
    g.setAttribute('role', 'radiogroup');
    g.setAttribute('aria-label', label);
    const all: HTMLButtonElement[] = [];
    for (const o of options) {
      const c = this.button(
        o.label,
        () => {
          for (const x of all) x.setAttribute('aria-checked', String(x === c));
          on(o.value);
        },
        'tb-chip',
      );
      c.setAttribute('role', 'radio');
      c.setAttribute('aria-checked', String(o.value === value));
      all.push(c);
      g.append(c);
    }
    return g;
  }

  /** A titled group of controls. */
  private section(title: string, ...children: HTMLElement[]): HTMLElement {
    const s = el(this.host.doc, 'section', 'tb-section');
    s.append(el(this.host.doc, 'h3', 'tb-section-title', title), ...children);
    return s;
  }

  /** The rare controls, folded: the menu's disclosure (a chevron, item 23). */
  private advanced(...children: HTMLElement[]): HTMLDetailsElement {
    const d = el(this.host.doc, 'details', 'tb-more');
    const summary = el(this.host.doc, 'summary', undefined, 'Advanced');
    d.append(summary, ...children);
    return d;
  }

  private row(...children: HTMLElement[]): HTMLElement {
    const r = el(this.host.doc, 'div', 'tb-row');
    r.append(...children);
    return r;
  }

  private build(): void {
    const { doc } = this.host;
    const head = el(doc, 'header', 'tb-head');
    const title = el(doc, 'h2', 'tb-title', 'Theater');
    title.id = 'theater-title';
    this.bar.setAttribute('aria-labelledby', title.id);
    this.bar.removeAttribute('aria-label');
    const close = this.button(
      '',
      () => {
        this.exit();
      },
      'tb-icon tb-close',
    );
    close.id = 'theater-close';
    close.setAttribute('aria-label', 'Close theater mode');
    close.title = 'Close (Esc)';
    head.append(title, this.badge, this.pauseButton, this.stopButton, this.collapse, close);
    this.pauseButton.hidden = true;
    this.stopButton.hidden = true;
    const tabs = el(doc, 'div', 'tb-tabs');
    tabs.setAttribute('role', 'tablist');
    tabs.setAttribute('aria-label', 'Theater tools');
    for (const [name, label] of [
      ['presets', 'Presets'],
      ['keys', 'Keyframes'],
      ['record', 'Record'],
    ] as const) {
      const t = this.button(
        label,
        () => {
          this.show(name);
        },
        'tb-tab',
      );
      t.setAttribute('role', 'tab');
      t.dataset.panel = name;
      tabs.append(t);
    }
    this.panels.set('presets', this.presetPanel());
    this.panels.set('keys', this.keysPanel());
    this.panels.set('record', this.recordPanel());
    this.body.append(tabs);
    for (const p of this.panels.values()) {
      p.setAttribute('role', 'tabpanel');
      this.body.append(p);
    }
    this.body.append(this.timeline, this.status);
    this.bar.append(head, this.body);
    // The panel's own clicks and keys are not the camera's.
    this.bar.addEventListener('pointerdown', (e) => {
      e.stopPropagation();
    });
  }

  private presetPanel(): HTMLElement {
    const { doc } = this.host;
    const p = el(doc, 'div', 'tb-panel');
    let preset = this.presetsData[0];
    let target = 'player';
    let params: Record<string, number> = {};
    const sliders = el(doc, 'div', 'tb-sliders');
    const fill = (): void => {
      sliders.replaceChildren();
      params = {};
      if (!preset) return;
      for (const [name, r] of Object.entries(preset.params)) {
        if (r.default === undefined) continue;
        params[name] = r.default;
        const out = el(doc, 'output', 'tb-value', String(r.default));
        const input = el(doc, 'input', 'tb-range');
        input.type = 'range';
        input.min = String(r.min);
        input.max = String(r.max);
        input.step = String((r.max - r.min) / 200);
        input.value = String(r.default);
        input.setAttribute('aria-label', name);
        input.addEventListener('input', () => {
          params[name] = Number(input.value);
          out.textContent = String(Math.round(Number(input.value) * 10) / 10);
        });
        const row = el(doc, 'label', 'tb-slider');
        row.append(el(doc, 'span', 'tb-label', name), input, out);
        sliders.append(row);
      }
    };
    const expand = (): PathJson | null => {
      if (!preset) return null;
      try {
        const subject = preset.move === 'pan' ? null : this.subject(target);
        return JSON.parse(
          this.host.client.theaterPreset(
            preset.name,
            JSON.stringify(this.up === 'auto' ? params : { ...params, up: this.up }),
            JSON.stringify(subject),
          ),
        ) as PathJson;
      } catch (e) {
        this.say(String(e));
        return null;
      }
    };
    // The creature or NPC to follow: picked from the list (nearest the camera first) or under the
    // crosshair; the path aims at it by id, so the camera follows it as it moves.
    const who = el(doc, 'select', 'tb-select');
    who.setAttribute('aria-label', 'Creature to follow');
    const listSubjects = (): void => {
      const all = this.subjects();
      const cam = this.camera()?.position ?? [0, 0, 0];
      who.replaceChildren(option(doc, all.length ? 'Pick a creature…' : 'No creatures near', ''));
      for (const s of nearestSubjects(all, cam)) {
        const d = Math.round(Math.hypot(s.at[0] - cam[0], s.at[1] - cam[1], s.at[2] - cam[2]));
        who.append(option(doc, `${s.name} #${String(s.id)} · ${String(d)} m`, String(s.id)));
      }
      if (this.picked) who.value = String(this.picked.id);
    };
    who.addEventListener('focus', listSubjects);
    who.addEventListener('change', () => {
      this.picked = this.subjects().find((s) => String(s.id) === who.value) ?? null;
    });
    const aim = this.button('Crosshair', () => {
      try {
        const point = this.aimPoint();
        const s = nearestSubjects(this.subjects(), point, 1)[0];
        if (!s) throw new Error('no creature in view');
        this.picked = s;
        listSubjects();
        this.say(`Following ${s.name} #${String(s.id)}`);
      } catch (e) {
        this.say((e as Error).message);
      }
    });
    aim.setAttribute('aria-label', 'Pick the creature under the crosshair');
    const creatureRow = this.row(this.field('Creature', who), aim);
    creatureRow.hidden = true;
    const targets = this.chips(
      'Target',
      [
        { value: 'player', label: 'Me' },
        { value: 'creature', label: 'Creature' },
        { value: 'point', label: 'Point' },
      ],
      target,
      (v) => {
        target = v;
        creatureRow.hidden = v !== 'creature';
        if (v === 'creature') listSubjects();
      },
    );
    const move = this.select(
      'Move',
      this.presetsData.map((x) => x.name),
      preset?.name ?? '',
      (v) => {
        preset = this.presetsData.find((x) => x.name === v);
        fill();
      },
    );
    const actions = this.row(
      this.button(
        '▶ Preview',
        () => {
          const path = expand();
          if (path) this.play(path);
        },
        'tb-btn tb-primary',
      ),
      this.button('To keyframes', () => {
        const path = expand();
        if (!path) return;
        this.editor.load(path);
        this.show('keys');
        this.refresh();
      }),
    );
    fill();
    p.append(
      this.section('Target and move', targets, creatureRow, this.row(move)),
      this.section('Settings', sliders),
      actions,
    );
    return p;
  }

  /** The creatures and NPCs this client sees (`Client.theaterSubjects`). */
  private subjects(): Subject[] {
    try {
      return JSON.parse(this.host.client.theaterSubjects()) as Subject[];
    } catch {
      return [];
    }
  }

  /** The point under the crosshair (the targeted cell's centre), else the camera's. */
  private aimPoint(): Vec3 {
    const aim = this.host.client.placement() as { cell: Vec3 } | null;
    const point: Vec3 | null = aim
      ? [aim.cell[0] + 0.5, aim.cell[1] + 0.5, aim.cell[2] + 0.5]
      : (this.camera()?.position ?? null);
    if (!point) throw new Error('nothing under the crosshair');
    return point;
  }

  /**
   * The preset's subject: the player, the picked creature or NPC by id (followed as it moves; the
   * one nearest the crosshair when none is picked), or the point under the crosshair.
   */
  private subject(kind: string): unknown {
    if (kind === 'player') return 'player';
    if (kind === 'point') return this.aimPoint();
    const known = this.picked && this.subjects().find((s) => s.id === this.picked?.id);
    const s = known ?? nearestSubjects(this.subjects(), this.aimPoint(), 1)[0];
    if (!s) throw new Error('no creature in view');
    this.picked = s;
    return subjectTarget(s);
  }

  private keysPanel(): HTMLElement {
    const { doc } = this.host;
    const p = el(doc, 'div', 'tb-panel');
    const speed = el(doc, 'input', 'tb-num');
    speed.type = 'number';
    speed.min = '0.05';
    speed.step = '0.25';
    speed.value = '1';
    speed.setAttribute('aria-label', 'Speed at the key');
    const fov = el(doc, 'input', 'tb-num');
    fov.type = 'number';
    fov.min = '5';
    fov.max = '150';
    fov.placeholder = 'auto';
    fov.setAttribute('aria-label', 'Field of view at the key');
    const time = el(doc, 'input', 'tb-range');
    time.type = 'range';
    time.min = '0';
    time.max = '1';
    time.step = '0.005';
    time.setAttribute('aria-label', 'Time of day');
    const timeOn = el(doc, 'input');
    timeOn.type = 'checkbox';
    timeOn.setAttribute('aria-label', 'Set the time of day');
    const applyTime = (): void => {
      this.timeOfDay = timeOn.checked ? Number(time.value) : undefined;
      this.host.client.setTimeOfDay(this.timeOfDay);
    };
    time.addEventListener('input', () => {
      timeOn.checked = true;
      applyTime();
    });
    timeOn.addEventListener('change', applyTime);
    const keyNow = (): Parameters<KeyframeEditor['add']>[0] | null => {
      const cam = this.camera();
      if (!cam) return null;
      const k: Parameters<KeyframeEditor['add']>[0] = { position: cam.position, look: cam.look };
      const s = Number(speed.value);
      if (s > 0 && s !== 1) k.speed = s;
      if (fov.value !== '') k.fov = Number(fov.value);
      if (this.timeOfDay !== undefined) k.timeOfDay = this.timeOfDay;
      if (this.up !== 'auto') k.up = this.up;
      return k;
    };
    const move = this.select('Move to the next key', THEATER.keyframes.moves, 'ease', (v) => {
      const s = this.editor.selected;
      if (s !== null) this.editor.setMove(Math.min(s, this.editor.segments.length - 1), v);
      this.refresh();
    });
    const name = el(doc, 'input', 'tb-name');
    name.value = this.editor.name;
    name.setAttribute('aria-label', 'Path name');
    name.addEventListener('change', () => {
      this.editor.name = name.value.trim() || 'shot';
    });
    const saved = el(doc, 'select', 'tb-select');
    saved.setAttribute('aria-label', 'Saved paths');
    const listSaved = (): void => {
      saved.replaceChildren(option(doc, 'Saved…', ''));
      for (const s of this.host.store.list()) saved.append(option(doc, s.name, s.name));
    };
    saved.addEventListener('change', () => {
      const rec = this.host.store.get(saved.value);
      if (rec) {
        this.editor.load(rec.path);
        name.value = this.editor.name;
        this.refresh();
      }
    });
    const file = el(doc, 'input');
    file.type = 'file';
    file.accept = '.json,application/json';
    file.hidden = true;
    file.setAttribute('aria-label', 'Import a path file');
    file.addEventListener('change', () => {
      const f = file.files?.[0];
      if (!f) return;
      void f.text().then((text) => {
        try {
          this.editor.load(parseImported(text));
          name.value = this.editor.name;
          this.refresh();
        } catch (e) {
          this.say(`Import: ${(e as Error).message}`);
        }
      });
      file.value = '';
    });
    const upSel = this.select('Up', [...UPS], this.up, (v) => {
      this.up = v as Up;
      this.host.client.theaterUp(this.up);
    });
    upSel.title =
      "Whose up the camera holds: auto (the face it's over), or face a's or b's wherever it is (upside down on the other face's side)";
    const timeField = el(doc, 'label', 'tb-field');
    timeField.append(el(doc, 'span', 'tb-label', 'Time of day'), this.row(timeOn, time));
    const actions = this.row(
      this.button(
        '+ Add key',
        () => {
          const k = keyNow();
          if (k) this.editor.add(k);
          this.refresh();
        },
        'tb-btn tb-primary',
      ),
      this.button('Delete', () => {
        const s = this.editor.selected;
        if (s !== null) this.editor.remove(s);
        this.refresh();
      }),
      this.button('Preview', () => {
        const path = this.fromSelected();
        if (path) this.play(path);
        else this.say('Select a key with one after it.');
      }),
      this.button(
        '▶ Play',
        () => {
          if (this.editor.keys.length >= 2) this.play(this.editor.toPath());
          else this.say('Add two keyframes first.');
        },
        'tb-btn tb-primary',
      ),
    );
    listSaved();
    p.append(
      this.section('Keyframes', this.keyList, actions),
      this.advanced(
        this.row(
          this.button('Set key to camera', () => {
            const k = keyNow();
            const s = this.editor.selected;
            if (k && s !== null) this.editor.update(s, k);
            this.refresh();
          }),
          move,
        ),
        this.row(this.field('Speed', speed), this.field('Field of view', fov), upSel),
        timeField,
        this.row(
          this.field('Name', name),
          this.button('Save', () => {
            this.host.store.save(this.editor.toPath(), this.host.where());
            listSaved();
            this.say(`Saved ${this.editor.name}`);
          }),
          this.field('Open', saved),
        ),
        this.row(
          this.button('Export', () => {
            const blob = new Blob([JSON.stringify(this.editor.toPath(), null, 2)], {
              type: 'application/json',
            });
            download(doc, blob, exportName(this.editor.name));
          }),
          this.button('Import', () => {
            file.click();
          }),
          file,
        ),
      ),
    );
    return p;
  }

  /** The path from the selected key on (Preview), or null without a key after it. */
  private fromSelected(): PathJson | null {
    const s = this.editor.selected ?? 0;
    const path = this.editor.toPath();
    if (s >= path.keyframes.length - 1) return null;
    const keys = path.keyframes.slice(s);
    const t0 = (keys[0]?.t as number | undefined) ?? 0;
    return {
      ...path,
      keyframes: keys.map((k) => ({ ...k, t: Math.round(((k.t as number) - t0) * 1000) / 1000 })),
      segments: path.segments.slice(s),
    };
  }

  private recordPanel(): HTMLElement {
    const { doc } = this.host;
    const p = el(doc, 'div', 'tb-panel');
    const r = THEATER.record;
    const aspect = this.chips(
      'Aspect',
      r.aspects.map((a) => ({ value: a.name, label: a.name })),
      this.aspect.name,
      (v) => {
        this.aspect = r.aspects.find((a) => a.name === v) ?? this.aspect;
      },
    );
    const fps = this.chips(
      'Frame rate',
      r.fps.map((f) => ({ value: String(f), label: `${String(f)} fps` })),
      String(this.fps),
      (v) => {
        this.fps = parseInt(v, 10);
      },
    );
    const height = this.select(
      'Height',
      r.heights.map((h) => `${String(h)}p`),
      `${String(this.height)}p`,
      (v) => {
        this.height = parseInt(v, 10);
      },
    );
    const actions = this.row(
      this.button(
        '● Record path',
        () => {
          if (this.editor.keys.length < 2) this.say('Add two keyframes (or a preset) first.');
          else void this.record(this.editor.toPath(), { download: false, exact: this.exactChosen });
        },
        'tb-btn tb-record',
      ),
      this.button('● Record free', () => {
        void this.record(null, { download: false });
      }),
      this.downloadButton,
    );
    p.append(
      this.section('Format', this.field('Aspect', aspect), this.field('Frame rate', fps)),
      this.section('Take', actions),
      this.advanced(this.row(height), this.exactField()),
      el(doc, 'p', 'tb-hint', 'Stop or Esc ends a take; Download saves it.'),
    );
    return p;
  }

  /** Advanced: frame-exact takes of a path (smooth on a slow machine, silent). */
  private exactField(): HTMLLabelElement {
    const { doc } = this.host;
    const box = el(doc, 'input');
    box.type = 'checkbox';
    box.checked = this.exactChosen;
    box.setAttribute('aria-label', 'Frame-exact recording');
    box.addEventListener('change', () => {
      this.exactChosen = box.checked;
    });
    const l = el(doc, 'label', 'tb-check');
    l.append(
      box,
      el(doc, 'span', 'tb-label', 'Frame-exact path takes (smooth on a slow machine, no sound)'),
    );
    return l;
  }

  /** Redraws the key list, the timeline, the state and the status line. */
  refresh(status = true): void {
    this.drawKeys();
    this.drawTimeline(null);
    this.showBusy();
    if (!status) return;
    const n = this.editor.keys.length;
    this.say(
      `${String(n)} key${n === 1 ? '' : 's'} · ${this.editor.duration.toFixed(1)} s · fly ${this.speed.toFixed(1)} m/s`,
    );
  }

  /** The key list: one row per key (its time and the move after it), the selected one marked. */
  private drawKeys(): void {
    const { doc } = this.host;
    const e = this.editor;
    this.keyList.replaceChildren();
    if (e.keys.length === 0) {
      const li = el(doc, 'li', 'tb-keys-empty', 'No keys yet: fly the camera and add one.');
      this.keyList.append(li);
      return;
    }
    e.keys.forEach((k, i) => {
      const li = el(doc, 'li');
      const seg = e.segments[i];
      const b = this.button(
        `Key ${String(i + 1)} · ${k.t.toFixed(1)} s${seg ? ` · ${seg.move}` : ''}`,
        () => {
          e.selected = i;
          this.refresh(false);
        },
        'tb-keyrow',
      );
      b.setAttribute('aria-pressed', String(i === e.selected));
      li.append(b);
      this.keyList.append(li);
    });
  }

  private drawTimeline(playhead: number | null): void {
    const { doc } = this.host;
    const e = this.editor;
    const span = Math.max(e.duration, 1);
    this.timeline.replaceChildren();
    e.segments.forEach((s, i) => {
      const a = e.keys[i]?.t ?? 0;
      const b = e.keys[i + 1]?.t ?? a;
      const seg = el(doc, 'span', 'tb-seg', s.move);
      seg.style.left = `${String((a / span) * 100)}%`;
      seg.style.width = `${String(((b - a) / span) * 100)}%`;
      this.timeline.append(seg);
    });
    e.keys.forEach((k, i) => {
      const m = el(doc, 'span', 'tb-key');
      m.style.left = `${String((k.t / span) * 100)}%`;
      m.title = `${k.t.toFixed(1)} s`;
      if (i === e.selected) m.classList.add('tb-key-on');
      m.addEventListener('pointerdown', (ev) => {
        ev.stopPropagation();
        e.selected = i;
        const rect = this.timeline.getBoundingClientRect();
        const moveTo = (x: number): void => {
          e.move(i, ((x - rect.left) / rect.width) * span);
          m.style.left = `${String(((e.keys[i]?.t ?? 0) / span) * 100)}%`;
        };
        const onMove = (pe: PointerEvent): void => {
          moveTo(pe.clientX);
        };
        const onUp = (): void => {
          doc.removeEventListener('pointermove', onMove);
          doc.removeEventListener('pointerup', onUp);
          this.refresh();
        };
        doc.addEventListener('pointermove', onMove);
        doc.addEventListener('pointerup', onUp);
        for (const o of this.timeline.querySelectorAll('.tb-key-on'))
          o.classList.remove('tb-key-on');
        m.classList.add('tb-key-on');
      });
      this.timeline.append(m);
    });
    if (playhead !== null) {
      const h = el(doc, 'span', 'tb-playhead');
      h.style.left = `${String((playhead / span) * 100)}%`;
      this.timeline.append(h);
    }
  }
}
