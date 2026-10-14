// Touch controls for phones and tablets (iOS Safari, Android, Capacitor): a floating joystick on the
// left half of the screen, drag-to-look on the right half, and Break/Place, Fly, Run, Crouch and
// Jump buttons. The DOM is here; what the pointers mean (stick deflection, taps, latched buttons)
// is the Rust client's (dwell-game's touch.rs), which these elements forward their events to.
import type { Client } from '../engine/wasm';

/** The on-screen buttons (touch.rs `TouchButton`), left to right; `debug` sits top right. */
const BUTTONS = [
  { name: 'edit', label: 'Break', id: 'touch-edit' },
  { name: 'fly', label: 'Fly', id: 'touch-fly' },
  { name: 'run', label: 'Run', id: 'touch-run' },
  { name: 'crouch', label: 'Crouch', id: 'touch-crouch' },
  { name: 'jump', label: 'Jump', id: 'touch-jump' },
] as const;

/** True on touch-first devices (phones, tablets). */
export function prefersTouch(): boolean {
  return window.matchMedia('(pointer: coarse)').matches || navigator.maxTouchPoints > 0;
}

export class TouchControls {
  private readonly root: HTMLDivElement;
  private readonly stickBase: HTMLDivElement;
  private readonly stickKnob: HTMLDivElement;
  private readonly flyButton: HTMLButtonElement;

  constructor(
    parent: HTMLElement,
    private readonly client: Client,
  ) {
    this.root = document.createElement('div');
    this.root.id = 'touch-controls';
    const moveZone = zone('touch-move');
    const lookZone = zone('touch-look');
    this.stickBase = document.createElement('div');
    this.stickBase.className = 'stick';
    this.stickBase.hidden = true;
    this.stickKnob = document.createElement('div');
    this.stickKnob.className = 'stick-knob';
    this.stickBase.append(this.stickKnob);

    const buttons = document.createElement('div');
    buttons.className = 'touch-buttons';
    const made = BUTTONS.map((spec) => this.button(spec.name, spec.label, spec.id));
    this.flyButton = made[1] as HTMLButtonElement;
    buttons.append(...made);
    const debug = this.button('debug', 'i', 'touch-debug');
    debug.setAttribute('aria-label', 'Debug overlay');
    this.root.append(moveZone, lookZone, this.stickBase, buttons, debug);
    parent.append(this.root);

    moveZone.addEventListener('pointerdown', (e) => {
      (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
      client.stickDown(e.pointerId, e.clientX, e.clientY);
      this.drawStick();
    });
    moveZone.addEventListener('pointermove', (e) => {
      client.stickMove(e.pointerId, e.clientX, e.clientY);
      this.drawStick();
    });
    const stickUp = (e: PointerEvent) => {
      client.stickUp(e.pointerId);
      this.drawStick();
    };
    moveZone.addEventListener('pointerup', stickUp);
    moveZone.addEventListener('pointercancel', stickUp);
    lookZone.addEventListener('pointerdown', (e) => {
      (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
      client.lookDown(e.pointerId, e.clientX, e.clientY, e.timeStamp);
    });
    lookZone.addEventListener('pointermove', (e) => {
      client.lookMove(e.pointerId, e.clientX, e.clientY);
    });
    lookZone.addEventListener('pointerup', (e) => {
      client.lookUp(e.pointerId, e.timeStamp, false, performance.now());
    });
    lookZone.addEventListener('pointercancel', (e) => {
      client.lookUp(e.pointerId, e.timeStamp, true, performance.now());
    });
    // iOS Safari: no page scrolling, rubber-banding, double-tap zoom, or callout while playing.
    for (const type of ['touchstart', 'touchmove', 'gesturestart', 'contextmenu'] as const) {
      this.root.addEventListener(
        type,
        (e) => {
          e.preventDefault();
        },
        { passive: false },
      );
    }
  }

  set visible(v: boolean) {
    this.root.hidden = !v;
  }

  /** Shows whether the player is flying, and hides the Fly button where flight is not allowed. */
  setFlight(allowed: boolean, flying: boolean): void {
    this.flyButton.hidden = !allowed;
    this.flyButton.classList.toggle('flying', flying);
  }

  private drawStick(): void {
    const v = this.client.stickView();
    if (v.length < 5) {
      this.stickBase.hidden = true;
      return;
    }
    const [x, y, kx, ky, running] = v as unknown as [number, number, number, number, number];
    this.stickBase.hidden = false;
    this.stickBase.style.left = `${String(x)}px`;
    this.stickBase.style.top = `${String(y)}px`;
    this.stickKnob.style.transform = `translate(${String(kx)}px, ${String(ky)}px)`;
    this.stickBase.classList.toggle('running', running === 1);
  }

  private button(name: string, label: string, id: string): HTMLButtonElement {
    const b = document.createElement('button');
    b.type = 'button';
    b.id = id;
    b.textContent = label;
    const apply = (down: boolean) => {
      const on = this.client.touchButton(name, down, performance.now());
      b.classList.toggle('pressed', on);
      if (name === 'edit') b.textContent = on ? 'Place' : 'Break';
    };
    b.addEventListener('pointerdown', (e) => {
      b.setPointerCapture(e.pointerId);
      apply(true);
    });
    b.addEventListener('pointerup', () => {
      apply(false);
    });
    b.addEventListener('pointercancel', () => {
      apply(false);
    });
    return b;
  }
}

function zone(id: string): HTMLDivElement {
  const z = document.createElement('div');
  z.id = id;
  z.className = 'touch-zone';
  return z;
}
