// Theater mode in game (docs/THEATER_MODE.md §2, §6; T-2 and T-3): the toggle, the free-fly camera
// (keyboard and mouse, controller, touch), the HUD switch, and the toolbar with its three panels
// (presets, keyframes, recording) and the timeline strip. The character stays put: while the mode
// is on the shell's input goes to the camera (`Client.theaterFly`), not the body.

import { type Aspect, THEATER, type TheaterConfig } from './config';
import { setHud } from './hud';
import { KeyframeEditor, type PathJson, UPS, type Up, type Vec3 } from './keyframes';
import { download, extensionOf, fitOnScreen, recordSize, startTake, type Take } from './record';
import { PathStore, exportName, parseImported } from './store';

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
  theaterState(): unknown;
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
  /** Renders the canvas at `size` (device px) for a take, or as the window again (null). */
  canvasSize(size: { width: number; height: number } | null): void;
  /** Where the path is made (saved with it as a hint). */
  where(): { world?: string; seed?: string };
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

export class TheaterMode {
  active = false;
  /** False when the host turned the mode off for guests (THEATER_MODE.md §7). */
  allowed = true;
  speed = THEATER.fly.speedMps;
  readonly editor = new KeyframeEditor('shot', THEATER.keyframes.gapS, 'ease');
  private readonly held = new Set<string>();
  private look: [number, number] = [0, 0];
  private touchLook: { id: number; x: number; y: number } | null = null;
  private touchMove: { id: number; x: number; y: number; dx: number; dy: number } | null = null;
  private lastMs: number | null = null;
  private take: Take | null = null;
  private takeEndsWithPath = false;
  private takeSize = { width: 0, height: 0 };
  private downloadTake = true;
  private takeDone: ((r: TakeResult) => void) | null = null;
  private timeOfDay: number | undefined;
  /** The face whose up the camera holds (the keys and presets take it; 'auto' the eye's). */
  private up: Up = 'auto';
  private aspect: Aspect;
  private height = THEATER.record.defaultHeight;
  private fps = THEATER.record.defaultFps;
  private readonly bar: HTMLElement;
  private readonly openButton: HTMLButtonElement;
  private readonly panels = new Map<string, HTMLElement>();
  private readonly timeline: HTMLElement;
  private readonly status: HTMLElement;
  private readonly presetsData: Preset[];
  private playing = false;

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
    this.bar = el(doc, 'div', 'theater-bar');
    this.bar.id = 'theater-bar';
    this.bar.hidden = true;
    this.status = el(doc, 'span', 'tb-status');
    this.timeline = el(doc, 'div', 'tb-timeline');
    this.build();
    // The touch screen's way in (a phone has no key): a button among the HUD's, hidden in the mode.
    this.openButton = el(doc, 'button', 'theater-open', 'Theater');
    this.openButton.type = 'button';
    this.openButton.id = 'theater-open';
    this.openButton.hidden = true;
    this.openButton.addEventListener('click', () => {
      this.enter();
    });
    doc.body.append(this.bar, this.openButton);
  }

  /** Shows the touch button (a touch device, playing, the mode allowed). */
  showOpenButton(shown: boolean): void {
    const v = shown && this.allowed && !this.active;
    if (this.openButton.hidden === v) this.openButton.hidden = !v;
  }

  /** Turns theater mode on or off (its key, Settings → Theater, the controller, the touch button). */
  toggle(): void {
    if (this.active) this.exit();
    else this.enter();
  }

  enter(): void {
    if (this.active || !this.allowed) return;
    this.active = true;
    this.held.clear();
    // From the player's eyes: a zero move places the view there.
    this.host.client.theaterFly(0, 0, 0, 0, 0);
    this.bar.hidden = false;
    setHud(this.host.doc, this.host, false);
    this.host.touch(false);
    this.show('keys');
    this.refresh();
  }

  exit(): void {
    if (!this.active) return;
    void this.stopTake();
    this.active = false;
    this.playing = false;
    this.host.client.releaseCamera();
    this.host.client.setTimeOfDay(undefined);
    this.bar.hidden = true;
    setHud(this.host.doc, this.host, true);
    this.host.touch(true);
  }

  /** A key event while the mode is on; true when theater took it (the game must not see it). */
  key(code: string, down: boolean, shift: boolean): boolean {
    if (!this.active) return false;
    if (down && code === 'Escape') {
      if (this.take) void this.stopTake();
      else if (this.playing) this.stop();
      else this.exit();
      return true;
    }
    if (down) this.held.add(code);
    else this.held.delete(code);
    if (shift) this.held.add('Shift');
    else this.held.delete('Shift');
    return true;
  }

  /** The mouse moved (pointer locked): the free camera turns. */
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

  /** Touch on the canvas: the left half moves (a virtual stick), the right half looks. */
  touchStart(id: number, x: number, y: number, width: number): void {
    if (!this.active) return;
    if (x < width / 2) this.touchMove ??= { id, x, y, dx: 0, dy: 0 };
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

  /** Each frame: flies the camera (when no path plays), reads the controller, ends a take. */
  frame(now: number, pad: Gamepad | null): void {
    const dt = this.lastMs === null ? 0 : Math.min(0.1, (now - this.lastMs) / 1000);
    this.lastMs = now;
    if (!this.active) return;
    const st = this.host.client.theaterState() as { time: number; finished: boolean } | null;
    if (this.playing && (!st || st.finished)) {
      this.playing = false;
      if (this.take && this.takeEndsWithPath) void this.stopTake();
      else setHud(this.host.doc, this.host, false, false);
      this.refresh();
    }
    if (this.playing) {
      this.drawTimeline(st?.time ?? 0);
      return;
    }
    const m = flyMove(this.held, this.speed, dt, this.held.has('Shift'));
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
    // The bar steps aside while the path plays (Esc stops it).
    setHud(this.host.doc, this.host, false, true);
  }

  private stop(): void {
    this.host.client.theaterStop();
    this.playing = false;
    this.host.client.theaterFly(0, 0, 0, 0, 0);
    setHud(this.host.doc, this.host, false, false);
    this.refresh();
  }

  /**
   * Starts a take; `path` plays it (the take ends with the path), else Esc (or `endTake`) ends
   * it. Resolves with the file when it ends (downloaded too unless `download` is false); the
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
    } = {},
  ): Promise<TakeResult | null> {
    if (typeof MediaRecorder === 'undefined') {
      this.say('This browser cannot record the canvas.');
      return Promise.resolve(null);
    }
    if (!this.active) this.enter();
    if (this.take) return Promise.resolve(null);
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
    this.take = startTake(
      this.host.canvas,
      fps,
      THEATER.record.mimeTypes,
      THEATER.record.bitsPerSecond,
    );
    this.takeEndsWithPath = path !== null;
    this.takeSize = size;
    setHud(this.host.doc, this.host, false, true);
    if (path) this.play(path);
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
    if (this.playing) {
      this.host.client.theaterStop();
      this.playing = false;
    }
    const blob = await take.stop();
    for (const p of ['position', 'left', 'top', 'width', 'height'] as const)
      this.host.canvas.style[p] = '';
    this.host.canvasSize(null);
    if (this.active) setHud(this.host.doc, this.host, false, false);
    if (this.downloadTake)
      download(this.host.doc, blob, `${this.editor.name}.${extensionOf(take.mime)}`);
    this.say(`Saved ${String(Math.round(blob.size / 1024))} kB`);
    this.takeDone?.({ blob, mime: take.mime, ...this.takeSize });
    this.takeDone = null;
  }

  private say(text: string): void {
    this.status.textContent = text;
  }

  // ----- The toolbar -----

  private show(name: string): void {
    for (const [n, p] of this.panels) p.hidden = n !== name;
    for (const t of this.bar.querySelectorAll<HTMLElement>('.tb-tab'))
      t.setAttribute('aria-pressed', String(t.dataset.panel === name));
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

  private select(options: string[], value: string, on: (v: string) => void): HTMLSelectElement {
    const s = el(this.host.doc, 'select', 'tb-select');
    for (const o of options) s.append(new Option(o, o, false, o === value));
    s.addEventListener('change', () => {
      on(s.value);
    });
    return s;
  }

  private build(): void {
    const { doc } = this.host;
    const head = el(doc, 'div', 'tb-head');
    head.append(el(doc, 'span', 'tb-title', 'Theater'));
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
      t.dataset.panel = name;
      head.append(t);
    }
    head.append(
      this.button(
        'Exit',
        () => {
          this.exit();
        },
        'tb-btn tb-exit',
      ),
    );
    this.bar.append(head);
    this.panels.set('presets', this.presetPanel());
    this.panels.set('keys', this.keysPanel());
    this.panels.set('record', this.recordPanel());
    for (const p of this.panels.values()) this.bar.append(p);
    this.bar.append(this.timeline, this.status);
    // The bar's own clicks and keys are not the camera's.
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
        const row = el(doc, 'label', 'tb-row');
        const out = el(doc, 'span', 'tb-value', String(r.default));
        const input = el(doc, 'input');
        input.type = 'range';
        input.min = String(r.min);
        input.max = String(r.max);
        input.step = String((r.max - r.min) / 200);
        input.value = String(r.default);
        input.addEventListener('input', () => {
          params[name] = Number(input.value);
          out.textContent = String(Math.round(Number(input.value) * 10) / 10);
        });
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
    const row = el(doc, 'div', 'tb-row');
    row.append(
      this.select(
        this.presetsData.map((x) => x.name),
        preset?.name ?? '',
        (v) => {
          preset = this.presetsData.find((x) => x.name === v);
          fill();
        },
      ),
      this.select(['player', 'creature', 'point'], target, (v) => {
        target = v;
      }),
    );
    const actions = el(doc, 'div', 'tb-row');
    actions.append(
      this.button('Preview', () => {
        const path = expand();
        if (path) this.play(path);
      }),
      this.button('To keyframes', () => {
        const path = expand();
        if (!path) return;
        this.editor.load(path);
        this.show('keys');
        this.refresh();
      }),
    );
    fill();
    p.append(row, sliders, actions);
    return p;
  }

  /**
   * The preset's subject: the player, the creature nearest the crosshair's point (as a point:
   * creatures carry no id to the shell yet), or the point under the crosshair.
   */
  private subject(kind: string): unknown {
    if (kind === 'player') return 'player';
    const aim = this.host.client.placement() as { cell: Vec3 } | null;
    const point: Vec3 | null = aim
      ? [aim.cell[0] + 0.5, aim.cell[1] + 0.5, aim.cell[2] + 0.5]
      : (this.camera()?.position ?? null);
    if (!point) throw new Error('nothing under the crosshair');
    if (kind === 'point') return point;
    const c = this.host.client.creatures();
    let best: Vec3 | null = null;
    let bestD = Infinity;
    for (let i = 0; i + 5 < c.length; i += 6) {
      const at: Vec3 = [c[i] ?? 0, (c[i + 1] ?? 0) + 1, c[i + 2] ?? 0];
      const d = Math.hypot(at[0] - point[0], at[1] - point[1], at[2] - point[2]);
      if (d < bestD) {
        bestD = d;
        best = at;
      }
    }
    if (!best) throw new Error('no creature in view');
    return best;
  }

  private keysPanel(): HTMLElement {
    const { doc } = this.host;
    const p = el(doc, 'div', 'tb-panel');
    const speed = el(doc, 'input', 'tb-num');
    speed.type = 'number';
    speed.min = '0.05';
    speed.step = '0.25';
    speed.value = '1';
    const fov = el(doc, 'input', 'tb-num');
    fov.type = 'number';
    fov.min = '5';
    fov.max = '150';
    fov.placeholder = 'fov';
    const time = el(doc, 'input');
    time.type = 'range';
    time.min = '0';
    time.max = '1';
    time.step = '0.005';
    const timeOn = el(doc, 'input');
    timeOn.type = 'checkbox';
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
    const move = this.select(THEATER.keyframes.moves, 'ease', (v) => {
      const s = this.editor.selected;
      if (s !== null) this.editor.setMove(Math.min(s, this.editor.segments.length - 1), v);
      this.refresh();
    });
    move.title = 'The move from the selected keyframe to the next';
    const name = el(doc, 'input', 'tb-name');
    name.value = this.editor.name;
    name.addEventListener('change', () => {
      this.editor.name = name.value.trim() || 'shot';
    });
    const saved = el(doc, 'select', 'tb-select');
    const listSaved = (): void => {
      saved.replaceChildren(new Option('Saved…', ''));
      for (const s of this.host.store.list()) saved.append(new Option(s.name, s.name));
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
    const r1 = el(doc, 'div', 'tb-row');
    r1.append(
      this.button(
        '+ Key',
        () => {
          const k = keyNow();
          if (k) this.editor.add(k);
          this.refresh();
        },
        'tb-btn tb-primary',
      ),
      this.button('Set', () => {
        const k = keyNow();
        const s = this.editor.selected;
        if (k && s !== null) this.editor.update(s, k);
        this.refresh();
      }),
      this.button('Delete', () => {
        const s = this.editor.selected;
        if (s !== null) this.editor.remove(s);
        this.refresh();
      }),
      move,
    );
    const r2 = el(doc, 'div', 'tb-row');
    const sp = el(doc, 'label', 'tb-label', 'speed ');
    sp.append(speed);
    const tl = el(doc, 'label', 'tb-label', 'time ');
    tl.append(timeOn, time);
    const upSel = this.select([...UPS], this.up, (v) => {
      this.up = v as Up;
      this.host.client.theaterUp(this.up);
    });
    upSel.title =
      "Whose up the camera holds: auto (the face it's over), or face a's or b's wherever it is (upside down on the other face's side)";
    const ul = el(doc, 'label', 'tb-label', 'up ');
    ul.append(upSel);
    r2.append(sp, fov, tl, ul);
    const r3 = el(doc, 'div', 'tb-row');
    r3.append(
      this.button(
        '▶ Play',
        () => {
          if (this.editor.keys.length >= 2) this.play(this.editor.toPath());
          else this.say('Add two keyframes first.');
        },
        'tb-btn tb-primary',
      ),
      name,
      this.button('Save', () => {
        this.host.store.save(this.editor.toPath(), this.host.where());
        listSaved();
        this.say(`Saved ${this.editor.name}`);
      }),
      saved,
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
    );
    listSaved();
    p.append(r1, r2, r3);
    return p;
  }

  private recordPanel(): HTMLElement {
    const { doc } = this.host;
    const p = el(doc, 'div', 'tb-panel');
    const r = THEATER.record;
    const row = el(doc, 'div', 'tb-row');
    row.append(
      this.select(
        r.aspects.map((a) => a.name),
        this.aspect.name,
        (v) => {
          this.aspect = r.aspects.find((a) => a.name === v) ?? this.aspect;
        },
      ),
      this.select(
        r.heights.map((h) => `${String(h)}p`),
        `${String(this.height)}p`,
        (v) => {
          this.height = parseInt(v, 10);
        },
      ),
      this.select(
        r.fps.map((f) => `${String(f)} fps`),
        `${String(this.fps)} fps`,
        (v) => {
          this.fps = parseInt(v, 10);
        },
      ),
    );
    const actions = el(doc, 'div', 'tb-row');
    actions.append(
      this.button(
        '● Record path',
        () => {
          if (this.editor.keys.length < 2) this.say('Add two keyframes (or a preset) first.');
          else void this.record(this.editor.toPath());
        },
        'tb-btn tb-record',
      ),
      this.button('● Record free', () => {
        void this.record(null);
      }),
    );
    p.append(row, actions, el(doc, 'p', 'tb-hint', 'Esc ends a take; the file downloads.'));
    return p;
  }

  /** Redraws the timeline and the status line. */
  refresh(): void {
    this.drawTimeline(null);
    const n = this.editor.keys.length;
    this.say(
      `${String(n)} key${n === 1 ? '' : 's'} · ${this.editor.duration.toFixed(1)} s · fly ${this.speed.toFixed(1)} m/s`,
    );
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
      const m = el(doc, 'button', 'tb-key');
      m.type = 'button';
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
