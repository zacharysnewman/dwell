// Blueprints in the shell (WORK_QUEUE item 56, docs/BLUEPRINTS.md): a library structure turned into
// the cells a blueprint record carries, and the compact panel shown while aiming into one ("log
// 12/40": each item's have / need, and the cells left), drawn from the client's `blueprintPanel`.
import type { ValidStructure } from '../local/structures';

/** A blueprint's most cells along a side, and palette names (dwell_protocol::blueprints). */
export const BLUEPRINT_MAX_SIDE = 32;
export const BLUEPRINT_MAX_PALETTE = 64;

/** A structure's cells for `Client.placeBlueprintCells`: its state names one a line and each cell's
 * palette index + 1 (0 kept), in the structure's layer order. Air carries nothing to build, so it is
 * kept. Throws a message when it doesn't fit a blueprint. */
export function blueprintCellsOf(s: ValidStructure): { palette: string; cells: Uint8Array } {
  if (s.size.some((n) => n > BLUEPRINT_MAX_SIDE)) {
    throw new Error(`A blueprint is at most ${String(BLUEPRINT_MAX_SIDE)} cells along a side.`);
  }
  const names: string[] = [];
  const index = new Map<string, number>();
  for (const [c, state] of s.palette) {
    if (state === null || state.startsWith('dwell:air')) continue;
    let k = names.indexOf(state);
    if (k < 0) {
      if (names.length === BLUEPRINT_MAX_PALETTE) {
        throw new Error(
          `A blueprint holds at most ${String(BLUEPRINT_MAX_PALETTE)} kinds of block.`,
        );
      }
      names.push(state);
      k = names.length - 1;
    }
    index.set(c, k + 1);
  }
  const cells = new Uint8Array(s.cells.length);
  for (let i = 0; i < s.cells.length; i++) cells[i] = index.get(s.cells[i] ?? '') ?? 0;
  return { palette: names.join('\n'), cells };
}

/** One panel row: an item still needed. */
export interface BlueprintRow {
  item: string;
  /** What the player has ("∞": unlimited blocks). */
  have: string;
  need: number;
}

export interface BlueprintPanelView {
  name: string;
  left: number;
  rows: BlueprintRow[];
  /** Rows past the panel's most. */
  more: number;
}

/** The client's `blueprintPanel` text (name, cells left, "item have need" lines, "+n"); null for "". */
export function parseBlueprintPanel(text: string): BlueprintPanelView | null {
  if (text === '') return null;
  const [name = '', left = '0', ...lines] = text.split('\n');
  const view: BlueprintPanelView = { name, left: Number(left), rows: [], more: 0 };
  for (const l of lines) {
    if (l.startsWith('+')) {
      view.more = Number(l.slice(1));
      continue;
    }
    const [item = '', have = '0', need = '0'] = l.split(' ');
    view.rows.push({ item, have, need: Number(need) });
  }
  return view;
}

/** A name without its namespace: "dwell:bp_hut" → "bp_hut". */
const bare = (n: string): string => n.slice(n.indexOf(':') + 1);

/** The panel's lines: the blueprint's name and cells left, then "log 12/40" per item. */
export function blueprintPanelLines(v: BlueprintPanelView): string[] {
  const out = [`${bare(v.name)} · ${String(v.left)} left`];
  for (const r of v.rows) out.push(`${bare(r.item)} ${r.have}/${String(r.need)}`);
  if (v.more > 0) out.push(`+${String(v.more)} more`);
  return out;
}

/** Whether a row is short: the player has fewer than it needs (drawn red, as its ghosts are). */
export function rowShort(r: BlueprintRow): boolean {
  return r.have !== '∞' && Number(r.have) < r.need;
}

/** The HUD's blueprint panel: shown while the view aims into a blueprint. */
export class BlueprintPanel {
  readonly element = document.createElement('div');
  private last = '';

  constructor() {
    this.element.id = 'blueprint-panel';
    this.element.className = 'blueprint-panel';
    this.element.hidden = true;
  }

  /** Each frame: the client's panel text ("" with none aimed). */
  update(text: string): void {
    if (text === this.last) return;
    this.last = text;
    const v = parseBlueprintPanel(text);
    this.element.hidden = v === null;
    this.element.replaceChildren();
    if (!v) return;
    const [head, ...rest] = blueprintPanelLines(v);
    const h = document.createElement('div');
    h.className = 'blueprint-panel-head';
    h.textContent = head ?? '';
    this.element.append(h);
    rest.forEach((line, i) => {
      const d = document.createElement('div');
      d.className = 'blueprint-panel-row';
      const row = v.rows[i];
      if (row && rowShort(row)) d.classList.add('short');
      d.textContent = line;
      this.element.append(d);
    });
  }
}
