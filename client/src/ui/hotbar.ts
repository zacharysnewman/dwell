// The block hotbar (ARCHITECTURE.md §6.5): the infinite palette with the selected block highlighted.
// Number keys and the scroll wheel (desktop) or tapping a slot (touch) change the selection.

import { BrushPanel, type BrushPanelState } from './brushPanel';

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
  /** The region tools' line (selection size, paste mode, the last tool's message). */
  private readonly region: HTMLDivElement;
  private readonly brush: BrushPanel;
  private selected = 0;

  constructor(
    parent: HTMLElement,
    private readonly palette: readonly HotbarSlot[],
    onPick: (slot: number) => void,
  ) {
    this.root = document.createElement('div');
    this.root.id = 'hotbar';
    this.label = document.createElement('div');
    this.label.id = 'hotbar-label';
    this.region = document.createElement('div');
    this.region.id = 'hotbar-region';
    this.region.hidden = true;
    const row = document.createElement('div');
    row.className = 'hotbar-slots';
    palette.forEach((slot, i) => {
      const b = document.createElement('button');
      b.type = 'button';
      b.className = 'hotbar-slot';
      // Choosing a slot has its own sound (the hotbar action), not a click.
      b.dataset.sound = 'none';
      b.dataset.slot = String(i);
      b.title = slot.name;
      b.append(swatch(slot));
      const key = slotKey(i);
      if (key) {
        const k = document.createElement('span');
        k.className = 'hotbar-key';
        k.textContent = key;
        b.append(k);
      }
      b.addEventListener('pointerdown', (e) => {
        e.stopPropagation();
        e.preventDefault();
        onPick(i);
      });
      this.slots.push(b);
      row.append(b);
    });
    this.root.append(this.region, this.label, row);
    parent.append(this.root);
    this.brush = new BrushPanel(this.root);
    this.root.prepend(this.root.lastChild as Node);
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

  /** The region tools' status line (`Client.hud().region`); hidden when empty. */
  setRegion(text: string): void {
    if (this.region.textContent === text) return;
    this.region.textContent = text;
    this.region.hidden = text === '';
  }

  private refresh(): void {
    const slot = this.palette[this.selected];
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
