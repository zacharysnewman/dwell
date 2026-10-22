// The create-world screen's generation settings (docs/WORLD_GENERATION.md §9): a preset picker with
// each preset's description, and an Advanced section of sliders grouped by category with their
// readouts, plus Reset. Everything it shows is data (shared/worldgen/settings.json, generated into
// world/worldgenSettings.gen.ts): a new slider, category or preset appears here without code. The
// settings are chosen once, when the world is created; the route carries them encoded (`gen`).
import {
  CATEGORIES,
  PRESETS,
  SLIDERS,
  encodeWorldgen,
  presetOf,
  presetValues,
  readout,
  snap,
  type WorldgenValues,
} from '../world/worldgenSettings';

/** What the picker shows when the sliders match no preset. */
export const CUSTOM_DESCRIPTION = 'Custom settings: your own mix of the sliders below.';

export class WorldgenPanel {
  readonly root = document.createElement('div');
  private values: WorldgenValues;
  private readonly presetButtons = new Map<string, HTMLButtonElement>();
  private readonly description = document.createElement('p');
  private readonly inputs = new Map<string, HTMLInputElement>();
  private readonly outputs = new Map<string, HTMLOutputElement>();
  readonly advanced = document.createElement('details');

  constructor() {
    this.values = presetValues(PRESETS[0]?.id ?? 'default') ?? {};
    this.root.id = 'worldgen';
    this.root.className = 'wg';

    const head = document.createElement('div');
    head.className = 'wg-head';
    const title = document.createElement('span');
    title.className = 'wg-title';
    title.id = 'worldgen-title';
    title.textContent = 'Landscape';
    head.append(title);

    const presets = document.createElement('div');
    presets.className = 'wg-presets';
    presets.setAttribute('role', 'radiogroup');
    presets.setAttribute('aria-labelledby', title.id);
    for (const p of PRESETS) {
      const b = document.createElement('button');
      b.type = 'button';
      b.className = 'wg-preset';
      b.dataset.preset = p.id;
      b.setAttribute('role', 'radio');
      b.textContent = p.label;
      b.addEventListener('click', () => {
        this.choose(p.id);
      });
      this.presetButtons.set(p.id, b);
      presets.append(b);
    }
    this.description.id = 'worldgen-description';
    this.description.className = 'menu-hint wg-description';
    this.description.setAttribute('aria-live', 'polite');

    this.advanced.id = 'worldgen-advanced';
    this.advanced.className = 'wg-advanced';
    const summary = document.createElement('summary');
    summary.textContent = 'Advanced';
    this.advanced.append(summary);
    for (const c of CATEGORIES) {
      const sliders = SLIDERS.filter((s) => s.category === c.id);
      if (!sliders.length) continue;
      const group = document.createElement('fieldset');
      group.className = 'wg-group';
      group.dataset.category = c.id;
      const legend = document.createElement('legend');
      legend.textContent = c.label;
      group.append(legend);
      for (const s of sliders) group.append(this.row(s));
      this.advanced.append(group);
    }
    const actions = document.createElement('div');
    actions.className = 'wg-actions';
    const reset = document.createElement('button');
    reset.type = 'button';
    reset.id = 'worldgen-reset';
    reset.className = 'quiet-button';
    reset.textContent = 'Reset';
    reset.title = 'Every slider back to the default world';
    reset.addEventListener('click', () => {
      this.choose(PRESETS[0]?.id ?? 'default');
    });
    actions.append(reset);
    this.advanced.append(actions);

    this.root.append(head, presets, this.description, this.advanced);
    this.sync();
  }

  /** One slider (a range with its readout, or a switch for a toggle) and its hint. */
  private row(s: (typeof SLIDERS)[number]): HTMLElement {
    const row = document.createElement('label');
    row.className = s.toggle ? 'wg-row wg-toggle' : 'wg-row';
    row.dataset.slider = s.id;
    const name = document.createElement('span');
    name.className = 'wg-label';
    name.textContent = s.label;
    const out = document.createElement('output');
    out.className = 'wg-value';
    const input = document.createElement('input');
    input.id = `worldgen-${s.id}`;
    input.name = s.id;
    if (s.toggle) {
      input.type = 'checkbox';
      input.setAttribute('role', 'switch');
      input.addEventListener('change', () => {
        this.set(s.id, input.checked ? 1 : 0);
      });
    } else {
      input.type = 'range';
      input.min = String(s.min);
      input.max = String(s.max);
      input.step = String(s.step);
      input.addEventListener('input', () => {
        this.set(s.id, Number(input.value));
      });
    }
    out.htmlFor.add(input.id);
    this.inputs.set(s.id, input);
    this.outputs.set(s.id, out);
    row.append(name, out, input);
    if (s.hint) {
      const hint = document.createElement('span');
      hint.className = 'wg-hint';
      hint.textContent = s.hint;
      row.append(hint);
    }
    return row;
  }

  /** Picks a preset: every slider takes its value. */
  choose(id: string): void {
    const v = presetValues(id);
    if (!v) return;
    this.values = v;
    this.sync();
  }

  /** Sets one slider (snapped to its step). */
  set(id: string, value: number): void {
    const s = SLIDERS.find((x) => x.id === id);
    if (!s) return;
    this.values = { ...this.values, [id]: snap(s, value) };
    this.sync();
  }

  /** The values, every slider. */
  get(): WorldgenValues {
    return { ...this.values };
  }

  /** The settings as the route carries them (`gen`; empty: the defaults). */
  encoded(): string {
    return encodeWorldgen(this.values);
  }

  /** The preset the values match, or null (custom). */
  preset(): string | null {
    return presetOf(this.values)?.id ?? null;
  }

  /** The controls, the readouts, the picker and the description from the values. */
  private sync(): void {
    for (const s of SLIDERS) {
      const v = this.values[s.id] ?? s.default;
      const input = this.inputs.get(s.id);
      if (input) {
        if (s.toggle) input.checked = v !== 0;
        else if (Number(input.value) !== v) input.value = String(v);
      }
      const out = this.outputs.get(s.id);
      if (out) out.textContent = readout(s, v);
      input?.closest('.wg-row')?.classList.toggle('wg-changed', v !== s.default);
    }
    const chosen = presetOf(this.values);
    for (const [id, b] of this.presetButtons) {
      const on = chosen?.id === id;
      b.setAttribute('aria-checked', String(on));
      b.classList.toggle('wg-preset-on', on);
    }
    this.root.dataset.preset = chosen?.id ?? 'custom';
    this.description.textContent = chosen?.description ?? CUSTOM_DESCRIPTION;
  }
}
