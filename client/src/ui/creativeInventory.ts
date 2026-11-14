// The creative inventory view (docs/ITEMS.md §6a; owner, 2026-11-10): every creative item
// (shared/items/items.json) in a catalogue grouped by its `category`, the hotbar's slots under it.
// Click or tap an item to put it in the selected hotbar slot, or drag it onto a slot; the picks are
// the client's (`Client.setCreativeSlot`) and saved per world in this browser (`dwell.creativeHotbar`).

import { CREATIVE } from './buildModes.gen';
import { HOTBAR_SLOTS } from './inventory.gen';
import type { HotbarSlot } from './hotbar';

export interface CatalogueItem {
  name: string;
  label: string;
  /** Its palette index (`Client.paletteItems()`). */
  index: number;
}

export interface CatalogueGroup {
  name: string;
  label: string;
  items: CatalogueItem[];
}

type Creative = {
  categories: readonly { name: string; label: string }[];
  items: readonly { name: string; label: string; category: string }[];
};

/**
 * The catalogue: the data's groups in order, each with its items in the data's order, matched to
 * the palette by name (an item this build's palette left out is not shown); empty groups dropped.
 * `query` keeps the items whose label holds it (any case).
 */
export function catalogue(
  palette: readonly string[],
  query = '',
  data: Creative = CREATIVE,
): CatalogueGroup[] {
  const q = query.trim().toLowerCase();
  return data.categories
    .map((c) => ({
      name: c.name,
      label: c.label,
      items: data.items
        .filter((it) => it.category === c.name)
        .map((it) => ({ name: it.name, label: it.label, index: palette.indexOf(it.name) }))
        .filter((it) => it.index >= 0 && (q === '' || it.label.toLowerCase().includes(q))),
    }))
    .filter((g) => g.items.length > 0);
}

/** A saved structure as the Structures tab lists it (the player's library, STRUCTURES.md §9). */
export interface StructureEntry {
  file: string;
  label: string;
  /** "5 × 4 × 5". */
  size: string;
}

/**
 * The Structures tab's entries: the store's readable structures (files a newer build wrote are
 * left out: they are kept, not placed), in the store's order, whose name holds `query` (any case).
 */
export function structureEntries(
  stored: readonly (
    | { file: string; status: 'ok'; structure: { name: string; size: readonly number[] } }
    | { file: string; status: 'unreadable' }
  )[],
  query = '',
): StructureEntry[] {
  const q = query.trim().toLowerCase();
  return stored
    .flatMap((e) =>
      e.status === 'ok'
        ? [{ file: e.file, label: e.structure.name, size: e.structure.size.join(' × ') }]
        : [],
    )
    .filter((e) => q === '' || e.label.toLowerCase().includes(q));
}

/** Where the Structures tab gets the library and places from it. */
export interface StructureSource {
  /** The world's mode places structures (creative only for now). */
  allowed(): boolean;
  list(): Promise<StructureEntry[]>;
  /** Loads it into paste mode (ghost, R rotate, right click places): an error, or '' when done. */
  place(file: string): Promise<string>;
}

/** The browser's store of creative picks: world key → item names, one per hotbar slot. */
export const PICKS_KEY = 'dwell.creativeHotbar';

/** A world's saved picks as palette indices, or null (none saved, or unreadable). */
export function loadPicks(
  storage: Pick<Storage, 'getItem'>,
  world: string,
  palette: readonly string[],
): number[] | null {
  try {
    const all = JSON.parse(storage.getItem(PICKS_KEY) ?? '{}') as Record<string, unknown>;
    const names = all[world];
    if (!Array.isArray(names)) return null;
    const picks = names.slice(0, HOTBAR_SLOTS).map((n) => palette.indexOf(String(n)));
    return picks.length === HOTBAR_SLOTS && picks.every((i) => i >= 0) ? picks : null;
  } catch {
    return null;
  }
}

/**
 * Saves a world's picks (palette indices) by item name. Append-only (CLAUDE.md): other worlds'
 * records and unknown fields are kept; a store that can't be read is left alone.
 */
export function savePicks(
  storage: Pick<Storage, 'getItem' | 'setItem'>,
  world: string,
  picks: readonly number[],
  palette: readonly string[],
): void {
  try {
    const raw = storage.getItem(PICKS_KEY);
    const all = (raw === null ? {} : JSON.parse(raw)) as Record<string, unknown>;
    if (typeof all !== 'object' || Array.isArray(all)) return;
    all[world] = picks.map((i) => palette[i] ?? '');
    storage.setItem(PICKS_KEY, JSON.stringify(all));
  } catch {
    // An unreadable store or no storage: the picks last this session only.
  }
}

/** A swatch canvas from a slot's picture. */
function picture(slot: HotbarSlot): HTMLCanvasElement {
  const canvas = document.createElement('canvas');
  canvas.width = slot.size;
  canvas.height = slot.size;
  const ctx = canvas.getContext('2d');
  if (ctx && slot.swatch.length === slot.size * slot.size * 4) {
    ctx.putImageData(new ImageData(new Uint8ClampedArray(slot.swatch), slot.size, slot.size), 0, 0);
  }
  return canvas;
}

/** What a drag carries: a catalogue item (palette index) or a hotbar slot's pick. */
export type DragSource = { item: number } | { slot: number };

/**
 * The slot assignments a drop makes ([slot, palette index] pairs, in order): an item dropped on a slot
 * fills it; a slot dropped on another swaps their picks (owner, 2026-11-10: dragging between the
 * creative slots "is really important, since that limits what you can place"). Nothing for a slot
 * dropped on itself or an empty source.
 */
export function dropAssignments(
  source: DragSource,
  target: number,
  picks: readonly number[],
): [number, number][] {
  if ('item' in source) return [[target, source.item]];
  if (source.slot === target) return [];
  const from = picks[source.slot];
  const to = picks[target];
  if (from === undefined) return [];
  return to === undefined
    ? [[target, from]]
    : [
        [target, from],
        [source.slot, to],
      ];
}

/** How far (px) a press travels before it is a drag, not a click. */
const DRAG_PX = 6;

export class CreativeInventory {
  private readonly root: HTMLDivElement;
  private readonly groups: HTMLDivElement;
  private readonly search: HTMLInputElement;
  private readonly slots: HTMLButtonElement[] = [];
  private picks: number[] = [];
  private selected = 0;
  /** True from a drag's start until the click that ends it (so a drag is not also a click). */
  private dragged = false;

  constructor(
    parent: HTMLElement,
    private readonly palette: readonly string[],
    private readonly swatchOf: (index: number) => HotbarSlot,
    /** Puts palette item `index` in hotbar slot `slot`; the picks after. */
    private readonly assign: (slot: number, index: number) => readonly number[],
    /** Selects hotbar slot `slot` (a click on a slot of the view). */
    private readonly select: (slot: number) => void,
    /** The player's saved structures (the Structures tab); none without one. */
    private readonly structures?: StructureSource,
  ) {
    this.root = document.createElement('div');
    this.root.id = 'creative-inventory';
    this.root.className = 'inventory-view';
    this.root.hidden = true;
    this.root.setAttribute('role', 'dialog');
    this.root.setAttribute('aria-modal', 'true');
    this.root.setAttribute('aria-label', 'Inventory');
    const head = document.createElement('div');
    head.className = 'inventory-head';
    const title = document.createElement('div');
    title.className = 'backpack-title';
    title.textContent = 'Inventory';
    this.search = document.createElement('input');
    this.search.type = 'search';
    this.search.className = 'inventory-search';
    this.search.placeholder = 'Search';
    this.search.setAttribute('aria-label', 'Search items');
    this.search.addEventListener('input', () => {
      this.drawCatalogue();
    });
    head.append(title, this.search);
    this.groups = document.createElement('div');
    this.groups.className = 'inventory-groups';
    const bar = document.createElement('div');
    bar.className = 'backpack-grid inventory-hotbar';
    bar.style.gridTemplateColumns = `repeat(${String(HOTBAR_SLOTS)}, 44px)`;
    for (let i = 0; i < HOTBAR_SLOTS; i++) {
      const b = document.createElement('button');
      b.type = 'button';
      b.className = 'hotbar-slot backpack-slot';
      b.dataset.slot = String(i);
      b.addEventListener('click', () => {
        if (this.dragged) return;
        this.select(i);
      });
      this.draggable(b, () => ({ slot: i }));
      this.slots.push(b);
      bar.append(b);
    }
    this.root.append(head, this.groups, bar);
    parent.append(this.root);
    this.drawCatalogue();
  }

  get open(): boolean {
    return !this.root.hidden;
  }

  set open(v: boolean) {
    const was = !this.root.hidden;
    this.root.hidden = !v;
    if (!v) {
      this.search.blur();
    } else if (!was) {
      void this.loadStructures();
    }
  }

  /** The library's entries, read each time the view opens (a save in the menu shows at once). */
  private entries: StructureEntry[] = [];
  private note = '';

  private async loadStructures(): Promise<void> {
    if (!this.structures?.allowed()) {
      this.entries = [];
    } else {
      try {
        this.entries = await this.structures.list();
      } catch {
        // No OPFS (a private window): no tab.
        this.entries = [];
      }
    }
    this.drawCatalogue();
  }

  /** The hotbar's picks (`Client.creativeHotbar()`) and selected slot. */
  set(picks: readonly number[], selected: number): void {
    if (this.picks.join() === picks.join() && this.selected === selected) return;
    this.picks = [...picks];
    this.selected = selected;
    this.slots.forEach((b, i) => {
      const index = this.picks[i];
      b.replaceChildren();
      b.classList.toggle('selected', i === selected);
      if (index === undefined) return;
      const pic = this.swatchOf(index);
      b.title = pic.name;
      b.append(picture(pic));
    });
  }

  /** Puts the item named `name` in the selected slot; false if the palette lacks it. */
  pickItem(name: string): boolean {
    const index = this.palette.indexOf(name);
    if (index < 0) return false;
    this.pick(this.selected, index);
    return true;
  }

  /** Puts palette item `index` in slot `slot` and redraws the view's hotbar. */
  pick(slot: number, index: number): void {
    const picks = this.assign(slot, index);
    this.picks = [];
    this.set(picks, this.selected);
  }

  private drawCatalogue(): void {
    this.groups.replaceChildren();
    for (const g of catalogue(this.palette, this.search.value)) {
      const section = document.createElement('section');
      section.className = 'inventory-group';
      section.dataset.category = g.name;
      const h = document.createElement('div');
      h.className = 'inventory-group-label';
      h.textContent = g.label;
      const grid = document.createElement('div');
      grid.className = 'inventory-items';
      for (const it of g.items) {
        const b = document.createElement('button');
        b.type = 'button';
        b.className = 'hotbar-slot backpack-slot';
        b.dataset.item = it.name;
        b.title = it.label;
        b.append(picture(this.swatchOf(it.index)));
        b.addEventListener('click', () => {
          if (this.dragged) return;
          this.pick(this.selected, it.index);
        });
        this.draggable(b, () => ({ item: it.index }));
        grid.append(b);
      }
      section.append(h, grid);
      this.groups.append(section);
    }
    this.drawStructures();
  }

  /** The Structures tab: the player's saved structures; a click places one (a ghost to aim). */
  private drawStructures(): void {
    const list = structureEntries(
      this.entries.map((e) => ({
        file: e.file,
        status: 'ok' as const,
        structure: { name: e.label, size: e.size.split(' × ').map(Number) },
      })),
      this.search.value,
    );
    if (list.length === 0 || !this.structures) return;
    const source = this.structures;
    const section = document.createElement('section');
    section.className = 'inventory-group';
    section.dataset.category = 'structures';
    const h = document.createElement('div');
    h.className = 'inventory-group-label';
    h.textContent = 'Structures';
    const grid = document.createElement('div');
    grid.className = 'inventory-structures';
    for (const e of list) {
      const b = document.createElement('button');
      b.type = 'button';
      b.className = 'st-button inventory-structure';
      b.dataset.structure = e.file;
      b.textContent = `${e.label.slice(e.label.indexOf(':') + 1)} (${e.size})`;
      b.title = `${e.label}: place it (R turns it, right click places)`;
      b.addEventListener('click', () => {
        void source.place(e.file).then((err) => {
          this.note = err;
          if (err) this.drawCatalogue();
        });
      });
      grid.append(b);
    }
    section.append(h, grid);
    if (this.note) {
      const n = document.createElement('div');
      n.className = 'inventory-note';
      n.textContent = this.note;
      section.append(n);
    }
    this.groups.append(section);
  }

  /** Drops `source` on slot `target`: fills it, or swaps two slots' picks. */
  drop(source: DragSource, target: number): void {
    const moves = dropAssignments(source, target, this.picks);
    if (moves.length === 0) return;
    let picks: readonly number[] = this.picks;
    for (const [slot, index] of moves) picks = this.assign(slot, index);
    this.picks = [];
    this.set(picks, this.selected);
  }

  /**
   * Pointer dragging (mouse, touch and pen alike; HTML drag-and-drop does not work on touch): a press
   * that travels DRAG_PX lifts a ghost of the button under the finger, and the release over a hotbar
   * slot of this view drops `source()` there.
   */
  private draggable(b: HTMLButtonElement, source: () => DragSource): void {
    b.style.touchAction = 'none';
    b.addEventListener('pointerdown', (down) => {
      if (down.button !== 0) return;
      const start = { x: down.clientX, y: down.clientY };
      let ghost: HTMLElement | null = null;
      this.dragged = false;
      const move = (e: PointerEvent): void => {
        if (!ghost) {
          if (Math.hypot(e.clientX - start.x, e.clientY - start.y) < DRAG_PX) return;
          this.dragged = true;
          ghost = b.cloneNode(true) as HTMLElement;
          const from = b.querySelector('canvas');
          const to = ghost.querySelector('canvas');
          if (from && to) to.getContext('2d')?.drawImage(from, 0, 0);
          ghost.className = 'hotbar-slot backpack-slot inventory-ghost';
          Object.assign(ghost.style, {
            position: 'fixed',
            pointerEvents: 'none',
            zIndex: '1000',
            opacity: '0.85',
            margin: '0',
          });
          document.body.append(ghost);
        }
        ghost.style.left = `${String(e.clientX - 22)}px`;
        ghost.style.top = `${String(e.clientY - 22)}px`;
        const over = this.slotAt(e.clientX, e.clientY);
        this.slots.forEach((s, i) => s.classList.toggle('drop-target', i === over));
      };
      const end = (e: PointerEvent): void => {
        b.removeEventListener('pointermove', move);
        b.removeEventListener('pointerup', end);
        b.removeEventListener('pointercancel', end);
        this.slots.forEach((s) => {
          s.classList.remove('drop-target');
        });
        if (!ghost) return;
        ghost.remove();
        const target = e.type === 'pointerup' ? this.slotAt(e.clientX, e.clientY) : null;
        if (target !== null) this.drop(source(), target);
        // The click that follows a drag is swallowed; clear the flag after it.
        setTimeout(() => {
          this.dragged = false;
        }, 0);
      };
      try {
        b.setPointerCapture(down.pointerId);
      } catch {
        // A synthetic or already-released pointer: the drag still follows the button's events.
      }
      b.addEventListener('pointermove', move);
      b.addEventListener('pointerup', end);
      b.addEventListener('pointercancel', end);
    });
  }

  /** The view's hotbar slot under a screen point, or null. */
  private slotAt(x: number, y: number): number | null {
    for (let i = 0; i < this.slots.length; i++) {
      const r = this.slots[i]?.getBoundingClientRect();
      if (r && x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) return i;
    }
    return null;
  }
}
