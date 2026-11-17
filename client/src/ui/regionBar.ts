// The touch region bar (docs/STRUCTURES.md §5.1): the region tools (dwell-game region_tools.rs) on a
// phone. A Select button, shown while the data's `showWhen` holds (flying in creative), opens a bar
// of buttons from shared/input/touchRegion.json: corners, copy, paste mode (place, rotate, mirror,
// cancel), undo, save as a structure and clear. Each runs a region tool by name (`Client.tool`) or a
// shell action; no edit logic lives here. Owner, 2026-11-15: "I'd love to copy a structure in mobile
// but there's no selection or copy control for it."
import data from '../../../shared/input/touchRegion.json';
import { ABILITIES } from './modes.gen';

/** The region tools `Client.tool` runs by name (crates/dwell-client/src/web/app.rs `tool`). */
export const TOOL_NAMES = [
  'cornerA',
  'cornerB',
  'deselect',
  'fill',
  'hollow',
  'replace',
  'clear',
  'copy',
  'pasteMode',
  'rotate',
  'mirror',
  'paste',
  'undo',
  'redo',
] as const;
/** The shell's own actions: `save` opens the Structures page's save with the selection. */
export const SHELL_ACTIONS = ['save'] as const;
/** The bar's states: choosing a box, or paste mode on. */
export const BAR_STATES = ['select', 'paste'] as const;
export type BarState = (typeof BAR_STATES)[number];

export interface RegionButton {
  id: string;
  label: string;
  run: string;
  in: BarState[];
}

export interface RegionBarData {
  showWhen: { flying: boolean; abilities: string[] };
  open: { id: string; label: string };
  buttons: RegionButton[];
}

const isObject = (v: unknown): v is Record<string, unknown> =>
  typeof v === 'object' && v !== null && !Array.isArray(v);
const keysOf = (o: Record<string, unknown>) => Object.keys(o).filter((k) => k !== '$comment');
const fail = (m: string): never => {
  throw new Error(`touchRegion.json: ${m}`);
};

/** Checks the bar's data; returns it typed. Throws naming the entry. */
export function validateRegionBar(raw: unknown): RegionBarData {
  if (!isObject(raw)) fail('must be an object');
  const d = raw as Record<string, unknown>;
  const extra = keysOf(d).filter((k) => !['showWhen', 'open', 'buttons'].includes(k));
  if (extra.length) fail(`unknown ${extra.join(', ')}`);
  const when = d.showWhen;
  if (!isObject(when)) return fail('showWhen must be an object');
  if (typeof when.flying !== 'boolean') fail('showWhen.flying must be true or false');
  if (!Array.isArray(when.abilities)) fail('showWhen.abilities must be a list');
  for (const a of when.abilities as unknown[]) {
    if (!(ABILITIES as readonly unknown[]).includes(a))
      fail(`showWhen.abilities: unknown ability ${JSON.stringify(a)}`);
  }
  const open = d.open;
  if (
    !isObject(open) ||
    typeof open.id !== 'string' ||
    typeof open.label !== 'string' ||
    !open.label
  )
    fail('open needs an id and a label');
  if (!Array.isArray(d.buttons) || d.buttons.length === 0) fail('buttons must be a list');
  const ids = new Set<string>([(open as { id: string }).id]);
  (d.buttons as unknown[]).forEach((b, i) => {
    const at = `buttons[${String(i)}]`;
    if (!isObject(b)) return fail(`${at} must be an object`);
    if (typeof b.id !== 'string' || !b.id) fail(`${at}.id must be a name`);
    if (ids.has(b.id as string)) fail(`${at}: id ${String(b.id)} is used twice`);
    ids.add(b.id as string);
    if (typeof b.label !== 'string' || !b.label) fail(`${at}.label must be text`);
    const run = b.run;
    const tool = typeof run === 'string' && run.startsWith('tool:') ? run.slice(5) : null;
    const known =
      tool !== null
        ? (TOOL_NAMES as readonly string[]).includes(tool)
        : (SHELL_ACTIONS as readonly unknown[]).includes(run);
    if (!known) fail(`${at}.run: unknown action ${JSON.stringify(run)}`);
    if (!Array.isArray(b.in) || b.in.length === 0) fail(`${at}.in must list states`);
    for (const s of b.in as unknown[]) {
      if (!(BAR_STATES as readonly unknown[]).includes(s))
        fail(`${at}.in: unknown state ${JSON.stringify(s)}`);
    }
  });
  return d as unknown as RegionBarData;
}

export const REGION_BAR: RegionBarData = validateRegionBar(data);

/** Whether the Select button shows: the data's rule over the player's flight and the mode. */
export function regionBarShown(
  rule: RegionBarData['showWhen'],
  now: { flying: boolean; enabled: boolean; allows: (ability: string) => boolean },
): boolean {
  if (!now.enabled) return false;
  if (rule.flying && !now.flying) return false;
  return rule.abilities.every((a) => now.allows(a));
}

/** The bar's state from `Client.regionState` (`[enabled, pasting, selected]`). */
export function barStateOf(st: ArrayLike<number>): BarState {
  return st.length >= 2 && st[1] === 1 ? 'paste' : 'select';
}

/** The tools' line (RegionTools::status) for the bar: without the keyboard's hints, which the
 * bar's buttons stand for on a phone. */
export function touchStatus(line: string): string {
  return line
    .split(' · ')
    .filter((p) => !/^(R rotate|M mirror|right click paste)$/.test(p))
    .join(' · ')
    .replace(/ \((B, N|Ctrl\+C)\)/g, '');
}

/** The buttons the bar shows in a state. */
export function buttonsIn(bar: RegionBarData, state: BarState): RegionButton[] {
  return bar.buttons.filter((b) => b.in.includes(state));
}

/** What the bar asks of the game (the wasm Client's region tools) and the shell. */
export interface RegionBarDeps {
  flying(): boolean;
  allows(ability: string): boolean;
  /** `[enabled, pasting, selected]` (Client.regionState); empty before a game. */
  state(): ArrayLike<number>;
  status(): string;
  tool(name: string): void;
  armed(open: boolean): void;
  /** A shell action (`save`). */
  action(name: string): void;
}

export class RegionBar {
  readonly openButton: HTMLButtonElement;
  readonly bar: HTMLDivElement;
  private readonly statusLine: HTMLDivElement;
  private readonly made: { spec: RegionButton; el: HTMLButtonElement }[];
  private open = false;
  private timer: ReturnType<typeof setInterval> | null = null;

  constructor(
    parent: HTMLElement,
    private readonly deps: RegionBarDeps,
    private readonly data: RegionBarData = REGION_BAR,
  ) {
    this.openButton = button(data.open.id, data.open.label, () => {
      this.setOpen(!this.open);
    });
    this.openButton.className = 'touch-region-open';
    this.openButton.setAttribute('aria-expanded', 'false');
    this.openButton.hidden = true;
    this.bar = document.createElement('div');
    this.bar.id = 'touch-region-bar';
    this.bar.hidden = true;
    this.statusLine = document.createElement('div');
    this.statusLine.className = 'region-status';
    this.statusLine.setAttribute('aria-live', 'polite');
    const row = document.createElement('div');
    row.className = 'region-buttons';
    this.made = data.buttons.map((spec) => ({
      spec,
      el: button(spec.id, spec.label, () => {
        this.run(spec.run);
      }),
    }));
    row.append(...this.made.map((m) => m.el));
    this.bar.append(this.statusLine, row);
    parent.append(this.openButton, this.bar);
  }

  get isOpen(): boolean {
    return this.open;
  }

  /** Re-reads flight and the mode: the Select button shows or hides (closing the bar). */
  update(): void {
    const st = this.deps.state();
    const shown = regionBarShown(this.data.showWhen, {
      flying: this.deps.flying(),
      enabled: st.length >= 1 && st[0] === 1,
      allows: (a) => this.deps.allows(a),
    });
    this.openButton.hidden = !shown;
    if (!shown && this.open) this.setOpen(false);
  }

  setOpen(open: boolean): void {
    this.open = open;
    this.bar.hidden = !open;
    this.openButton.classList.toggle('pressed', open);
    this.openButton.setAttribute('aria-expanded', String(open));
    // The bar takes the flight speed panel's place (style.css).
    this.bar.ownerDocument.body.classList.toggle('region-bar-open', open);
    this.deps.armed(open);
    if (this.timer !== null) clearInterval(this.timer);
    this.timer = null;
    // The status line follows the tools (a selection's size, a paste's turn, edits sending).
    if (open) {
      this.timer = setInterval(() => {
        this.refresh();
      }, 400);
    }
    this.refresh();
  }

  /** The buttons for the tools' state (select or paste) and the status line. */
  refresh(): void {
    const state = barStateOf(this.deps.state());
    for (const m of this.made) m.el.hidden = !m.spec.in.includes(state);
    this.bar.dataset.state = state;
    this.statusLine.textContent = touchStatus(this.deps.status()) || 'Aim, then set the corners';
  }

  private run(run: string): void {
    if (run.startsWith('tool:')) this.deps.tool(run.slice(5));
    else this.deps.action(run);
    this.refresh();
  }
}

/** A bar button: a tap runs it and goes no further (not to the look layer under it, BUGS #174). */
function button(id: string, label: string, onTap: () => void): HTMLButtonElement {
  const b = document.createElement('button');
  b.type = 'button';
  b.id = id;
  b.textContent = label;
  b.dataset.sound = 'none';
  b.addEventListener('pointerdown', (e) => {
    e.preventDefault();
    e.stopPropagation();
    onTap();
  });
  return b;
}
