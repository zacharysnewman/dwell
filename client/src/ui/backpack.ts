// The backpack screen (docs/adr/0066-inventory-and-drops.md §10): every inventory slot, the
// backpack's rows above the hotbar's, in a world that uses items. Pick a stack up with a press and
// put it down on another slot (a drag or two clicks); a right press (or Shift) takes half. The
// server applies the move (`Client.moveItems`) and sends the inventory back.

import { BACKPACK_SLOTS, HOTBAR_SLOTS } from './inventory.gen';
import { itemLabel, type HotbarSlot, type InventoryHud } from './hotbar';

/** The move a pick-up and a put-down make: from, to, count (0: the whole stack). */
export function moveFor(
  inv: InventoryHud,
  from: number,
  to: number,
  half: boolean,
): [number, number, number] | null {
  const s = inv.slots[from];
  if (!s || from === to) return null;
  return [from, to, half ? Math.ceil(s.count / 2) : 0];
}

/** The slots in screen order: the backpack's rows, then the hotbar's. */
export function screenOrder(): number[] {
  const back = Array.from({ length: BACKPACK_SLOTS }, (_, i) => HOTBAR_SLOTS + i);
  const hot = Array.from({ length: HOTBAR_SLOTS }, (_, i) => i);
  return [...back, ...hot];
}

export class Backpack {
  private readonly root: HTMLDivElement;
  private readonly grid: HTMLDivElement;
  private readonly cells = new Map<number, HTMLButtonElement>();
  private inv: InventoryHud | null = null;
  private picked: { slot: number; half: boolean } | null = null;
  private key = '';

  constructor(
    parent: HTMLElement,
    private readonly swatchOf: (item: string) => HotbarSlot,
    private readonly move: (from: number, to: number, count: number) => void,
  ) {
    this.root = document.createElement('div');
    this.root.id = 'backpack';
    this.root.hidden = true;
    const title = document.createElement('div');
    title.className = 'backpack-title';
    title.textContent = 'Backpack';
    this.grid = document.createElement('div');
    this.grid.className = 'backpack-grid';
    this.grid.style.gridTemplateColumns = `repeat(${String(HOTBAR_SLOTS)}, 44px)`;
    for (const slot of screenOrder()) {
      const b = document.createElement('button');
      b.type = 'button';
      b.className = 'hotbar-slot backpack-slot';
      if (slot < HOTBAR_SLOTS) b.classList.add('backpack-hotbar');
      b.dataset.slot = String(slot);
      b.addEventListener('contextmenu', (e) => {
        e.preventDefault();
      });
      b.addEventListener('pointerdown', (e) => {
        e.preventDefault();
        e.stopPropagation();
        this.press(slot, e.button === 2 || e.shiftKey);
      });
      b.addEventListener('pointerup', (e) => {
        if (this.picked && this.picked.slot !== slot) {
          e.preventDefault();
          this.press(slot, false);
        }
      });
      this.cells.set(slot, b);
      this.grid.append(b);
    }
    this.root.append(title, this.grid);
    parent.append(this.root);
  }

  /** Whether there is an inventory to show (a world that uses items). */
  get usable(): boolean {
    return this.inv !== null;
  }

  get open(): boolean {
    return !this.root.hidden;
  }

  set open(v: boolean) {
    this.root.hidden = !v;
    this.picked = null;
    this.draw(true);
  }

  /** The inventory from the HUD; null (creative) closes the screen. */
  set(inv: InventoryHud | null): void {
    this.inv = inv;
    if (!inv && this.open) this.open = false;
    this.draw(false);
  }

  private press(slot: number, half: boolean): void {
    if (!this.inv) return;
    if (!this.picked) {
      if (this.inv.slots[slot]) this.picked = { slot, half };
    } else {
      const m = moveFor(this.inv, this.picked.slot, slot, this.picked.half);
      this.picked = null;
      if (m) this.move(...m);
    }
    this.draw(true);
  }

  private draw(force: boolean): void {
    if (!this.inv || !this.open) return;
    const key = JSON.stringify([this.inv.slots, this.picked]);
    if (!force && key === this.key) return;
    this.key = key;
    for (const [slot, b] of this.cells) {
      const s = this.inv.slots[slot] ?? null;
      b.replaceChildren();
      b.classList.toggle('selected', this.picked?.slot === slot);
      b.title = s ? `${itemLabel(s.item)} × ${String(s.count)}` : '';
      if (!s) continue;
      const pic = this.swatchOf(s.item);
      const canvas = document.createElement('canvas');
      canvas.width = pic.size;
      canvas.height = pic.size;
      const ctx = canvas.getContext('2d');
      if (ctx && pic.swatch.length === pic.size * pic.size * 4) {
        ctx.putImageData(
          new ImageData(new Uint8ClampedArray(pic.swatch), pic.size, pic.size),
          0,
          0,
        );
      }
      const n = document.createElement('span');
      n.className = 'hotbar-count';
      n.textContent = String(s.count);
      b.append(canvas, n);
    }
  }
}
