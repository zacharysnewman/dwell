// Settings → Controller (docs/GAMEPAD.md): each play action with the button it is on (press the
// row, then a button on the controller, to move it), the look speed and inverted Y, and a reset to
// the data's defaults. Kept per browser (input/gamepadSettings.ts).
import { PLAY_ACTIONS, type PlayAction } from '../input/gamepad.gen';
import {
  buttonLabel,
  defaultGamepad,
  LOOK_SPEED_MAX,
  LOOK_SPEED_MIN,
  rebind,
  type GamepadSettings,
} from '../input/gamepadSettings';

/** What each action is called in the list. */
export const ACTION_LABELS: Record<PlayAction, string> = {
  jump: 'Jump (fly up)',
  crouch: 'Crouch (fly down)',
  run: 'Run',
  break: 'Break',
  place: 'Place',
  hotbarNext: 'Next block',
  hotbarPrev: 'Previous block',
  fly: 'Fly',
  flyFaster: 'Fly faster',
  flySlower: 'Fly slower',
  spyglass: 'Spyglass',
  map: 'Map',
  pause: 'Menu',
};

export interface ControllerPanelOptions {
  settings: GamepadSettings;
  /** The settings changed: apply and keep them. */
  apply: (s: GamepadSettings) => void;
  /** Hands the next button pressed to `done` (or cancels with null). */
  capture: (done: ((button: string) => void) | null) => void;
  /** A controller is connected. */
  connected: () => boolean;
}

export class ControllerPanel {
  readonly element: HTMLElement;
  private settings: GamepadSettings;
  private readonly status: HTMLParagraphElement;
  private readonly buttons = new Map<PlayAction, HTMLButtonElement>();
  private readonly speed: HTMLInputElement;
  private readonly speedValue: HTMLSpanElement;
  private readonly invert: HTMLInputElement;
  private waiting: PlayAction | null = null;

  constructor(private readonly options: ControllerPanelOptions) {
    this.settings = options.settings;
    const page = document.createElement('section');
    this.element = page;
    this.status = document.createElement('p');
    this.status.className = 'gm-lead controller-status';
    page.append(this.status);

    const look = group('Look');
    const grid = div('gm-grid');
    const speedRow = document.createElement('label');
    speedRow.className = 'settings-row';
    this.speedValue = span('settings-value');
    this.speed = document.createElement('input');
    this.speed.type = 'range';
    this.speed.min = String(LOOK_SPEED_MIN);
    this.speed.max = String(LOOK_SPEED_MAX);
    this.speed.step = '0.05';
    this.speed.addEventListener('input', () => {
      this.set({ ...this.settings, lookSpeed: Number(this.speed.value) });
    });
    speedRow.append(span('settings-name', 'Look speed'), this.speedValue, this.speed);
    const invertRow = document.createElement('label');
    invertRow.className = 'settings-row settings-toggle';
    this.invert = document.createElement('input');
    this.invert.type = 'checkbox';
    this.invert.addEventListener('change', () => {
      this.set({ ...this.settings, invertY: this.invert.checked });
    });
    invertRow.append(
      span('settings-name', 'Invert look up and down'),
      this.invert,
      span('settings-hint', 'Up on the right stick looks down'),
    );
    grid.append(speedRow, invertRow);
    look.append(grid);
    page.append(look);

    const buttonsGroup = group('Buttons');
    const list = div('control-list controller-bindings');
    for (const action of PLAY_ACTIONS) {
      const row = div('control-row');
      const name = span('control-action', ACTION_LABELS[action]);
      const b = document.createElement('button');
      b.type = 'button';
      b.className = 'settings-choice controller-binding';
      b.dataset.action = action;
      b.addEventListener('click', () => {
        this.listen(action);
      });
      this.buttons.set(action, b);
      row.append(name, b);
      list.append(row);
    }
    buttonsGroup.append(
      list,
      p(
        'settings-hint',
        'Press a row, then the button to put it on. The action on that button swaps.',
      ),
    );
    page.append(buttonsGroup);

    const reset = document.createElement('button');
    reset.type = 'button';
    reset.className = 'quiet-button settings-reset';
    reset.textContent = 'Reset';
    reset.title = 'Back to the default buttons and look';
    reset.addEventListener('click', () => {
      this.set(defaultGamepad());
    });
    const actions = div('settings-actions');
    actions.append(reset);
    page.append(actions);
    this.show();
  }

  /** Called when the page is shown and now and then while it is: the connection line. */
  refresh(): void {
    this.status.textContent = this.options.connected()
      ? 'A controller is connected. In menus: the D-pad or left stick moves, A presses, B goes back.'
      : 'No controller found. Connect one and press any button on it.';
  }

  /** The page was left: stop waiting for a button. */
  cancel(): void {
    if (this.waiting) this.options.capture(null);
    this.waiting = null;
    this.show();
  }

  private listen(action: PlayAction): void {
    this.waiting = action;
    this.show();
    this.options.capture((button) => {
      this.waiting = null;
      this.set(rebind(this.settings, action, button));
    });
  }

  private set(s: GamepadSettings): void {
    this.settings = s;
    this.show();
    this.options.apply(s);
  }

  private show(): void {
    this.speed.value = String(this.settings.lookSpeed);
    this.speedValue.textContent = `${this.settings.lookSpeed.toFixed(2)}×`;
    this.invert.checked = this.settings.invertY;
    for (const [action, b] of this.buttons) {
      const waiting = this.waiting === action;
      b.textContent = waiting ? 'Press a button…' : buttonLabel(this.settings.bindings[action]);
      b.setAttribute('aria-pressed', String(waiting));
    }
    this.refresh();
  }
}

function div(className: string): HTMLDivElement {
  const e = document.createElement('div');
  e.className = className;
  return e;
}
function span(className: string, text?: string): HTMLSpanElement {
  const e = document.createElement('span');
  e.className = className;
  if (text) e.textContent = text;
  return e;
}
function p(className: string, text: string): HTMLParagraphElement {
  const e = document.createElement('p');
  e.className = className;
  e.textContent = text;
  return e;
}
function group(title: string): HTMLDivElement {
  const g = div('gm-group');
  const h = document.createElement('h2');
  h.textContent = title;
  g.append(h);
  return g;
}
