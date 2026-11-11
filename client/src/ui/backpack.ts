// The backpack screen (docs/adr/0066-inventory-and-drops.md §10): every inventory slot, the
// backpack's rows above the hotbar's, in a world that uses items. Pick a stack up with a press and
// put it down on another slot (a drag or two clicks); a right press (or Shift) takes half. The
// server applies the move (`Client.moveItems`) and sends the inventory back.

import { BACKPACK_SLOTS, HOTBAR_SLOTS, RECIPES, type Recipe } from './inventory.gen';
import { itemLabel, wearBar, type HotbarSlot, type InventoryHud } from './hotbar';

/**
 * The recipes the inventory can make (owner, 2026-11-11: combining) and how many times each, in
 * the data's order; those it can't make are left out. The server decides (`Client.combine`).
 */
export function combinable(
  inv: InventoryHud,
  recipes: readonly Recipe[] = RECIPES,
): { recipe: Recipe; times: number }[] {
  const have = new Map<string, number>();
  for (const s of inv.slots) if (s) have.set(s.item, (have.get(s.item) ?? 0) + s.count);
  return recipes
    .map((recipe) => ({
      recipe,
      times: Math.min(...recipe.inputs.map((i) => Math.floor((have.get(i.item) ?? 0) / i.count))),
    }))
    .filter((r) => r.times > 0);
}

/** A recipe's inputs as text: "2 Stick + 2 Rock shard". */
export function recipeInputs(r: Recipe): string {
  return r.inputs.map((i) => `${String(i.count)} ${itemLabel(i.item)}`).join(' + ');
}

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

/**
 * What a key does to the backpack screen: E toggles it (where the world uses items), Escape closes
 * it; the new open state, or null when the key is not the backpack's. E does nothing else: a
 * ground item is knocked loose by the break action, not picked up with E (ADR 0071, amended
 * 2026-11-09).
 */
export function backpackKey(
  code: string,
  repeat: boolean,
  open: boolean,
  usable: boolean,
): boolean | null {
  if (code === 'KeyE' && !repeat && usable) return !open;
  if (code === 'Escape' && open) return false;
  return null;
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
  private readonly combineList: HTMLDivElement;
  private readonly combineRoot: HTMLDivElement;
  private inv: InventoryHud | null = null;
  private picked: { slot: number; half: boolean } | null = null;
  private key = '';

  constructor(
    parent: HTMLElement,
    private readonly swatchOf: (item: string) => HotbarSlot,
    private readonly move: (from: number, to: number, count: number) => void,
    private readonly combine: (recipe: string) => void = () => undefined,
  ) {
    this.root = document.createElement('div');
    this.root.id = 'backpack';
    this.root.className = 'inventory-view';
    this.root.hidden = true;
    // A dialog the controller's menu focus moves in (docs/GAMEPAD.md): A on a slot presses it.
    this.root.setAttribute('role', 'dialog');
    this.root.setAttribute('aria-modal', 'true');
    this.root.setAttribute('aria-label', 'Backpack');
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
      // A press with no pointer (the controller's A, Enter): pick up or put down, as a click.
      b.addEventListener('click', (e) => {
        if (e.detail === 0) this.press(slot, false);
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
    // Combine (owner, 2026-11-11): the recipes the inventory can make, a button each (where the
    // server makes recipes; an older server's backpack has none).
    this.combineRoot = document.createElement('div');
    this.combineRoot.className = 'backpack-combine';
    const ct = document.createElement('div');
    ct.className = 'backpack-title';
    ct.textContent = 'Combine';
    this.combineList = document.createElement('div');
    this.combineList.className = 'combine-list';
    this.combineRoot.append(ct, this.combineList);
    this.root.append(title, this.grid, this.combineRoot);
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
      const bar = wearBar(s.item, s.wear);
      if (bar) b.append(bar);
    }
    this.drawCombine();
  }

  private drawCombine(): void {
    if (!this.inv) return;
    const can = this.inv.combines ? combinable(this.inv) : [];
    this.combineRoot.hidden = !this.inv.combines;
    this.combineList.replaceChildren();
    if (can.length === 0) {
      const none = document.createElement('div');
      none.className = 'combine-none';
      none.textContent = 'Nothing to combine yet';
      this.combineList.append(none);
      return;
    }
    for (const { recipe, times } of can) {
      const b = document.createElement('button');
      b.type = 'button';
      b.className = 'combine-recipe';
      b.dataset.recipe = recipe.name;
      const out = recipe.output.count > 1 ? `${String(recipe.output.count)} ` : '';
      const name = document.createElement('span');
      name.className = 'combine-output';
      name.textContent = `${out}${itemLabel(recipe.output.item)}`;
      const ins = document.createElement('span');
      ins.className = 'combine-inputs';
      ins.textContent = recipeInputs(recipe);
      const n = document.createElement('span');
      n.className = 'combine-times';
      n.textContent = `×${String(times)}`;
      b.append(name, ins, n);
      b.addEventListener('pointerdown', (e) => {
        e.preventDefault();
        e.stopPropagation();
        this.combine(recipe.name);
      });
      b.addEventListener('click', (e) => {
        if (e.detail === 0) this.combine(recipe.name);
      });
      this.combineList.append(b);
    }
  }
}
