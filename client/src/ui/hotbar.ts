// The hotbar (ARCHITECTURE.md §6.5): its slots (shared/inventory/inventory.json `hotbarSlots`, 6)
// with the selected one highlighted: in creative the palette items picked in the inventory view
// (docs/ITEMS.md §6a), in a world that uses items the inventory's first slots. Number keys and the
// scroll wheel (desktop) or tapping a slot (touch) change the selection.

import { BrushPanel, type BrushPanelState } from './brushPanel';
import { BUILD_MODES } from './buildModes.gen';
import { keyLabel } from './controls';
import { HOTBAR_SLOTS, ITEMS } from './inventory.gen';
import { DURABILITY } from './tools.gen';

/** The inventory as the HUD has it (`Client.hud().inventory`, ADR 0066): null in creative. */
export interface InventoryHud {
  /** `wear`: the hits a wearing tool has used (absent or 0: unworn). */
  slots: ({ item: string; count: number; wear?: number } | null)[];
  held: number;
  money: number;
  capacity: number;
  /** The server makes recipes (`inventory_flags::COMBINE`): the backpack shows Combine. */
  combines?: boolean;
}

/**
 * A slot's wear bar (owner, 2026-11-11: tools wear): the share of its hits left, 0..1, for an item
 * the data gives a durability (`DURABILITY`); null for one that never wears or is unworn.
 */
export function wearLeft(item: string, wear: number | undefined): number | null {
  const hits = DURABILITY[item];
  if (hits === undefined || !wear) return null;
  return Math.max(0, (hits - wear) / hits);
}

/** The wear bar's element for a slot (null: no bar). */
export function wearBar(item: string, wear: number | undefined): HTMLSpanElement | null {
  const left = wearLeft(item, wear);
  if (left === null) return null;
  const bar = document.createElement('span');
  bar.className = 'wear-bar';
  const fill = document.createElement('span');
  fill.style.width = `${String(Math.round(left * 100))}%`;
  fill.style.background = left > 0.5 ? '#7cd35a' : left > 0.2 ? '#e6c43c' : '#e0533a';
  bar.append(fill);
  return bar;
}

/** An item's label: a non-block item's from the data, a block's from its name. */
export function itemLabel(item: string): string {
  const def = ITEMS.find((i) => i.name === item);
  if (def) return def.label;
  const name = item.replace(/^[a-z0-9_]+:/, '').replace(/_/g, ' ');
  return name.charAt(0).toUpperCase() + name.slice(1);
}

/** A palette slot as the hotbar shows it: its name and picture (`Client.palette()`, `hotbarSwatch`). */
export interface HotbarSlot {
  name: string;
  /** size × size RGBA8. */
  swatch: Uint8Array;
  size: number;
}

/** Key label of a slot: 1–9, then 0 for the tenth; later slots are reached by scrolling. */
export function slotKey(slot: number): string {
  return slot < 9 ? String(slot + 1) : slot === 9 ? '0' : '';
}

/** Slot selected by a key code (Digit1…Digit9, Digit0), or null. */
export function slotForKey(code: string): number | null {
  const m = /^Digit(\d)$/.exec(code);
  if (!m) return null;
  const d = Number(m[1]);
  return d === 0 ? 9 : d - 1;
}

export class Hotbar {
  private readonly root: HTMLDivElement;
  private readonly slots: HTMLButtonElement[] = [];
  private readonly label: HTMLDivElement;
  /** The build mode (what holding Place does, docs/ITEMS.md §5). */
  private readonly build: HTMLSpanElement;
  /** The region tools' line (selection size, paste mode, the last tool's message). */
  private readonly region: HTMLDivElement;
  private readonly brush: BrushPanel;
  private selected = 0;
  /** The creative slots' palette indices (`Client.creativeHotbar()`). */
  private picks: number[] = Array.from({ length: HOTBAR_SLOTS }, (_, i) => i);
  /** The inventory's hotbar (a world that uses items), shown instead of the palette's slots. */
  private readonly palRow: HTMLDivElement;
  private readonly invRow: HTMLDivElement;
  private readonly wallet: HTMLDivElement;
  private inventory: InventoryHud | null = null;
  private invKey = '';

  constructor(
    parent: HTMLElement,
    private readonly palette: readonly HotbarSlot[],
    onPick: (slot: number) => void,
  ) {
    this.root = document.createElement('div');
    this.root.id = 'hotbar';
    this.label = document.createElement('div');
    this.label.id = 'hotbar-label';
    this.build = document.createElement('span');
    this.build.id = 'hotbar-build';
    const head = document.createElement('div');
    head.className = 'hotbar-head';
    head.append(this.label, this.build);
    this.region = document.createElement('div');
    this.region.id = 'hotbar-region';
    this.region.hidden = true;
    const row = document.createElement('div');
    row.className = 'hotbar-slots';
    for (let i = 0; i < HOTBAR_SLOTS; i++) {
      const b = document.createElement('button');
      b.type = 'button';
      b.className = 'hotbar-slot';
      // Choosing a slot has its own sound (the hotbar action), not a click.
      b.dataset.sound = 'none';
      b.dataset.slot = String(i);
      b.addEventListener('pointerdown', (e) => {
        e.stopPropagation();
        e.preventDefault();
        onPick(i);
      });
      this.slots.push(b);
      row.append(b);
    }
    this.palRow = row;
    this.invRow = document.createElement('div');
    this.invRow.className = 'hotbar-slots';
    this.invRow.hidden = true;
    for (let i = 0; i < HOTBAR_SLOTS; i++) {
      const b = document.createElement('button');
      b.type = 'button';
      b.className = 'hotbar-slot';
      b.dataset.sound = 'none';
      b.dataset.slot = String(i);
      b.addEventListener('pointerdown', (e) => {
        e.stopPropagation();
        e.preventDefault();
        onPick(i);
      });
      this.invRow.append(b);
    }
    this.wallet = document.createElement('div');
    this.wallet.id = 'hotbar-wallet';
    this.wallet.hidden = true;
    this.root.append(this.region, head, row, this.invRow, this.wallet);
    parent.append(this.root);
    this.setBuildMode(BUILD_MODES.modes[0].label);
    this.brush = new BrushPanel(this.root);
    this.root.prepend(this.root.lastChild as Node);
    this.setPicks(this.picks);
  }

  /** The creative slots' palette indices (`Client.creativeHotbar()`): each slot's picture. */
  setPicks(picks: readonly number[]): void {
    this.picks = [...picks];
    this.slots.forEach((b, i) => {
      const slot = this.palette[this.picks[i] ?? -1];
      b.replaceChildren();
      b.title = slot?.name ?? '';
      if (slot) b.append(swatch(slot));
      const k = document.createElement('span');
      k.className = 'hotbar-key';
      k.textContent = slotKey(i);
      b.append(k);
    });
    this.refresh();
  }

  /** The brush tool's panel (`Client.hud().brush`); hidden while the tool is off. */
  setBrush(state: BrushPanelState | null): void {
    this.brush.set(state);
  }

  set visible(v: boolean) {
    this.root.hidden = !v;
  }

  setSelected(slot: number): void {
    this.selected = slot;
    this.slots.forEach((b, i) => b.classList.toggle('selected', i === slot));
    this.refresh();
  }

  /** The build mode's label (`Client.buildMode()`, a `buildMode` event). */
  setBuildMode(label: string): void {
    this.build.textContent = label;
    this.build.title = `Build mode: ${label} (${keyLabel(BUILD_MODES.key)} to switch)`;
  }

  /** The region tools' status line (`Client.hud().region`); hidden when empty. */
  setRegion(text: string): void {
    if (this.region.textContent === text) return;
    this.region.textContent = text;
    this.region.hidden = text === '';
  }

  /**
   * The inventory (ADR 0066): its first slots replace the palette's, each with its count, and the
   * wallet shows; null shows the palette again. `swatchOf` draws an item (`itemSwatch`).
   */
  setInventory(inv: InventoryHud | null, swatchOf: (item: string) => HotbarSlot): void {
    const key = inv ? JSON.stringify([inv.slots.slice(0, HOTBAR_SLOTS), inv.held, inv.money]) : '';
    if (key === this.invKey) return;
    this.invKey = key;
    this.inventory = inv;
    this.palRow.hidden = inv !== null;
    this.invRow.hidden = inv === null;
    this.wallet.hidden = inv === null;
    if (!inv) {
      this.refresh();
      return;
    }
    this.selected = inv.held;
    [...this.invRow.children].forEach((el, i) => {
      const b = el as HTMLButtonElement;
      const s = inv.slots[i] ?? null;
      b.replaceChildren();
      b.classList.toggle('selected', i === inv.held);
      b.title = s ? `${itemLabel(s.item)} × ${String(s.count)}` : '';
      if (s) {
        b.append(swatch(swatchOf(s.item)));
        const n = document.createElement('span');
        n.className = 'hotbar-count';
        n.textContent = String(s.count);
        b.append(n);
        const bar = wearBar(s.item, s.wear);
        if (bar) b.append(bar);
      }
      const k = document.createElement('span');
      k.className = 'hotbar-key';
      k.textContent = slotKey(i);
      b.append(k);
    });
    this.wallet.textContent = `◆ ${String(inv.money)} / ${String(inv.capacity)}`;
    this.wallet.title = 'Gems in your wallet';
    this.refresh();
  }

  private refresh(): void {
    if (this.inventory) {
      const s = this.inventory.slots[this.inventory.held] ?? null;
      this.label.textContent = s ? itemLabel(s.item) : '';
      return;
    }
    const slot = this.palette[this.picks[this.selected] ?? -1];
    this.label.textContent = (slot?.name ?? '').replace(/_/g, ' ');
  }
}

/** A slot's picture: its block's side texture, or its flat colour. */
function swatch(slot: HotbarSlot): HTMLCanvasElement {
  const canvas = document.createElement('canvas');
  canvas.width = slot.size;
  canvas.height = slot.size;
  const ctx = canvas.getContext('2d');
  if (!ctx || slot.swatch.length !== slot.size * slot.size * 4) return canvas;
  ctx.putImageData(new ImageData(new Uint8ClampedArray(slot.swatch), slot.size, slot.size), 0, 0);
  return canvas;
}
