// The controller's player settings (docs/GAMEPAD.md): which button each play action is on (over
// the data's defaults, shared/input/gamepad.json), the look speed and inverted Y. Kept per browser
// under their own key; the stored record is append-only (unknown fields kept on rewrite), like every
// cross-version browser store (CLAUDE.md, RELEASES.md).
import { GAMEPAD, PLAY_ACTIONS, type PlayAction } from './gamepad.gen';

export const GAMEPAD_KEY = 'dwell.gamepad';
export const LOOK_SPEED_MIN = 0.25;
export const LOOK_SPEED_MAX = 3;

export interface GamepadSettings {
  /** Each play action's button (by name). */
  bindings: Record<PlayAction, string>;
  /** × the data's look turn. */
  lookSpeed: number;
  invertY: boolean;
}

const BUTTON_NAMES: readonly string[] = GAMEPAD.buttons.map((b) => b.name);

export function defaultGamepad(): GamepadSettings {
  return { bindings: { ...GAMEPAD.play }, lookSpeed: 1, invertY: false };
}

/** A button's label ("A", "D-pad up"), or the name itself for one this build doesn't know. */
export function buttonLabel(name: string): string {
  return GAMEPAD.buttons.find((b) => b.name === name)?.label ?? name;
}

/** The button name at a standard-mapping index, if the data has one there. */
export function buttonAt(index: number): string | undefined {
  return GAMEPAD.buttons.find((b) => b.index === index)?.name;
}

/**
 * Puts `action` on `button`: the action that had it takes `action`'s old button, so no two share
 * one (the engine's `Gamepad::bind` does the same).
 */
export function rebind(s: GamepadSettings, action: PlayAction, button: string): GamepadSettings {
  const bindings = { ...s.bindings };
  const old = bindings[action];
  const other = PLAY_ACTIONS.find((a) => bindings[a] === button);
  if (other) bindings[other] = old;
  bindings[action] = button;
  return { ...s, bindings };
}

/**
 * Stored settings into range: an unknown action or button is dropped, a missing one takes its
 * default, and two actions on one button are resolved by keeping the first and giving the second
 * its default (or, if that is taken too, any free button).
 */
export function sanitizeGamepad(value: unknown): GamepadSettings {
  const d = defaultGamepad();
  if (typeof value !== 'object' || value === null || Array.isArray(value)) return d;
  const v = value as Record<string, unknown>;
  const speed = v.lookSpeed;
  const lookSpeed =
    typeof speed === 'number' && Number.isFinite(speed)
      ? Math.min(LOOK_SPEED_MAX, Math.max(LOOK_SPEED_MIN, speed))
      : d.lookSpeed;
  const stored =
    typeof v.bindings === 'object' && v.bindings !== null
      ? (v.bindings as Record<string, unknown>)
      : {};
  const bindings = { ...d.bindings };
  const used = new Set<string>();
  const wanted = (a: PlayAction): string => {
    const b = stored[a];
    return typeof b === 'string' && BUTTON_NAMES.includes(b) ? b : d.bindings[a];
  };
  for (const a of PLAY_ACTIONS) {
    let b = wanted(a);
    if (used.has(b)) b = used.has(d.bindings[a]) ? '' : d.bindings[a];
    if (b === '') b = BUTTON_NAMES.find((n) => !used.has(n)) ?? d.bindings[a];
    bindings[a] = b;
    used.add(b);
  }
  return { bindings, lookSpeed, invertY: typeof v.invertY === 'boolean' ? v.invertY : d.invertY };
}

/** The record to store: the stored one's other fields kept (newer builds' fields survive). */
export function gamepadRecord(stored: unknown, s: GamepadSettings): Record<string, unknown> {
  const base =
    typeof stored === 'object' && stored !== null && !Array.isArray(stored)
      ? (stored as Record<string, unknown>)
      : {};
  const oldBindings =
    typeof base.bindings === 'object' && base.bindings !== null && !Array.isArray(base.bindings)
      ? (base.bindings as Record<string, unknown>)
      : {};
  return {
    ...base,
    bindings: { ...oldBindings, ...s.bindings },
    lookSpeed: s.lookSpeed,
    invertY: s.invertY,
  };
}

function storage(): Storage | null {
  try {
    return globalThis.localStorage;
  } catch {
    return null;
  }
}

function loadRaw(): unknown {
  try {
    const raw = storage()?.getItem(GAMEPAD_KEY);
    return raw ? (JSON.parse(raw) as unknown) : null;
  } catch {
    return null;
  }
}

export function loadGamepadSettings(): GamepadSettings {
  return sanitizeGamepad(loadRaw());
}

export function saveGamepadSettings(s: GamepadSettings): void {
  try {
    storage()?.setItem(GAMEPAD_KEY, JSON.stringify(gamepadRecord(loadRaw(), s)));
  } catch {
    // Not kept: they still apply for this visit.
  }
}
