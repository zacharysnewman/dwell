// Input sampling and quantization (PLAYER_CONTROLLER.md §8.4). The client quantizes each tick's
// input exactly once; the server and the client's own prediction both dequantize the same
// integers (C++ DequantizeInput), so they simulate identical inputs.
import { InputButtons } from '../protocol/constants.gen';
import type { TouchState } from './touch';
import type { InputFrame } from '../protocol/messages';

/** One tick of player input before quantization. */
export interface PlayerInputState {
  moveX: number; // right
  moveY: number; // forward
  yaw: number; // degrees; 0 = +Z, 90 = +X
  pitch: number; // degrees, positive up
  jump: boolean;
  run: boolean;
  crouch: boolean;
}

export const IDLE_INPUT: PlayerInputState = {
  moveX: 0,
  moveY: 0,
  yaw: 0,
  pitch: 0,
  jump: false,
  run: false,
  crouch: false,
};

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** Wraps an integer to int16. */
function int16(v: number): number {
  const u = ((v % 65536) + 65536) % 65536;
  return u >= 32768 ? u - 65536 : u;
}

export function quantizeYaw(degrees: number): number {
  return int16(Math.round((degrees / 360) * 65536));
}

export function quantizePitch(degrees: number): number {
  return Math.round((clamp(degrees, -90, 90) / 90) * 32767);
}

/** Mirrors C++ player::QuantizeInput: move clamped to the unit circle (the server rejects more). */
export function quantizeInput(input: PlayerInputState, seq: number): InputFrame {
  let { moveX, moveY } = input;
  const length = Math.hypot(moveX, moveY);
  if (length > 1) {
    moveX /= length;
    moveY /= length;
  }
  return {
    seq,
    moveX: Math.round(clamp(moveX, -1, 1) * 127),
    moveY: Math.round(clamp(moveY, -1, 1) * 127),
    buttons:
      (input.jump ? InputButtons.jump : 0) |
      (input.run ? InputButtons.run : 0) |
      (input.crouch ? InputButtons.crouch : 0),
    yaw: quantizeYaw(input.yaw),
    pitch: quantizePitch(input.pitch),
  };
}

export function dequantizeYaw(q: number): number {
  return (q * 360) / 65536;
}

export function dequantizePitch(q: number): number {
  return (q * 90) / 32767;
}

/** Keyboard + pointer-lock mouse look. WASD move, Space jump, Shift run, C / Ctrl crouch. */
export class KeyboardMouseInput {
  private readonly keys = new Set<string>();
  yaw = 0;
  pitch = 0;
  /** Degrees per pixel of mouse movement. */
  sensitivity = 0.12;
  /** Debug toggles: F3 overlay. */
  onToggle: ((key: string) => void) | null = null;
  /** On-screen touch controls, merged into every sample (predict/touch.ts). */
  touch: TouchState | null = null;

  constructor(private readonly target: HTMLElement) {
    window.addEventListener('keydown', this.onKeyDown);
    window.addEventListener('keyup', this.onKeyUp);
    window.addEventListener('blur', this.onBlur);
    target.addEventListener('click', this.onClick);
    document.addEventListener('mousemove', this.onMouseMove);
  }

  dispose(): void {
    window.removeEventListener('keydown', this.onKeyDown);
    window.removeEventListener('keyup', this.onKeyUp);
    window.removeEventListener('blur', this.onBlur);
    this.target.removeEventListener('click', this.onClick);
    document.removeEventListener('mousemove', this.onMouseMove);
  }

  /** Presses or releases a key programmatically (tests and automation). */
  setKey(code: string, down: boolean): void {
    if (down) this.keys.add(code);
    else this.keys.delete(code);
  }

  sample(): PlayerInputState {
    const k = (code: string) => this.keys.has(code);
    const t = this.touch;
    return {
      moveX: clamp((k('KeyD') ? 1 : 0) - (k('KeyA') ? 1 : 0) + (t?.moveX ?? 0), -1, 1),
      moveY: clamp((k('KeyW') ? 1 : 0) - (k('KeyS') ? 1 : 0) + (t?.moveY ?? 0), -1, 1),
      yaw: this.yaw,
      pitch: this.pitch,
      jump: k('Space') || (t?.jump ?? false),
      run: k('ShiftLeft') || k('ShiftRight') || (t?.run ?? false),
      crouch: k('KeyC') || k('ControlLeft') || (t?.crouch ?? false),
    };
  }

  private readonly onKeyDown = (e: KeyboardEvent): void => {
    if (e.code === 'F3') {
      e.preventDefault();
      this.onToggle?.(e.code);
      return;
    }
    this.keys.add(e.code);
    if (e.code === 'Space' || e.code.startsWith('Arrow')) e.preventDefault();
  };

  private readonly onKeyUp = (e: KeyboardEvent): void => {
    this.keys.delete(e.code);
  };

  private readonly onBlur = (): void => {
    this.keys.clear();
  };

  private readonly onClick = (): void => {
    // Not every browser has pointer lock (iOS Safari doesn't); touch looks by dragging instead.
    if (!('requestPointerLock' in this.target)) return;
    if (document.pointerLockElement !== this.target) {
      Promise.resolve(this.target.requestPointerLock()).catch(() => undefined);
    }
  };

  private readonly onMouseMove = (e: MouseEvent): void => {
    if (document.pointerLockElement !== this.target) return;
    // Right-handed, Y up: facing +Z (yaw 0), right is −X, so turning right decreases yaw.
    this.yaw = (((this.yaw - e.movementX * this.sensitivity) % 360) + 360) % 360;
    this.pitch = clamp(this.pitch - e.movementY * this.sensitivity, -89, 89);
  };
}
