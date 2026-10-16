// The Structures page of the game menu (docs/STRUCTURES.md §4): save what was built as a
// structure, the library of saved structures (local/structureStore.ts) with load, export, rename
// and delete, and import. Loading puts a structure on the region tools' clipboard in paste mode
// (§6.5): the ghost box follows the crosshair and a right click pastes it as region edits.
import { StructureStore, type StoredStructure } from '../local/structureStore';
import {
  boxOf,
  bottomCentre,
  KEPT_ON_SAVE,
  MAX_EDGE,
  parseStructure,
  scanBuilt,
  sizeLabel,
  structureFile,
  structureFromVoxels,
  structureName,
  type ReadBox,
  type StructureJson,
  type ValidStructure,
  type Vec3,
} from '../local/structures';

/** What the page needs of the game. */
export interface StructuresDeps {
  store: StructureStore;
  /** The region tools' selection (B, N): its lowest corner and size; null with none. */
  selection(): { min: Vec3; size: Vec3 } | null;
  read: ReadBox;
  /** The player's feet; null while not playing. */
  feet(): Vec3 | null;
  /** The aimed-at block; null with none. */
  target(): Vec3 | null;
  /** Puts the structure on the clipboard in paste mode: an error message, or '' when done. */
  load(structure: ValidStructure): string;
  /** Closes the menu (to aim the paste). */
  close(): void;
}

/** How far around the player "everything built" looks: x and z either way, y down and up. */
export const SCAN_AROUND: { side: number; below: number; above: number } = {
  side: 48,
  below: 8,
  above: 48,
};

export type SaveSource = 'selection' | 'built' | 'corners';

/** "12 3 -4" or "12, 3, -4" → a cell; null if it isn't three whole numbers. */
export function parseCell(text: string): Vec3 | null {
  const parts = text.trim().split(/[\s,]+/);
  if (parts.length !== 3) return null;
  const v = parts.map(Number);
  return v.every((n) => Number.isInteger(n)) ? (v as Vec3) : null;
}

/**
 * The box a save takes: the selection, everything built around the feet (not air or the
 * sandbox floor), or two typed corners. An error message instead when there is none or it is
 * over the size cap.
 */
export function saveBox(
  source: SaveSource,
  deps: Pick<StructuresDeps, 'selection' | 'read' | 'feet'>,
  corners: [string, string],
): { min: Vec3; size: Vec3 } | { error: string } {
  let box: { min: Vec3; size: Vec3 } | null;
  if (source === 'selection') {
    box = deps.selection();
    if (!box) return { error: 'Select a box first: aim and press B, then N.' };
  } else if (source === 'corners') {
    const a = parseCell(corners[0]);
    const b = parseCell(corners[1]);
    if (!a || !b) return { error: 'Give both corners as three whole numbers (x y z).' };
    box = boxOf(a, b);
  } else {
    const feet = deps.feet();
    if (!feet) return { error: 'Not playing yet.' };
    const f = feet.map(Math.floor) as Vec3;
    const { side, below, above } = SCAN_AROUND;
    box = scanBuilt(
      deps.read,
      [f[0] - side, f[1] - below, f[2] - side],
      [2 * side + 1, below + above + 1, 2 * side + 1],
    );
    if (!box) return { error: 'Nothing built near you yet.' };
  }
  if (box.size.some((n) => n > MAX_EDGE)) {
    return {
      error: `That is ${sizeLabel(box.size)}: a structure is at most ${String(MAX_EDGE)} a side.`,
    };
  }
  return box;
}

/** A structure from the world's box; an error message if it isn't loaded or isn't valid. */
export function saveFromWorld(
  read: ReadBox,
  box: { min: Vec3; size: Vec3 },
  nameText: string,
  anchorText: string,
): { json: StructureJson } | { error: string } {
  const name = structureName(nameText);
  if ('error' in name) return name;
  let anchor = bottomCentre(box.size);
  if (anchorText.trim() !== '') {
    const a = parseCell(anchorText);
    if (!a || a.some((n, i) => n < 0 || n >= (box.size[i] ?? 0))) {
      return {
        error: `The anchor is a cell inside the box, 0 0 0 to ${box.size.map((n) => n - 1).join(' ')}.`,
      };
    }
    anchor = a;
  }
  const cells = read(box.min, box.size);
  if (cells.length === 0) return { error: 'Part of that box is not loaded: go closer.' };
  try {
    return {
      json: structureFromVoxels({
        name: name.name,
        size: box.size,
        cells,
        anchor,
        kept: KEPT_ON_SAVE,
      }),
    };
  } catch (e) {
    return { error: e instanceof Error ? e.message : String(e) };
  }
}

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className?: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (className) e.className = className;
  if (text !== undefined) e.textContent = text;
  return e;
}

function button(text: string, onClick: () => void, className = 'st-button'): HTMLButtonElement {
  const b = el('button', className, text);
  b.type = 'button';
  b.addEventListener('click', onClick);
  return b;
}

/** Offers a file to save (the browser's download). */
function download(file: string, text: string): void {
  const url = URL.createObjectURL(new Blob([text], { type: 'application/json' }));
  const a = el('a');
  a.href = url;
  a.download = file;
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => {
    URL.revokeObjectURL(url);
  }, 1000);
}

export class StructuresPanel {
  readonly element = el('section', 'structures-panel');
  private readonly status = el('p', 'st-status');
  private readonly list = el('div', 'st-list');
  private readonly name = el('input');
  private readonly anchor = el('input');
  private readonly source = el('select');
  private readonly cornerA = el('input');
  private readonly cornerB = el('input');
  private readonly cornersRow = el('div', 'st-corners');
  private readonly paste = el('textarea', 'st-paste');

  constructor(private readonly deps: StructuresDeps) {
    this.element.id = 'structures-panel';
    const lead = el(
      'p',
      'gm-lead',
      'Save what you built as a structure, kept in this browser. Export it to commit it to shared/structures/.',
    );

    // Save.
    const save = el('div', 'gm-group');
    save.append(el('h2', undefined, 'Save'));
    this.name.id = 'structure-name';
    this.name.placeholder = 'Name, e.g. oak_tree';
    this.name.maxLength = 64;
    for (const [value, label] of [
      ['selection', 'The selected box (B, N)'],
      ['built', 'Everything built near you'],
      ['corners', 'Between two corners'],
    ] as const) {
      const o = el('option', undefined, label);
      o.value = value;
      this.source.append(o);
    }
    this.source.id = 'structure-source';
    this.source.addEventListener('change', () => {
      this.cornersRow.hidden = this.source.value !== 'corners';
    });
    this.cornerA.placeholder = 'Corner x y z';
    this.cornerB.placeholder = 'Corner x y z';
    this.cornerA.id = 'structure-corner-a';
    this.cornerB.id = 'structure-corner-b';
    const aim = (input: HTMLInputElement) =>
      button('Aimed block', () => {
        const t = this.deps.target();
        if (t) input.value = t.join(' ');
        else this.say('Aim at a block first.');
      });
    this.cornersRow.append(this.cornerA, aim(this.cornerA), this.cornerB, aim(this.cornerB));
    this.anchor.id = 'structure-anchor';
    this.anchor.placeholder = 'Anchor x y z in the box (blank: bottom centre)';
    const saveButton = button('Save structure', () => void this.save(), 'st-button st-primary');
    saveButton.id = 'structure-save';
    save.append(this.name, this.source, this.cornersRow, this.anchor, saveButton);

    // Library.
    const library = el('div', 'gm-group');
    library.append(el('h2', undefined, 'Saved structures'), this.list);

    // Import.
    const importing = el('div', 'gm-group');
    importing.append(el('h2', undefined, 'Import'));
    const file = el('input');
    file.type = 'file';
    file.accept = '.json,application/json';
    file.hidden = true;
    file.id = 'structure-file';
    file.addEventListener('change', () => {
      const f = file.files?.[0];
      file.value = '';
      if (f) void f.text().then((text) => this.importText(text, f.name));
    });
    this.paste.placeholder = 'Paste a structure’s JSON here';
    this.paste.rows = 4;
    this.paste.id = 'structure-paste';
    const row = el('div', 'st-row');
    row.append(
      button('Import file…', () => {
        file.click();
      }),
      button('Import pasted JSON', () => void this.importText(this.paste.value, 'pasted JSON')),
    );
    importing.append(this.paste, row, file);

    this.status.setAttribute('role', 'status');
    this.status.id = 'structure-status';
    this.element.append(lead, this.status, save, library, importing);
  }

  /** Called as the page opens: the source the save defaults to, and the list. */
  async refresh(): Promise<void> {
    this.source.value = this.deps.selection() ? 'selection' : 'built';
    this.cornersRow.hidden = true;
    await this.renderList();
  }

  private say(text: string): void {
    this.status.textContent = text;
  }

  private async save(): Promise<void> {
    const box = saveBox(this.source.value as SaveSource, this.deps, [
      this.cornerA.value,
      this.cornerB.value,
    ]);
    if ('error' in box) {
      this.say(box.error);
      return;
    }
    const made = saveFromWorld(this.deps.read, box, this.name.value, this.anchor.value);
    if ('error' in made) {
      this.say(made.error);
      return;
    }
    const file = structureFile(String(made.json.name));
    try {
      const exists = (await this.deps.store.get(file)) !== null;
      if (exists && !confirm(`Replace the saved ${file}?`)) return;
      await this.deps.store.save(made.json, { overwrite: exists });
      this.say(`Saved ${file} (${sizeLabel(box.size)}).`);
    } catch (e) {
      this.say(e instanceof Error ? e.message : String(e));
    }
    await this.renderList();
  }

  private async importText(text: string, from: string): Promise<void> {
    let json: StructureJson;
    try {
      ({ json } = parseStructure(text, from));
    } catch (e) {
      this.say(e instanceof Error ? e.message : String(e));
      return;
    }
    const file = structureFile(String(json.name));
    try {
      const exists = (await this.deps.store.get(file)) !== null;
      if (exists && !confirm(`Replace the saved ${file}?`)) return;
      await this.deps.store.save(json, { overwrite: exists });
      this.paste.value = '';
      this.say(`Imported ${file}.`);
    } catch (e) {
      this.say(e instanceof Error ? e.message : String(e));
    }
    await this.renderList();
  }

  private async renderList(): Promise<void> {
    let entries: StoredStructure[];
    try {
      entries = await this.deps.store.list();
    } catch {
      this.list.replaceChildren(el('p', 'st-empty', 'Saving is not available in this browser.'));
      return;
    }
    if (entries.length === 0) {
      this.list.replaceChildren(el('p', 'st-empty', 'None yet.'));
      return;
    }
    this.list.replaceChildren(...entries.map((e) => this.row(e)));
  }

  private row(entry: StoredStructure): HTMLElement {
    const row = el('div', 'st-entry');
    row.dataset.file = entry.file;
    const info = el('div', 'st-info');
    const actions = el('div', 'st-actions');
    const exportText = async () => (await this.deps.store.exportText(entry.file)) ?? '';
    const download_ = button(
      'Download',
      () =>
        void exportText().then((t) => {
          download(entry.file, t);
        }),
    );
    const copy = button('Copy', () => {
      void exportText().then(async (t) => {
        try {
          await navigator.clipboard.writeText(t);
          this.say(`Copied ${entry.file}.`);
        } catch {
          this.paste.value = t;
          this.paste.select();
          this.say('No clipboard here: the JSON is in the import box to copy by hand.');
        }
      });
    });
    if (entry.status === 'unreadable') {
      info.append(
        el('span', 'st-name', entry.file),
        el('span', 'st-size', 'Kept as is: this version can’t read it'),
      );
      info.title = entry.reason;
      actions.append(download_, copy);
      row.append(info, actions);
      return row;
    }
    const s = entry.structure;
    info.append(el('span', 'st-name', s.name), el('span', 'st-size', sizeLabel(s.size)));
    const load = button(
      'Load',
      () => {
        const err = this.deps.load(s);
        if (err) {
          this.say(err);
          return;
        }
        this.deps.close();
      },
      'st-button st-primary',
    );
    const rename = button('Rename', () => {
      const to = prompt('New name', s.name);
      if (to === null) return;
      const n = structureName(to);
      if ('error' in n) {
        this.say(n.error);
        return;
      }
      void this.deps.store
        .rename(entry.file, n.name)
        .then((f) => {
          this.say(`Renamed to ${f}.`);
        })
        .catch((e: unknown) => {
          this.say(e instanceof Error ? e.message : String(e));
        })
        .then(() => this.renderList());
    });
    const remove = button('Delete', () => {
      if (!confirm(`Delete ${entry.file}?`)) return;
      void this.deps.store
        .remove(entry.file)
        .then(() => {
          this.say(`Deleted ${entry.file}.`);
        })
        .then(() => this.renderList());
    });
    actions.append(load, download_, copy, rename, remove);
    row.append(info, actions);
    return row;
  }
}
