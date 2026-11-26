// The Character painter (docs/CHARACTERS.md §9.4; owner, 2026-11-26): your character as a 30 m giant on
// an empty stage, flown round with theater's free camera, painted face by face from seven colour slots.
// Keys: 1–7 pick a slot, left click/drag paints with the brush, right click fills the connected patch,
// the wheel or [ ] sizes the brush, C changes the slot's colour from the palette, Ctrl+Z / Ctrl+Shift+Z
// undo and redo, Esc done. A controller: the sticks fly and look, RT paints, LT fills, the bumpers pick
// a slot, the d-pad's up and down fly up and down and its left and right size the brush, Y steps the
// slot's colour, B done. The look is kept on this device (`LOOK_KEY`) and sent to the other players.

import { THEATER } from '../theater/config';
import { flyMove } from '../theater/mode';

/** What the painter needs from the WASM client. */
export interface PainterClient {
  openPainter(): boolean;
  closePainter(keep: boolean): boolean;
  paint(op: string, slot: number, radiusCm: number): boolean;
  paintSlot(k: number, ramp: string): boolean;
  painterSlots(): string;
  ownLook(): string;
  theaterFly(forward: number, right: number, up: number, yaw: number, pitch: number): unknown;
}

/** The device's store key of your look (`Client.ownLook`'s JSON). */
export const LOOK_KEY = 'dwell.look';
export const SLOTS = 7;
/** The body's class while painting: the game's HUD hidden (style.css). */
export const PAINTING_CLASS = 'character-painting';
/** The brush's radius range and step (cm, on the 30 m giant). */
export const BRUSH_CM = { min: 20, max: 400, step: 1.25, start: 80 } as const;

/** The brush radius after `notches` of the wheel or [ ] (negative: smaller). */
export function brushAfter(cm: number, notches: number): number {
  return Math.round(Math.min(BRUSH_CM.max, Math.max(BRUSH_CM.min, cm * BRUSH_CM.step ** notches)));
}

/** The slot a key picks (Digit1–Digit7 → 0–6), or null. */
export function slotOfKey(code: string): number | null {
  const m = /^Digit([1-7])$/.exec(code);
  return m ? Number(m[1]) - 1 : null;
}

/** The ramp after `ramp` in `ramps` (wrapping), for the controller's Y. */
export function nextRamp(ramps: readonly string[], ramp: string, step = 1): string {
  const i = ramps.indexOf(ramp);
  const n = ramps.length;
  return ramps[(((i < 0 ? 0 : i) + step) % n + n) % n] ?? ramp;
}

/** The painter's keys move the camera, but C is the palette's (not down). */
const FLY_EXCLUDED = new Set(['KeyC']);

export class CharacterPainter {
  active = false;
  slot = 0;
  brushCm: number = BRUSH_CM.start;
  private held = new Set<string>();
  private look: [number, number] = [0, 0];
  private painting = false;
  private padWas: boolean[] = [];
  private lastMs = 0;
  /** The palette is open (the mouse freed on purpose: not "done"). */
  choosing = false;
  private readonly root: HTMLElement;
  private readonly bar: HTMLElement;
  private readonly size: HTMLElement;
  private readonly palette: HTMLElement;

  constructor(
    private readonly doc: Document,
    private readonly client: PainterClient,
    private readonly ramps: readonly (readonly [string, string])[],
    private readonly store: Pick<Storage, 'setItem'>,
    private readonly hooks: { lock(): void; unlock(): void; closed(): void },
  ) {
    this.root = doc.createElement('div');
    this.root.id = 'character-painter';
    this.root.className = 'character-painter';
    this.root.hidden = true;
    this.bar = doc.createElement('div');
    this.bar.className = 'cp-slots';
    this.size = doc.createElement('div');
    this.size.className = 'cp-size';
    const hint = doc.createElement('div');
    hint.className = 'cp-hint';
    hint.textContent =
      '1–7 colour · drag paint · right click fill · wheel brush · C palette · Ctrl+Z undo · Esc done';
    this.palette = doc.createElement('div');
    this.palette.className = 'cp-palette';
    this.palette.hidden = true;
    const cross = doc.createElement('div');
    cross.className = 'cp-cross';
    this.root.append(cross, this.bar, this.size, hint, this.palette);
    doc.body.append(this.root);
  }

  private slots(): string[] {
    try {
      const v: unknown = JSON.parse(this.client.painterSlots());
      return Array.isArray(v) ? v.map(String) : [];
    } catch {
      return [];
    }
  }

  private colour(ramp: string): string {
    return this.ramps.find(([n]) => n === ramp)?.[1] ?? '#888';
  }

  private refresh(): void {
    const slots = this.slots();
    this.bar.replaceChildren(
      ...slots.map((ramp, k) => {
        const b = this.doc.createElement('button');
        b.type = 'button';
        b.className = 'cp-slot' + (k === this.slot ? ' on' : '');
        b.style.background = this.colour(ramp);
        b.textContent = String(k + 1);
        b.title = ramp;
        b.addEventListener('click', () => {
          this.slot = k;
          this.refresh();
        });
        return b;
      }),
    );
    this.size.textContent = `brush ${(this.brushCm / 100).toFixed(1)} m`;
  }

  /** Opens it (from the game menu's Character page). */
  enter(): boolean {
    if (this.active || !this.client.openPainter()) return false;
    this.active = true;
    this.root.hidden = false;
    this.doc.body.classList.add(PAINTING_CLASS);
    this.held.clear();
    this.refresh();
    this.hooks.lock();
    return true;
  }

  /** Closes it, keeping the paint as your look and saving it on this device. */
  done(): void {
    if (!this.active) return;
    this.closePalette(false);
    this.active = false;
    this.root.hidden = true;
    this.doc.body.classList.remove(PAINTING_CLASS);
    this.client.closePainter(true);
    try {
      this.store.setItem(LOOK_KEY, this.client.ownLook());
    } catch {
      // A store that refuses (private mode, full): the look still applies for this session.
    }
    this.hooks.closed();
  }

  private act(op: string): void {
    if (this.client.paint(op, this.slot, this.brushCm) || op !== 'brush') this.refresh();
  }

  /** A key (down or up). Whether it was the painter's. */
  key(code: string, down: boolean, shift: boolean, ctrl: boolean): boolean {
    if (!this.active) return false;
    if (this.choosing) {
      if (down && (code === 'Escape' || code === 'KeyC')) this.closePalette(true);
      return true;
    }
    if (down) {
      const s = slotOfKey(code);
      if (s !== null) {
        this.slot = s;
        this.refresh();
      } else if (code === 'BracketLeft' || code === 'BracketRight') {
        this.brushCm = brushAfter(this.brushCm, code === 'BracketRight' ? 1 : -1);
        this.refresh();
      } else if (code === 'KeyC') {
        this.openPalette();
      } else if (code === 'KeyZ' && ctrl) {
        this.act(shift ? 'redo' : 'undo');
      } else if (code === 'Escape') {
        this.done();
      }
      if (!FLY_EXCLUDED.has(code)) this.held.add(code);
    } else this.held.delete(code);
    if (shift) this.held.add('Shift');
    else this.held.delete('Shift');
    return true;
  }

  mouse(dx: number, dy: number): void {
    if (!this.active || this.choosing) return;
    this.look[0] += dx * THEATER.fly.lookDegPerPx;
    this.look[1] -= dy * THEATER.fly.lookDegPerPx;
  }

  mouseDown(button: number): void {
    if (!this.active || this.choosing) return;
    if (button === 0) {
      this.painting = true;
      this.act('brush');
    } else if (button === 2) this.act('fill');
  }

  mouseUp(button: number): void {
    if (!this.active) return;
    if (button === 0 && this.painting) {
      this.painting = false;
      this.act('end');
    }
  }

  wheel(deltaY: number): void {
    if (!this.active || this.choosing) return;
    this.brushCm = brushAfter(this.brushCm, -deltaY / 100);
    this.refresh();
  }

  private openPalette(): void {
    this.choosing = true;
    const current = this.slots()[this.slot];
    this.palette.replaceChildren(
      ...this.ramps.map(([name, hex]) => {
        const b = this.doc.createElement('button');
        b.type = 'button';
        b.className = 'cp-swatch' + (name === current ? ' on' : '');
        b.style.background = hex;
        b.title = name.replace(/_/g, ' ');
        b.addEventListener('click', () => {
          this.client.paintSlot(this.slot, name);
          this.closePalette(true);
        });
        return b;
      }),
      (() => {
        const r = this.doc.createElement('button');
        r.type = 'button';
        r.className = 'cp-reset';
        r.textContent = 'Reset to the model';
        r.addEventListener('click', () => {
          this.act('reset');
          this.closePalette(true);
        });
        return r;
      })(),
    );
    this.palette.hidden = false;
    this.hooks.unlock();
  }

  private closePalette(relock: boolean): void {
    if (!this.choosing) return;
    this.choosing = false;
    this.palette.hidden = true;
    this.refresh();
    if (relock) this.hooks.lock();
  }

  /** Each animation frame: flies the camera, paints while held, reads the controller. */
  frame(nowMs: number, pad: Gamepad | null): void {
    if (!this.active) return;
    const dt = this.lastMs ? Math.min(0.1, (nowMs - this.lastMs) / 1000) : 0;
    this.lastMs = nowMs;
    if (this.choosing) return;
    const speed = THEATER.fly.speedMps * 2;
    const m = flyMove(this.held, speed, dt, this.held.has('Shift'));
    if (pad) {
      const dz = THEATER.fly.deadzone;
      const ax = (i: number): number => {
        const v = pad.axes[i] ?? 0;
        return Math.abs(v) < dz ? 0 : v;
      };
      const pressed = (i: number): boolean => pad.buttons[i]?.pressed ?? false;
      const edge = (i: number): boolean => pressed(i) && !this.padWas[i];
      const step = speed * dt;
      m[0] -= ax(1) * step;
      m[1] += ax(0) * step;
      m[2] += (Number(pressed(12)) - Number(pressed(13))) * step;
      this.look[0] += ax(2) * THEATER.fly.padLookDegPerS * dt;
      this.look[1] -= ax(3) * THEATER.fly.padLookDegPerS * dt;
      if (edge(4) || edge(5)) {
        this.slot = (this.slot + (edge(5) ? 1 : SLOTS - 1)) % SLOTS;
        this.refresh();
      }
      if (edge(14) || edge(15)) {
        this.brushCm = brushAfter(this.brushCm, edge(15) ? 1 : -1);
        this.refresh();
      }
      if (edge(3)) {
        const ramp = this.slots()[this.slot] ?? '';
        this.client.paintSlot(this.slot, nextRamp(this.ramps.map(([n]) => n), ramp));
        this.refresh();
      }
      if (edge(6)) this.act('fill');
      if (pressed(7)) {
        this.painting = true;
        this.act('brush');
      } else if (this.padWas[7]) {
        this.painting = false;
        this.act('end');
      }
      if (edge(1)) {
        this.padWas = pad.buttons.map((b) => b.pressed);
        this.done();
        return;
      }
      this.padWas = pad.buttons.map((b) => b.pressed);
    }
    const [yaw, pitch] = this.look;
    if (m.some((v) => v !== 0) || yaw !== 0 || pitch !== 0)
      this.client.theaterFly(m[0], m[1], m[2], yaw, pitch);
    this.look = [0, 0];
    if (this.painting && (m.some((v) => v !== 0) || yaw !== 0 || pitch !== 0)) this.act('brush');
  }
}
