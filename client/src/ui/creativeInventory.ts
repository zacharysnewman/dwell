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

export class CreativeInventory {
  private readonly root: HTMLDivElement;
  private readonly groups: HTMLDivElement;
  private readonly search: HTMLInputElement;
  private readonly slots: HTMLButtonElement[] = [];
  private picks: number[] = [];
  private selected = 0;

  constructor(
    parent: HTMLElement,
    private readonly palette: readonly string[],
    private readonly swatchOf: (index: number) => HotbarSlot,
    /** Puts palette item `index` in hotbar slot `slot`; the picks after. */
    private readonly assign: (slot: number, index: number) => readonly number[],
    /** Selects hotbar slot `slot` (a click on a slot of the view). */
    private readonly select: (slot: number) => void,
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
        this.select(i);
      });
      b.addEventListener('dragover', (e) => {
        e.preventDefault();
      });
      b.addEventListener('drop', (e) => {
        e.preventDefault();
        const index = Number(e.dataTransfer?.getData('text/plain'));
        if (Number.isInteger(index) && index >= 0) this.pick(i, index);
      });
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
    this.root.hidden = !v;
    if (!v) {
      this.search.blur();
    }
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
        b.draggable = true;
        b.append(picture(this.swatchOf(it.index)));
        b.addEventListener('click', () => {
          this.pick(this.selected, it.index);
        });
        b.addEventListener('dragstart', (e) => {
          e.dataTransfer?.setData('text/plain', String(it.index));
        });
        grid.append(b);
      }
      section.append(h, grid);
      this.groups.append(section);
    }
  }
}
