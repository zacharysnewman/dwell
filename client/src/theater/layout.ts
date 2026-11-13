// Theater mode's panel layout and its controller navigation (docs/THEATER_MODE.md §2; owner,
// 2026-11-13: "Improve the UI on the theater mode. It's really bad, it doesn't even have a proper X
// button or responsive layout."). Pure helpers: the panel (mode.ts) applies the class the viewport
// picks and moves the focus with the controller's d-pad; tests drive both.

/** Where the panel sits: docked beside the view, a bottom sheet (portrait), a side sheet (landscape). */
export type TheaterLayout = 'docked' | 'sheet-bottom' | 'sheet-side';

/** Each layout's class on the panel (style.css). */
export const LAYOUT_CLASS: Readonly<Record<TheaterLayout, string>> = {
  docked: 'tb-docked',
  'sheet-bottom': 'tb-sheet-bottom',
  'sheet-side': 'tb-sheet-side',
};

/** The smallest viewport (CSS px) that docks the panel; anything smaller is a phone's sheet. */
export const DOCK_MIN_W = 900;
export const DOCK_MIN_H = 560;

/** The layout for a viewport of `width` × `height` CSS px. */
export function layoutFor(width: number, height: number): TheaterLayout {
  if (width >= DOCK_MIN_W && height >= DOCK_MIN_H) return 'docked';
  return height >= width ? 'sheet-bottom' : 'sheet-side';
}

/** A take's running time as the recording badge shows it: `0:07`, `1:05`, `12:00`. */
export function timerText(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${String(Math.floor(s / 60))}:${String(s % 60).padStart(2, '0')}`;
}

const FOCUSABLE = 'button, input, select, summary, [tabindex="0"]';

/** Whether `e` can take the focus now: not disabled, not hidden, not inside a closed disclosure. */
function reachable(e: HTMLElement, root: HTMLElement): boolean {
  if ((e as HTMLButtonElement).disabled || e.hidden) return false;
  if (e instanceof HTMLInputElement && e.type === 'file') return false;
  for (let p = e.parentElement; p && p !== root.parentElement; p = p.parentElement) {
    if (p.hidden) return false;
    // A closed <details> shows only its own summary.
    const summary = e.tagName === 'SUMMARY' && e.parentElement === p;
    if (p.tagName === 'DETAILS' && !(p as HTMLDetailsElement).open && !summary) return false;
  }
  return true;
}

/** The controls the focus moves through, in document order. */
export function focusables(root: HTMLElement): HTMLElement[] {
  return [...root.querySelectorAll<HTMLElement>(FOCUSABLE)].filter((e) => reachable(e, root));
}

/** The accessible name a control has: its aria-label, its own text, or its label's text. */
export function accessibleName(e: HTMLElement): string {
  const aria = e.getAttribute('aria-label');
  if (aria?.trim()) return aria.trim();
  if (e instanceof HTMLInputElement || e instanceof HTMLSelectElement) {
    const label = e.closest('label');
    if (label?.textContent.trim()) return label.textContent.trim();
    if (e.title.trim()) return e.title.trim();
    return '';
  }
  return e.textContent.trim();
}

/** The standard gamepad's buttons the panel reads. */
export const PAD = { a: 0, b: 1, up: 12, down: 13, left: 14, right: 15 } as const;

/** What a frame's d-pad and face buttons did to the panel. */
export type PadAction = 'close' | null;

/**
 * The controller in the panel: the d-pad moves the focus (up/down through the controls;
 * left/right steps a slider or a choice, else moves too), A presses, B closes. Edges only: a held
 * button acts once.
 */
export class PadNav {
  private was = new Set<number>();

  step(pad: { buttons: readonly { pressed: boolean }[] }, root: HTMLElement): PadAction {
    const now = new Set<number>();
    pad.buttons.forEach((b, i) => {
      if (b.pressed) now.add(i);
    });
    const was = this.was;
    const edge = (i: number): boolean => now.has(i) && !was.has(i);
    this.was = now;
    const doc = root.ownerDocument;
    const list = focusables(root);
    const at = list.indexOf(doc.activeElement as HTMLElement);
    const go = (d: number): void => {
      if (list.length === 0) return;
      const i = at < 0 ? (d > 0 ? 0 : list.length - 1) : (at + d + list.length) % list.length;
      list[i]?.focus();
    };
    if (edge(PAD.b)) return 'close';
    if (edge(PAD.up)) go(-1);
    if (edge(PAD.down)) go(1);
    for (const [btn, d] of [
      [PAD.left, -1],
      [PAD.right, 1],
    ] as const) {
      if (!edge(btn)) continue;
      const f = list[at];
      if (f instanceof HTMLInputElement && f.type === 'range') {
        const step = Number(f.step) || 1;
        f.value = String(
          Math.min(Number(f.max), Math.max(Number(f.min), Number(f.value) + d * step * 10)),
        );
        f.dispatchEvent(new Event('input', { bubbles: true }));
      } else if (f instanceof HTMLSelectElement) {
        const n = f.options.length;
        if (n > 0) {
          f.selectedIndex = (f.selectedIndex + d + n) % n;
          f.dispatchEvent(new Event('change', { bubbles: true }));
        }
      } else go(d);
    }
    if (edge(PAD.a)) {
      const f = list[at];
      if (f) f.click();
    }
    return null;
  }
}
