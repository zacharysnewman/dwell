// The big map's marker picker (docs/MAP_MARKERS.md): a small panel over the map for placing a
// marker (a click on the map) or editing one (a click on it): the icon set and the palette as
// buttons (each icon its vector path, never a glyph), a label of at most `limits.labelChars`,
// Save, Delete (editing) and Cancel. `PickerDraft` is its state and rules, kept apart for tests.

import { MARKER_ICONS, MARKER_LIMITS, MARKER_PALETTE } from './markers.gen';
import { type MarkerLook, clampLabel, iconOf, iconSvg, paletteRgb } from './markers';

/**
 * What the picker holds: an icon, a colour and a label. Picking an icon takes its default colour
 * until a colour is picked by hand; the label is cut to the limit as typed.
 */
export class PickerDraft {
  icon: string;
  rgb: string;
  label: string;
  private colourPicked: boolean;

  constructor(from?: MarkerLook) {
    const first = MARKER_ICONS[0];
    this.icon = from?.icon ?? first?.id ?? 'question';
    this.rgb = from?.rgb ?? paletteRgb(iconOf(this.icon).colour);
    this.label = from?.label ?? '';
    // An edited marker keeps its colour when its icon changes.
    this.colourPicked = from !== undefined;
  }

  pickIcon(id: string): void {
    this.icon = iconOf(id).id;
    if (!this.colourPicked) this.rgb = paletteRgb(iconOf(id).colour);
  }

  pickColour(rgb: string): void {
    this.rgb = rgb;
    this.colourPicked = true;
  }

  setLabel(text: string): void {
    this.label = clampLabel(text);
  }

  /** The look to save (an empty label: the icon's name). */
  look(): MarkerLook {
    return { icon: this.icon, rgb: this.rgb, label: this.label || iconOf(this.icon).name };
  }
}

/** The picker's answer: saved (with the look), deleted, or cancelled. */
export type PickerResult =
  { kind: 'save'; look: MarkerLook } | { kind: 'delete' } | { kind: 'cancel' };

/** The picker panel, in the big map's body. */
export class MarkerPicker {
  readonly root: HTMLDivElement;
  private draft = new PickerDraft();
  private done: ((r: PickerResult) => void) | null = null;
  private readonly iconButtons = new Map<string, HTMLButtonElement>();
  private readonly colourButtons = new Map<string, HTMLButtonElement>();
  private readonly input: HTMLInputElement;
  private readonly title: HTMLHeadingElement;
  private readonly del: HTMLButtonElement;

  constructor(private readonly doc: Document) {
    this.root = doc.createElement('div');
    this.root.className = 'marker-picker';
    this.root.hidden = true;
    this.root.setAttribute('role', 'dialog');
    this.root.setAttribute('aria-label', 'Map marker');
    this.title = doc.createElement('h3');
    const icons = doc.createElement('div');
    icons.className = 'marker-icons';
    for (const ic of MARKER_ICONS) {
      const b = this.button(`marker-icon-${ic.id}`, ic.name);
      b.dataset.icon = ic.id;
      b.addEventListener('click', () => {
        this.draft.pickIcon(ic.id);
        this.show();
      });
      icons.append(b);
      this.iconButtons.set(ic.id, b);
    }
    const colours = doc.createElement('div');
    colours.className = 'marker-colours';
    for (const c of MARKER_PALETTE) {
      const b = this.button(`marker-colour-${c.name}`, c.name);
      b.style.setProperty('--swatch', c.rgb);
      b.addEventListener('click', () => {
        this.draft.pickColour(c.rgb);
        this.show();
      });
      colours.append(b);
      this.colourButtons.set(c.rgb, b);
    }
    this.input = doc.createElement('input');
    this.input.type = 'text';
    this.input.className = 'marker-label';
    this.input.maxLength = MARKER_LIMITS.labelChars;
    this.input.placeholder = 'Label';
    this.input.setAttribute('aria-label', 'Label');
    this.input.addEventListener('input', () => {
      this.draft.setLabel(this.input.value);
    });
    this.input.addEventListener('keydown', (e) => {
      e.stopPropagation();
      if (e.key === 'Enter') this.finish({ kind: 'save', look: this.draft.look() });
      if (e.key === 'Escape') this.finish({ kind: 'cancel' });
    });
    const row = doc.createElement('div');
    row.className = 'marker-actions';
    const save = this.button('marker-save', 'Save', 'Save');
    this.del = this.button('marker-delete', 'Delete', 'Delete');
    const cancel = this.button('marker-cancel', 'Cancel', 'Cancel');
    save.addEventListener('click', () => {
      this.finish({ kind: 'save', look: this.draft.look() });
    });
    this.del.addEventListener('click', () => {
      this.finish({ kind: 'delete' });
    });
    cancel.addEventListener('click', () => {
      this.finish({ kind: 'cancel' });
    });
    row.append(save, this.del, cancel);
    this.root.append(this.title, icons, colours, this.input, row);
    // The map under it doesn't take its pointers.
    this.root.addEventListener('pointerdown', (e) => {
      e.stopPropagation();
    });
  }

  private button(cls: string, label: string, text = ''): HTMLButtonElement {
    const b = this.doc.createElement('button');
    b.type = 'button';
    b.className = `map-button ${cls}`;
    b.title = label;
    b.setAttribute('aria-label', label);
    b.textContent = text;
    return b;
  }

  /** Called when the draft's look changes (the map redraws the pending pin). */
  onChange: (() => void) | null = null;

  /** The draft's look now (the pending pin's icon and colour). */
  get look(): MarkerLook {
    return this.draft.look();
  }

  get open(): boolean {
    return !this.root.hidden;
  }

  /** Opens to place (no `from`) or edit a marker; `done` hears the answer once. */
  openFor(from: MarkerLook | undefined, done: (r: PickerResult) => void): void {
    if (this.done) this.finish({ kind: 'cancel' });
    this.draft = new PickerDraft(from);
    this.done = done;
    this.title.textContent = from ? 'Edit marker' : 'New marker';
    this.del.hidden = !from;
    this.input.value = this.draft.label;
    this.root.hidden = false;
    this.show();
    this.input.focus({ preventScroll: true });
  }

  close(): void {
    if (this.open) this.finish({ kind: 'cancel' });
  }

  private finish(r: PickerResult): void {
    this.root.hidden = true;
    const done = this.done;
    this.done = null;
    done?.(r);
  }

  /** The buttons as the draft is: each icon in the draft's colour, the picked ones marked. */
  private show(): void {
    this.onChange?.();
    for (const [id, b] of this.iconButtons) {
      b.replaceChildren(iconSvg(this.doc, id, this.draft.rgb, 22));
      b.classList.toggle('on', id === this.draft.icon);
      b.setAttribute('aria-pressed', String(id === this.draft.icon));
    }
    for (const [rgb, b] of this.colourButtons) {
      b.classList.toggle('on', rgb === this.draft.rgb);
      b.setAttribute('aria-pressed', String(rgb === this.draft.rgb));
    }
  }
}
