// The controller, polled every animation frame (docs/GAMEPAD.md): the first connected gamepad's
// standard-mapping buttons and axes go to the WASM client (play: crates/dwell-game/src/gamepad.rs)
// and, while a menu is open, to the menu's focus (gamepadMenu.ts). Settings → Controller rebinds
// through `capture`: the next button pressed is handed to it instead.
import { buttonAt } from './gamepadSettings';
import { GamepadMenu, menuHeld } from './gamepadMenu';

/** What the poller drives: the client (none on the main menu page) and the menu. */
export interface GamepadHost {
  client: {
    gamepad(buttons: Float32Array, axes: Float32Array, nowMs: number): string[];
    gamepadDisconnected(): void;
  } | null;
  /** A menu is open (the game menu, or the main menu page). */
  menuOpen(): boolean;
  openMenu(): void;
  closeMenu(): void;
  /** B in a menu: back, as Esc. */
  back(): void;
}

/** The pad to read: the first connected one, a standard-mapped one first. */
export function pickGamepad(pads: readonly (Gamepad | null)[]): Gamepad | null {
  const live = pads.filter((p): p is Gamepad => p !== null && p.connected);
  return live.find((p) => p.mapping === 'standard') ?? live[0] ?? null;
}

export class GamepadInput {
  private readonly menu: GamepadMenu;
  private connected = false;
  private down: boolean[] = [];
  private wasMenu = false;
  /** While set, the next newly pressed button's name goes here (and does nothing else). */
  capture: ((button: string) => void) | null = null;

  constructor(
    private readonly host: GamepadHost,
    doc: Document = document,
  ) {
    this.menu = new GamepadMenu(
      doc,
      () => {
        host.back();
      },
      () => {
        host.closeMenu();
      },
    );
  }

  /** True once a controller has been read (Settings shows it). */
  get active(): boolean {
    return this.connected;
  }

  poll(now: number): void {
    const pads = typeof navigator.getGamepads === 'function' ? navigator.getGamepads() : [];
    const pad = pickGamepad(pads);
    if (!pad) {
      if (this.connected) this.host.client?.gamepadDisconnected();
      this.connected = false;
      this.down = [];
      return;
    }
    this.connected = true;
    const values = pad.buttons.map((b) => (b.pressed ? Math.max(b.value, 1) : b.value));
    const down = values.map((v) => v >= 0.5);
    const was = this.down;
    this.down = down;
    const buttons = Float32Array.from(values);
    const axes = Float32Array.from(pad.axes);
    if (this.capture) {
      const i = down.findIndex((d, k) => d && !was[k]);
      const name = i >= 0 ? buttonAt(i) : undefined;
      if (name) {
        const done = this.capture;
        this.capture = null;
        done(name);
      }
      // Tracked, so the press isn't also an action once the capture ends.
      this.menu.reset(menuHeld(values, pad.axes));
      this.host.client?.gamepad(buttons, axes, now);
      return;
    }
    const menuOpen = this.host.menuOpen();
    if (menuOpen) {
      // Opened this frame: what is held already isn't pressed in it.
      if (!this.wasMenu) this.menu.reset(menuHeld(values, pad.axes));
      this.menu.frame(values, pad.axes, now);
    }
    this.wasMenu = menuOpen;
    const shell = this.host.client?.gamepad(buttons, axes, now) ?? [];
    if (!menuOpen && shell.includes('pause')) this.host.openMenu();
  }
}

/** The main menu page's controller (no game): menus only, on its own animation frames. */
export function startMenuGamepad(doc: Document, back: () => void): void {
  const input = new GamepadInput(
    {
      client: null,
      menuOpen: () => true,
      openMenu: () => undefined,
      closeMenu: () => undefined,
      back,
    },
    doc,
  );
  const frame = (now: number): void => {
    input.poll(now);
    requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);
}
