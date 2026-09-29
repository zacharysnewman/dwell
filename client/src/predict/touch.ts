// Touch controls for phones and tablets (iOS Safari, Android, Capacitor later): a floating
// joystick on the left half of the screen, drag-to-look on the right half, and Jump / Crouch / Run
// / Fly buttons. Built on Pointer Events, one captured pointer per control, so every control works at
// the same time with several fingers.

/** Pixels from the stick's centre to full deflection. */
export const STICK_RADIUS = 56;
const STICK_DEADZONE = 0.12;
/** Dragging past this multiple of the radius runs (the ring lights up). */
const STICK_RUN = 1.3;
/** Degrees of view rotation per pixel of drag. */
export const TOUCH_LOOK_SENSITIVITY = 0.3;
/** A touch on the view that moves less than this (px) and lifts within TAP_MS is a tap (§6.5). */
export const TAP_SLOP = 12;
export const TAP_MS = 500;

/** Is a touch that moved (dx, dy) px in total and lasted `ms` a tap rather than a look drag? */
export function isTap(dx: number, dy: number, ms: number): boolean {
  return Math.hypot(dx, dy) <= TAP_SLOP && ms <= TAP_MS;
}

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

/** How a touch button reports its state: pressed while held, or latched on/off per tap. */
export type TouchButtonMode = 'hold' | 'toggle';

/** The on-screen buttons, left to right. */
export const TOUCH_BUTTONS = {
  // What a tap on the view does: Break, or (latched) Place (§6.5).
  edit: { label: 'Break', id: 'touch-edit', mode: 'toggle' },
  run: { label: 'Run', id: 'touch-run', mode: 'toggle' },
  // Creative flight on/off (the flight toggle holds the state; the button shows it).
  fly: { label: 'Fly', id: 'touch-fly', mode: 'hold' },
  // The F3 debug overlay (phones have no function keys); top right, clear of the hotbar.
  debug: { label: 'i', id: 'touch-debug', mode: 'hold' },
  crouch: { label: 'Crouch', id: 'touch-crouch', mode: 'hold' },
  jump: { label: 'Jump', id: 'touch-jump', mode: 'hold' },
} as const satisfies Record<string, { label: string; id: string; mode: TouchButtonMode }>;

/** A button's on/off state as pointers press and release it. */
export class TouchButtonState {
  active = false;
  constructor(readonly mode: TouchButtonMode) {}

  press(): boolean {
    this.active = this.mode === 'hold' ? true : !this.active;
    return this.active;
  }

  release(): boolean {
    if (this.mode === 'hold') this.active = false;
    return this.active;
  }
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
  private lookStart = { x: 0, y: 0, t: 0 };
  /** A tap on the view: break or place at the crosshair (§6.5). */
  onTap: (() => void) | null = null;
  /** The Break/Place toggle changed: true = place. */
  onPlaceMode: ((place: boolean) => void) | null = null;
  /** The Fly button, or Jump (a double tap toggles flight, as Space does), was pressed. */
  onFly: (() => void) | null = null;
  /** The debug button was pressed (toggles the overlay, as F3 does). */
  onDebug: (() => void) | null = null;
  onJumpPress: ((nowMs: number) => void) | null = null;
  private readonly flyButton: HTMLButtonElement;
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
    const run = touchButton(TOUCH_BUTTONS.run, (on) => {
      this.runLatched = on;
      this.state.run = this.runLatched || this.stickRun;
    });
    const crouch = touchButton(TOUCH_BUTTONS.crouch, (on) => {
      this.state.crouch = on;
    });
    const jump = touchButton(TOUCH_BUTTONS.jump, (on) => {
      if (on && !this.state.jump) this.onJumpPress?.(performance.now());
      this.state.jump = on;
    });
    this.flyButton = touchButton(TOUCH_BUTTONS.fly, (on) => {
      if (on) this.onFly?.();
    });
    const edit = touchButton(TOUCH_BUTTONS.edit, (place) => {
      edit.textContent = place ? 'Place' : 'Break';
      this.onPlaceMode?.(place);
    });
    buttons.append(edit, this.flyButton, run, crouch, jump);
    const debug = touchButton(TOUCH_BUTTONS.debug, (on) => {
      if (on) this.onDebug?.();
    });
    debug.setAttribute('aria-label', 'Debug overlay');

    this.root.append(moveZone, lookZone, this.stickBase, buttons, debug);
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

  /** Shows whether the player is flying, and hides the Fly button where flight is not allowed. */
  setFlight(allowed: boolean, flying: boolean): void {
    this.flyButton.hidden = !allowed;
    this.flyButton.classList.toggle('flying', flying);
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
    this.lookStart = { x: e.clientX, y: e.clientY, t: e.timeStamp };
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
    if (e.pointerId !== this.lookPointer) return;
    this.lookPointer = null;
    const s = this.lookStart;
    // Travel up to the last move (a lifted touch may not report where it ended).
    const l = this.lookLast;
    if (e.type === 'pointerup' && isTap(l.x - s.x, l.y - s.y, e.timeStamp - s.t)) {
      this.onTap?.();
    }
  };
}

function zone(id: string): HTMLDivElement {
  const z = document.createElement('div');
  z.id = id;
  z.className = 'touch-zone';
  return z;
}

function touchButton(
  spec: { label: string; id: string; mode: TouchButtonMode },
  set: (on: boolean) => void,
): HTMLButtonElement {
  const b = document.createElement('button');
  b.type = 'button';
  b.id = spec.id;
  b.textContent = spec.label;
  const state = new TouchButtonState(spec.mode);
  const apply = (on: boolean) => {
    b.classList.toggle('pressed', on);
    set(on);
  };
  b.addEventListener('pointerdown', (e) => {
    b.setPointerCapture(e.pointerId);
    apply(state.press());
  });
  const release = () => {
    apply(state.release());
  };
  b.addEventListener('pointerup', release);
  b.addEventListener('pointercancel', release);
  return b;
}
