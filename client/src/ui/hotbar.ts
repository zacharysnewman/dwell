// The block hotbar (ARCHITECTURE.md §6.5): the infinite palette with the selected block highlighted.
// Number keys and the scroll wheel (desktop) or tapping a slot (touch) change the selection.
import type { PaletteSlot } from '../interact/blockInteraction';
import { sharedAtlas, TILE, tilePixels } from '../render/textures';
import { SHAPES } from '../world/blocks';
import { materialStyle } from '../world/materials';

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

  constructor(
    parent: HTMLElement,
    private readonly palette: readonly PaletteSlot[],
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
      b.append(swatch(slot.material));
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
  }

  set visible(v: boolean) {
    this.root.hidden = !v;
  }

  setSelected(slot: number): void {
    this.slots.forEach((b, i) => b.classList.toggle('selected', i === slot));
    this.label.textContent = (this.palette[slot]?.name ?? '').replace(/_/g, ' ');
  }
}

/** A small picture of a material: its side texture, or its flat colour. */
function swatch(material: number): HTMLCanvasElement {
  const canvas = document.createElement('canvas');
  canvas.width = TILE;
  canvas.height = TILE;
  const ctx = canvas.getContext('2d');
  if (!ctx) return canvas;
  const style = materialStyle(material);
  if (style.textures) {
    ctx.putImageData(
      new ImageData(tilePixels(sharedAtlas(), style.textures.side), TILE, TILE),
      0,
      0,
    );
  } else {
    ctx.fillStyle = `#${style.color.toString(16).padStart(6, '0')}`;
    ctx.fillRect(0, 0, TILE, TILE);
  }
  const shape = style.look === 'shaped' ? SHAPES[style.shape] : undefined;
  if (shape && !shape.inverted) ctx.clearRect(0, 0, TILE, TILE * (1 - shape.maxY));
  return canvas;
}
