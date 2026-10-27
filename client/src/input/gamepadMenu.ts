// Menus by controller (docs/GAMEPAD.md): while a menu is open, the D-pad or the left stick moves
// the focus to the nearest control that way, A presses it, B goes back (as Esc does) and Menu
// closes the game menu; left and right step a slider or a choice row. The picking and the repeat
// are plain functions, tested; the DOM part is thin.
import { GAMEPAD, type MenuAction } from './gamepad.gen';

export type Direction = 'up' | 'down' | 'left' | 'right';

export interface Rect {
  left: number;
  top: number;
  width: number;
  height: number;
}

const centre = (r: Rect): [number, number] => [r.left + r.width / 2, r.top + r.height / 2];

/**
 * The index of the rect nearest `from` in `dir` (its centre past `from`'s that way), by the
 * distance along `dir` plus twice the distance across it, so a control straight ahead wins over a
 * nearer one off to the side; -1 if none lies that way.
 */
export function nearestToward(from: Rect, rects: readonly Rect[], dir: Direction): number {
  const [fx, fy] = centre(from);
  let best = -1;
  let bestScore = Infinity;
  rects.forEach((r, i) => {
    const [x, y] = centre(r);
    const along = { up: fy - y, down: y - fy, left: fx - x, right: x - fx }[dir];
    const across = dir === 'up' || dir === 'down' ? Math.abs(x - fx) : Math.abs(y - fy);
    if (along <= 1) return;
    const score = along + 2 * across;
    if (score < bestScore) {
      bestScore = score;
      best = i;
    }
  });
  return best;
}

/** A held direction's repeat: true on the press, then after the delay every `every` ms. */
export class Repeat {
  private since: number | null = null;
  private last = 0;
  constructor(
    private readonly delayMs = GAMEPAD.tunables.menuRepeatDelayMs,
    private readonly everyMs = GAMEPAD.tunables.menuRepeatMs,
  ) {}

  step(down: boolean, now: number): boolean {
    if (!down) {
      this.since = null;
      return false;
    }
    if (this.since === null) {
      this.since = now;
      this.last = now;
      return true;
    }
    if (now - this.since >= this.delayMs && now - this.last >= this.everyMs) {
      this.last = now;
      return true;
    }
    return false;
  }
}

/** One frame of a controller as the menu reads it: its menu actions held, stick included. */
export function menuHeld(
  buttons: readonly number[],
  axes: readonly number[],
): Record<MenuAction, boolean> {
  const t = GAMEPAD.tunables;
  const index = (name: string) => GAMEPAD.buttons.find((b) => b.name === name)?.index ?? -1;
  const pressed = (name: string) => (buttons[index(name)] ?? 0) >= t.triggerThreshold;
  const x = axes[GAMEPAD.sticks.moveX] ?? 0;
  const y = axes[GAMEPAD.sticks.moveY] ?? 0;
  const tilt = 0.5;
  const m = GAMEPAD.menu;
  return {
    up: pressed(m.up) || y < -tilt,
    down: pressed(m.down) || y > tilt,
    left: pressed(m.left) || x < -tilt,
    right: pressed(m.right) || x > tilt,
    confirm: pressed(m.confirm),
    back: pressed(m.back),
    close: pressed(m.close),
  };
}

const FOCUSABLE =
  'button:not([disabled]), input:not([disabled]), select:not([disabled]), a[href], [tabindex]:not([tabindex="-1"])';

/** The controls a controller can reach: the open modal dialog's, else the page's; shown only. */
export function reachable(doc: Document): HTMLElement[] {
  const dialogs = [...doc.querySelectorAll<HTMLElement>('[aria-modal="true"]')].filter(
    (d) => !d.hidden && d.offsetParent !== null,
  );
  const root: ParentNode = dialogs.at(-1) ?? doc;
  return [...root.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(
    (e) => e.offsetParent !== null && !e.closest('[hidden]'),
  );
}

/** Steps a slider or a choice row left or right; true if `e` is one. */
export function stepControl(e: HTMLElement, dir: 'left' | 'right'): boolean {
  if (e instanceof HTMLInputElement && e.type === 'range') {
    if (dir === 'left') e.stepDown();
    else e.stepUp();
    e.dispatchEvent(new Event('input', { bubbles: true }));
    e.dispatchEvent(new Event('change', { bubbles: true }));
    return true;
  }
  if (e instanceof HTMLSelectElement) {
    const n = e.options.length;
    if (n === 0) return true;
    e.selectedIndex = (e.selectedIndex + (dir === 'left' ? n - 1 : 1)) % n;
    e.dispatchEvent(new Event('change', { bubbles: true }));
    return true;
  }
  return false;
}

/** The menu side of the controller: call `frame` every animation frame while a menu is open. */
export class GamepadMenu {
  private readonly repeats: Record<Direction, Repeat> = {
    up: new Repeat(),
    down: new Repeat(),
    left: new Repeat(),
    right: new Repeat(),
  };
  private was: Record<MenuAction, boolean> | null = null;

  constructor(
    private readonly doc: Document,
    /** B: back (the shell's Esc). */
    private readonly onBack: () => void,
    /** Menu: close the game menu. */
    private readonly onClose: () => void,
  ) {}

  /** Forgets what was held (a menu just opened with a button down doesn't press it). */
  reset(held: Record<MenuAction, boolean> | null): void {
    this.was = held;
  }

  frame(buttons: readonly number[], axes: readonly number[], now: number): void {
    const held = menuHeld(buttons, axes);
    const was = this.was;
    this.was = held;
    const press = (a: MenuAction) => held[a] && was !== null && !was[a];
    const controls = reachable(this.doc);
    const active = this.doc.activeElement as HTMLElement | null;
    const focused = active && controls.includes(active) ? active : null;
    for (const dir of ['up', 'down', 'left', 'right'] as const) {
      if (!this.repeats[dir].step(held[dir], now) || was === null) continue;
      if (!focused) {
        controls[0]?.focus();
        continue;
      }
      if ((dir === 'left' || dir === 'right') && stepControl(focused, dir)) continue;
      const others = controls.filter((c) => c !== focused);
      const i = nearestToward(
        focused.getBoundingClientRect(),
        others.map((c) => c.getBoundingClientRect()),
        dir,
      );
      const next = others[i];
      if (next) {
        next.focus();
        next.scrollIntoView({ block: 'nearest' });
      }
    }
    if (press('confirm')) (focused ?? controls[0])?.click();
    if (press('back')) this.onBack();
    if (press('close')) this.onClose();
  }
}
