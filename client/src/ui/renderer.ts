// The renderer switch's shell side (docs/RAY_MARCHED_RENDERER.md §3, §5): which terrain renderer
// the page asks for, the settings menu's choices, and compare mode's overlay (a label per side and
// a draggable divider). Local builds only: `__RENDERER_SWITCH__` (vite.config.ts) is false in
// deployed builds, which keep the mesher. The renderers themselves are the Rust client's.

/** A choice in the settings menu: `renderer` and compare mode's right side (`?renderer=`, `?compare=`). */
export interface RendererChoice {
  id: string;
  label: string;
  renderer: string;
  compare?: string;
}

export const RENDERER_CHOICES: RendererChoice[] = [
  { id: 'raster', label: 'Raster (mesher)', renderer: 'raster' },
  { id: 'march', label: 'Ray marched', renderer: 'march' },
  { id: 'compare', label: 'Compare: raster | march', renderer: 'compare' },
  {
    id: 'compare-raster',
    label: 'Compare: raster | raster',
    renderer: 'compare',
    compare: 'raster',
  },
];

/** What the page asks for: `?renderer=` and `?compare=`, only with the switch. */
export function rendererParams(
  search: string,
  enabled: boolean,
): { renderer?: string; compare?: string } {
  if (!enabled) return {};
  const p = new URLSearchParams(search);
  return { renderer: p.get('renderer') ?? undefined, compare: p.get('compare') ?? undefined };
}

/** The choice matching the client's label (`raster`, `march`, `compare raster | march`). */
export function choiceOf(label: string): RendererChoice | undefined {
  const compare = /^compare (\w+) \| (\w+)$/.exec(label);
  if (compare) {
    return RENDERER_CHOICES.find(
      (c) => c.renderer === 'compare' && (c.compare ?? 'march') === compare[2],
    );
  }
  return RENDERER_CHOICES.find((c) => c.renderer === label);
}

/** The divider's place, a share of the canvas's width, from a pointer's x (clamped to it). */
export function dividerShare(clientX: number, left: number, width: number): number {
  if (!(width > 0)) return 0.5;
  return Math.min(1, Math.max(0, (clientX - left) / width));
}

/** Compare mode's state as the client reports it (`compareState()`). */
export interface CompareState {
  renderer: string;
  view: 'split' | 'blink' | 'difference';
  divider: number;
  blinkRight: boolean;
  overlay: [string, string];
  difference: { pixels: number; coverage: number; depth: number; colour: number } | null;
}

/** The difference view's line: how many pixels disagree, and how. */
export function formatDifference(d: CompareState['difference']): string {
  if (!d) return 'difference: waiting for the first count';
  const differing = d.coverage + d.depth + d.colour;
  const pct = d.pixels > 0 ? (100 * differing) / d.pixels : 0;
  return `difference ${String(differing)} of ${String(d.pixels)} px (${pct.toFixed(3)} %) · coverage ${String(d.coverage)} · depth ${String(d.depth)} · colour ${String(d.colour)}`;
}

/** Compare mode's overlay: a label per side, and in split view the divider to drag. */
export class CompareOverlay {
  private readonly root = document.createElement('div');
  private readonly left = document.createElement('div');
  private readonly right = document.createElement('div');
  private readonly divider = document.createElement('div');
  private dragging = false;

  constructor(
    parent: HTMLElement,
    private readonly canvas: HTMLElement,
    private readonly onDivider: (share: number) => void,
  ) {
    this.root.id = 'compare-overlay';
    this.root.hidden = true;
    this.left.className = 'compare-label compare-left';
    this.right.className = 'compare-label compare-right';
    this.divider.className = 'compare-divider';
    this.divider.title = 'Drag to move the split';
    this.root.append(this.divider, this.left, this.right);
    parent.append(this.root);
    const move = (e: PointerEvent): void => {
      if (!this.dragging) return;
      const r = this.canvas.getBoundingClientRect();
      this.onDivider(dividerShare(e.clientX, r.left, r.width));
    };
    this.divider.addEventListener('pointerdown', (e) => {
      this.dragging = true;
      this.divider.setPointerCapture(e.pointerId);
      e.preventDefault();
      e.stopPropagation();
    });
    this.divider.addEventListener('pointermove', move);
    const stop = (): void => {
      this.dragging = false;
    };
    this.divider.addEventListener('pointerup', stop);
    this.divider.addEventListener('pointercancel', stop);
  }

  /** Shows the state after a frame (null: not in compare mode). */
  show(state: CompareState | null): void {
    this.root.hidden = state === null;
    if (!state) return;
    const split = state.view === 'split';
    this.divider.hidden = !split;
    this.divider.style.left = `${String(state.divider * 100)}%`;
    const [l, r] = state.overlay;
    if (state.view === 'difference') {
      this.left.textContent = `${l} vs ${r}`;
      this.right.textContent = formatDifference(state.difference);
    } else if (state.view === 'blink') {
      this.left.textContent = `blink (F8) · showing ${state.blinkRight ? r : l}`;
      this.right.textContent = '';
    } else {
      this.left.textContent = l;
      this.right.textContent = r;
    }
    this.right.hidden = this.right.textContent === '';
  }
}
