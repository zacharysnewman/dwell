// Theater mode's flying controls on a phone (docs/THEATER_MODE.md §2): a stick at the bottom left
// (forward, back and sideways, its full tilt at theater.json `touch.stickRadiusPx`), Up and Down
// buttons beside it (held), and a drag anywhere else on the view to look. Shown while the mode is on
// a touch screen and nothing plays; above the bottom sheet's header when the sheet is collapsed.
// The mapping from the thumbs to the camera's move is `touchFly` (pure, tested).

import { THEATER, type TheaterConfig } from './config';

/** The stick's tilt: x right, y down (screen), each in [-1, 1], within the unit circle. */
export interface Stick {
  x: number;
  y: number;
}

/** The stick's tilt for a thumb `dx`, `dy` CSS px from where it touched down. */
export function stickTilt(dx: number, dy: number, c: TheaterConfig = THEATER): Stick {
  const r = c.touch.stickRadiusPx;
  const len = Math.hypot(dx, dy);
  const k = len > r ? 1 / len : 1 / r;
  return { x: dx * k, y: dy * k };
}

/**
 * The free camera's move this frame (metres along forward, right, up) from the stick and the held
 * Up and Down buttons, at `speed` m/s over `dt` s. Pushing the stick up flies forward; a full tilt
 * any way is full speed (a diagonal no faster); Up and Down at `touch.verticalFactor` of it.
 */
export function touchFly(
  stick: Stick | null,
  up: boolean,
  down: boolean,
  speed: number,
  dt: number,
  c: TheaterConfig = THEATER,
): [number, number, number] {
  const step = speed * dt;
  const s = stick ?? { x: 0, y: 0 };
  const v = (up ? 1 : 0) - (down ? 1 : 0);
  return [-s.y * step, s.x * step, v * step * c.touch.verticalFactor];
}

/** The on-screen controls: their element (`#theater-touch`) and the thumbs' state. */
export class TheaterTouch {
  readonly element: HTMLElement;
  private stickId: number | null = null;
  private origin = { x: 0, y: 0 };
  private tilt: Stick | null = null;
  private up = false;
  private down = false;
  private readonly knob: HTMLElement;

  constructor(doc: Document) {
    this.element = doc.createElement('div');
    this.element.id = 'theater-touch';
    this.element.className = 'theater-touch';
    this.element.hidden = true;
    const base = doc.createElement('div');
    base.className = 'tt-stick';
    base.setAttribute('role', 'application');
    base.setAttribute('aria-label', 'Move the camera: drag');
    this.knob = doc.createElement('div');
    this.knob.className = 'tt-knob';
    base.append(this.knob);
    base.addEventListener('pointerdown', (e) => {
      e.preventDefault();
      e.stopPropagation();
      if (this.stickId !== null) return;
      this.stickId = e.pointerId;
      const r = base.getBoundingClientRect();
      this.origin = { x: r.left + r.width / 2, y: r.top + r.height / 2 };
      this.moveStick(e.clientX, e.clientY);
      base.setPointerCapture(e.pointerId);
    });
    base.addEventListener('pointermove', (e) => {
      if (e.pointerId === this.stickId) this.moveStick(e.clientX, e.clientY);
    });
    for (const ev of ['pointerup', 'pointercancel'] as const)
      base.addEventListener(ev, (e) => {
        if (e.pointerId !== this.stickId) return;
        this.stickId = null;
        this.tilt = null;
        this.knob.style.transform = '';
      });
    const hold = (label: string, cls: string, on: (held: boolean) => void): HTMLButtonElement => {
      const b = doc.createElement('button');
      b.type = 'button';
      b.className = `tt-btn ${cls}`;
      b.textContent = label;
      b.setAttribute('aria-label', `Fly ${label.toLowerCase()}`);
      b.addEventListener('pointerdown', (e) => {
        e.preventDefault();
        e.stopPropagation();
        on(true);
      });
      for (const ev of ['pointerup', 'pointercancel', 'pointerleave'] as const)
        b.addEventListener(ev, () => {
          on(false);
        });
      return b;
    };
    const buttons = doc.createElement('div');
    buttons.className = 'tt-buttons';
    buttons.append(
      hold('Up', 'tt-up', (h) => {
        this.up = h;
      }),
      hold('Down', 'tt-down', (h) => {
        this.down = h;
      }),
    );
    this.element.append(base, buttons);
  }

  private moveStick(x: number, y: number): void {
    const t = stickTilt(x - this.origin.x, y - this.origin.y);
    this.tilt = t;
    const r = THEATER.touch.stickRadiusPx;
    this.knob.style.transform = `translate(${String(t.x * r)}px, ${String(t.y * r)}px)`;
  }

  /** Shown (a touch screen, the mode on, nothing playing) or hidden (the thumbs let go). */
  show(shown: boolean): void {
    if (this.element.hidden === !shown) return;
    this.element.hidden = !shown;
    if (!shown) {
      this.stickId = null;
      this.tilt = null;
      this.up = this.down = false;
      this.knob.style.transform = '';
    }
  }

  /** The move this frame (`touchFly`). */
  move(speed: number, dt: number): [number, number, number] {
    return touchFly(this.tilt, this.up, this.down, speed, dt);
  }
}
