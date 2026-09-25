// Touch controls for phones and tablets (iOS Safari, Android, Capacitor later): a floating
// joystick on the left half of the screen, drag-to-look on the right half, and Jump / Crouch / Run
// buttons. Built on Pointer Events, one captured pointer per control, so every control works at
// the same time with several fingers.

/** Pixels from the stick's centre to full deflection. */
export const STICK_RADIUS = 56;
const STICK_DEADZONE = 0.12;
/** Dragging past this multiple of the radius runs (the ring lights up). */
const STICK_RUN = 1.3;
/** Degrees of view rotation per pixel of drag. */
export const TOUCH_LOOK_SENSITIVITY = 0.3;

export interface StickOutput {
  moveX: number; // right
  moveY: number; // forward (dragging up the screen)
  run: boolean;
}

/** Stick deflection from a drag of (dx, dy) screen pixels: clamped to the unit circle. */
export function stickOutput(dx: number, dy: number, radius = STICK_RADIUS): StickOutput {
  const distance = Math.hypot(dx, dy);
  const run = distance >= radius * STICK_RUN;
  const magnitude = Math.min(1, distance / radius);
  if (magnitude < STICK_DEADZONE) return { moveX: 0, moveY: 0, run: false };
  // Rescale past the dead zone so small deflections still start from zero.
  const scaled = (magnitude - STICK_DEADZONE) / (1 - STICK_DEADZONE);
  return { moveX: (dx / distance) * scaled, moveY: (-dy / distance) * scaled, run };
}

/** What touch contributes to the player's input each tick. */
export interface TouchState {
  moveX: number;
  moveY: number;
  run: boolean;
  jump: boolean;
  crouch: boolean;
}

/** Receives view rotation from the look area (degrees). */
export interface LookTarget {
  yaw: number;
  pitch: number;
}

/** True on touch-first devices (phones, tablets). */
export function prefersTouch(): boolean {
  return window.matchMedia('(pointer: coarse)').matches || navigator.maxTouchPoints > 0;
}

export class TouchControls {
  readonly state: TouchState = { moveX: 0, moveY: 0, run: false, jump: false, crouch: false };
  private readonly root: HTMLDivElement;
  private readonly stickBase: HTMLDivElement;
  private readonly stickKnob: HTMLDivElement;
  private stickPointer: number | null = null;
  private stickOrigin = { x: 0, y: 0 };
  private lookPointer: number | null = null;
  private lookLast = { x: 0, y: 0 };
  private runLatched = false;
  private stickRun = false;

  constructor(
    parent: HTMLElement,
    private readonly look: LookTarget,
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
    const jump = this.holdButton('Jump', 'touch-jump', (down) => {
      this.state.jump = down;
    });
    const crouch = this.toggleButton('Crouch', 'touch-crouch', (on) => {
      this.state.crouch = on;
    });
    const run = this.toggleButton('Run', 'touch-run', (on) => {
      this.runLatched = on;
      this.state.run = this.runLatched || this.stickRun;
    });
    buttons.append(run, crouch, jump);

    this.root.append(moveZone, lookZone, this.stickBase, buttons);
    parent.append(this.root);

    moveZone.addEventListener('pointerdown', this.onStickDown);
    moveZone.addEventListener('pointermove', this.onStickMove);
    moveZone.addEventListener('pointerup', this.onStickUp);
    moveZone.addEventListener('pointercancel', this.onStickUp);
    lookZone.addEventListener('pointerdown', this.onLookDown);
    lookZone.addEventListener('pointermove', this.onLookMove);
    lookZone.addEventListener('pointerup', this.onLookUp);
    lookZone.addEventListener('pointercancel', this.onLookUp);
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

  private readonly onStickDown = (e: PointerEvent): void => {
    if (this.stickPointer !== null) return;
    this.stickPointer = e.pointerId;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    this.stickOrigin = { x: e.clientX, y: e.clientY };
    this.stickBase.hidden = false;
    this.stickBase.style.left = `${String(e.clientX)}px`;
    this.stickBase.style.top = `${String(e.clientY)}px`;
    this.updateStick(0, 0);
  };

  private readonly onStickMove = (e: PointerEvent): void => {
    if (e.pointerId !== this.stickPointer) return;
    this.updateStick(e.clientX - this.stickOrigin.x, e.clientY - this.stickOrigin.y);
  };

  private readonly onStickUp = (e: PointerEvent): void => {
    if (e.pointerId !== this.stickPointer) return;
    this.stickPointer = null;
    this.stickBase.hidden = true;
    this.updateStick(0, 0);
  };

  private updateStick(dx: number, dy: number): void {
    const out = stickOutput(dx, dy);
    this.state.moveX = out.moveX;
    this.state.moveY = out.moveY;
    this.stickRun = out.run;
    this.state.run = this.runLatched || this.stickRun;
    const distance = Math.hypot(dx, dy);
    const k = distance > STICK_RADIUS ? STICK_RADIUS / distance : 1;
    this.stickKnob.style.transform = `translate(${String(dx * k)}px, ${String(dy * k)}px)`;
    this.stickBase.classList.toggle('running', out.run);
  }

  private readonly onLookDown = (e: PointerEvent): void => {
    if (this.lookPointer !== null) return;
    this.lookPointer = e.pointerId;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    this.lookLast = { x: e.clientX, y: e.clientY };
  };

  private readonly onLookMove = (e: PointerEvent): void => {
    if (e.pointerId !== this.lookPointer) return;
    const dx = e.clientX - this.lookLast.x;
    const dy = e.clientY - this.lookLast.y;
    this.lookLast = { x: e.clientX, y: e.clientY };
    // Right-handed, Y up: dragging right turns right, which decreases yaw.
    this.look.yaw = (((this.look.yaw - dx * TOUCH_LOOK_SENSITIVITY) % 360) + 360) % 360;
    this.look.pitch = Math.min(89, Math.max(-89, this.look.pitch - dy * TOUCH_LOOK_SENSITIVITY));
  };

  private readonly onLookUp = (e: PointerEvent): void => {
    if (e.pointerId === this.lookPointer) this.lookPointer = null;
  };

  private holdButton(label: string, id: string, set: (down: boolean) => void): HTMLButtonElement {
    const b = button(label, id);
    b.addEventListener('pointerdown', (e) => {
      b.setPointerCapture(e.pointerId);
      b.classList.add('pressed');
      set(true);
    });
    const release = () => {
      b.classList.remove('pressed');
      set(false);
    };
    b.addEventListener('pointerup', release);
    b.addEventListener('pointercancel', release);
    return b;
  }

  private toggleButton(label: string, id: string, set: (on: boolean) => void): HTMLButtonElement {
    const b = button(label, id);
    b.addEventListener('pointerdown', () => {
      const on = !b.classList.contains('pressed');
      b.classList.toggle('pressed', on);
      set(on);
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

function button(label: string, id: string): HTMLButtonElement {
  const b = document.createElement('button');
  b.type = 'button';
  b.id = id;
  b.textContent = label;
  return b;
}
