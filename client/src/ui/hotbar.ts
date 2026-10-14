// The block hotbar (ARCHITECTURE.md §6.5): the infinite palette with the selected block highlighted.
// Number keys and the scroll wheel (desktop) or tapping a slot (touch) change the selection.

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
    const row = document.createElement('div');
    row.className = 'hotbar-slots';
    palette.forEach((slot, i) => {
      const b = document.createElement('button');
      b.type = 'button';
      b.className = 'hotbar-slot';
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
    this.root.append(this.label, row);
    parent.append(this.root);
    this.refresh();
  }

  set visible(v: boolean) {
    this.root.hidden = !v;
  }

  setSelected(slot: number): void {
    this.selected = slot;
    this.slots.forEach((b, i) => b.classList.toggle('selected', i === slot));
    this.refresh();
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
