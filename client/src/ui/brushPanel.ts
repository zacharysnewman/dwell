// The brush tool's HUD panel (docs/BRUSHES.md): the preset, brush, performer, size, the block it
// puts down and the one it replaces, the last message, and the keys. Shown above the hotbar while
// the tool is on (`Client.hud().brush`, null while off).

/** `Client.hud().brush`. */
export interface BrushPanelState {
  preset: string;
  brush: string;
  performer: string;
  size: number;
  /** The hotbar's block. */
  material: string;
  /** What a replace (or a target mask) replaces; null when the performer doesn't use it. */
  replace: string | null;
  note: string;
}

/** The panel's rows: label and value, in order. */
export function brushRows(s: BrushPanelState): [string, string][] {
  const rows: [string, string][] = [
    ['Brush', s.brush],
    ['Performer', s.performer],
    ['Size', String(s.size)],
    ['Block', s.material.replace(/_/g, ' ')],
  ];
  if (s.replace !== null) rows.push(['Replace', s.replace.replace(/_/g, ' ')]);
  return rows;
}

/** The key hints under the rows (the controls table's Brushes group has them all). */
export const BRUSH_HINT =
  'Click paint · Right click in front · Middle/⌥ click pick · Q E preset · Z shape · P performer · [ ] size · \\ preview · V off';

export class BrushPanel {
  private readonly root: HTMLDivElement;
  private readonly title: HTMLDivElement;
  private readonly rows: HTMLDListElement;
  private readonly note: HTMLDivElement;
  private last = '';

  constructor(parent: HTMLElement) {
    this.root = document.createElement('div');
    this.root.id = 'brush-panel';
    this.root.hidden = true;
    this.title = document.createElement('div');
    this.title.className = 'brush-title';
    this.rows = document.createElement('dl');
    this.rows.className = 'brush-rows';
    this.note = document.createElement('div');
    this.note.className = 'brush-note';
    const hint = document.createElement('div');
    hint.className = 'brush-hint';
    hint.textContent = BRUSH_HINT;
    this.root.append(this.title, this.rows, this.note, hint);
    parent.append(this.root);
  }

  set(state: BrushPanelState | null): void {
    const key = JSON.stringify(state);
    if (key === this.last) return;
    this.last = key;
    this.root.hidden = state === null;
    if (state === null) return;
    this.title.textContent = state.preset;
    this.rows.replaceChildren(
      ...brushRows(state).flatMap(([k, v]) => {
        const dt = document.createElement('dt');
        dt.textContent = k;
        const dd = document.createElement('dd');
        dd.textContent = v;
        return [dt, dd];
      }),
    );
    this.note.textContent = state.note;
    this.note.hidden = state.note === '';
  }
}
