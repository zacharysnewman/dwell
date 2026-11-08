// The on-screen interface's layout (owner, 2026-11-08: "The UI needs a relayout on mobile, it's all
// overlapping itself"): each part's anchored place, priority and narrowest width are data
// (shared/ui/layout.json → layout.gen.ts), and a small solver places the visible parts on this
// screen so no two overlap and each stays inside the safe area. `solveLayout` is pure (the tests run
// it over phone, tablet and desktop sizes); `applyLayout` measures the page's parts, solves, and
// moves them (inline left/top, the CSS keeps their looks).

export type Anchor = 'top-left' | 'top' | 'top-right' | 'bottom-left' | 'bottom' | 'bottom-right';

/** How a part is placed (layout.json). */
export interface Placement {
  anchor?: Anchor;
  flush?: boolean;
  after?: string;
  minWidthPx?: number;
}

export interface LayoutPart extends Placement {
  id: string;
  selector: string;
  priority?: number;
  fixed?: boolean;
  clearPx?: number;
  touch?: Placement;
  landscape?: Placement;
  portrait?: Placement;
}

export interface KeepClear {
  id: string;
  x: number[];
  y: number[];
  touch: boolean;
  orientation: string;
}

export interface HudLayout {
  gapPx: number;
  keepClear: KeepClear[];
  parts: LayoutPart[];
}

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** The screen: its size, and the safe area's insets (the notch, the home bar; at least the margin). */
export interface Screen {
  w: number;
  h: number;
  safe: { l: number; t: number; r: number; b: number };
  touch: boolean;
}

/** A visible part's size (its natural width and height), or a fixed part's rectangle. */
export type Measured = { w: number; h: number } | { rect: Rect };

/** Each visible part's place (null: no room for it on this screen). */
export type Solved = Map<string, Rect | null>;

export function overlaps(a: Rect, b: Rect): boolean {
  return a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
}

/** The regions kept clear on this screen, as rectangles. */
export function clearRegions(layout: HudLayout, screen: Screen): Rect[] {
  const landscape = screen.w > screen.h;
  return layout.keepClear
    .filter((c) => !c.touch || screen.touch)
    .filter((c) => c.orientation === 'any' || (c.orientation === 'landscape') === landscape)
    .map((c) => ({
      x: (c.x[0] ?? 0) * screen.w,
      y: (c.y[0] ?? 0) * screen.h,
      w: ((c.x[1] ?? 1) - (c.x[0] ?? 0)) * screen.w,
      h: ((c.y[1] ?? 1) - (c.y[0] ?? 0)) * screen.h,
    }));
}

/**
 * Places the measured parts (`sizes`: only the visible ones) on `screen`. In priority order, each
 * part starts at its anchor; where placed parts or kept-clear regions are in its row it takes the
 * free stretch on its side (shrinking to `minWidthPx` at most), or moves on (down from a top anchor,
 * up from a bottom one) past them to the next row with room.
 */
export function solveLayout(
  layout: HudLayout,
  screen: Screen,
  sizes: Map<string, Measured>,
): Solved {
  const gap = layout.gapPx;
  const { w: W, h: H, safe } = screen;
  const taken: Rect[] = clearRegions(layout, screen);
  const out: Solved = new Map();
  for (const p of layout.parts) {
    const m = sizes.get(p.id);
    if (p.fixed && m && 'rect' in m) {
      const c = p.clearPx ?? 0;
      taken.push({ x: m.rect.x - c, y: m.rect.y - c, w: m.rect.w + 2 * c, h: m.rect.h + 2 * c });
      out.set(p.id, m.rect);
    }
  }
  const order = layout.parts
    .map((p, i) => ({ p, i }))
    .filter(({ p }) => !p.fixed)
    .sort((a, b) => (a.p.priority ?? 0) - (b.p.priority ?? 0) || a.i - b.i);
  for (const { p } of order) {
    const m = sizes.get(p.id);
    if (!m || 'rect' in m) continue;
    const spec: Placement = {
      ...p,
      ...(screen.touch ? p.touch : {}),
      ...(screen.w > screen.h ? p.landscape : p.portrait),
    };
    const anchor = spec.anchor ?? 'top-left';
    const down = anchor.startsWith('top');
    const side = anchor.endsWith('left') ? 'left' : anchor.endsWith('right') ? 'right' : 'centre';
    const h = m.h;
    const minW = Math.min(m.w, spec.minWidthPx ?? m.w);
    const lo = safe.t;
    const hi = H - safe.b - h;
    let y0 = down ? lo : hi;
    const after = spec.after === undefined ? null : out.get(spec.after);
    if (after) y0 = down ? Math.max(y0, after.y + after.h + gap) : Math.min(y0, after.y - gap - h);
    // The rows to try: the start, then just past each placed thing, in the scan's direction.
    const ys = [y0];
    for (const t of taken) ys.push(down ? t.y + t.h + gap : t.y - gap - h);
    const rows = ys
      .filter((y) => (down ? y >= y0 : y <= y0) && y >= lo - 0.01 && y <= hi + 0.01)
      .sort((a, b) => (down ? a - b : b - a));
    // A centred part looks for a row whose free stretch holds the screen's middle first (the
    // desktop hotbar moves up past the health bar rather than aside), then for any stretch.
    let placed: Rect | null = null;
    for (const strict of side === 'centre' ? [true, false] : [false]) {
      for (const y of rows) {
        const spans = freeSpans(taken, y, h, gap, safe.l, W - safe.r);
        const fits = spans.filter(
          ([a, b]) =>
            b - a >= minW - 0.01 &&
            (!spec.flush ||
              (side === 'left' && a <= safe.l + 0.01) ||
              (side === 'right' && b >= W - safe.r - 0.01) ||
              side === 'centre') &&
            (!strict || (a <= W / 2 && W / 2 <= b)),
        );
        if (fits.length === 0) continue;
        let span: [number, number];
        if (side === 'left') span = fits[0] as [number, number];
        else if (side === 'right') span = fits[fits.length - 1] as [number, number];
        else {
          // The stretch holding the screen's middle, or the one nearest it.
          const mid = W / 2;
          const dist = ([a, b]: [number, number]) => (mid < a ? a - mid : mid > b ? mid - b : 0);
          span = fits.reduce((best, s) => (dist(s) < dist(best) ? s : best));
        }
        const w = Math.min(m.w, span[1] - span[0]);
        const x =
          side === 'left'
            ? span[0]
            : side === 'right'
              ? span[1] - w
              : Math.min(Math.max(W / 2 - w / 2, span[0]), span[1] - w);
        placed = { x, y, w, h };
        break;
      }
      if (placed) break;
    }
    out.set(p.id, placed);
    if (placed) taken.push(placed);
  }
  return out;
}

/** The free stretches of the row [y, y + h) between `left` and `right`, `gap` clear of `taken`. */
function freeSpans(
  taken: Rect[],
  y: number,
  h: number,
  gap: number,
  left: number,
  right: number,
): [number, number][] {
  const blocks = taken
    .filter((t) => t.y < y + h + gap - 0.01 && t.y + t.h + gap > y + 0.01)
    .map((t) => [t.x - gap, t.x + t.w + gap] as [number, number])
    .sort((a, b) => a[0] - b[0]);
  const spans: [number, number][] = [];
  let x = left;
  for (const [a, b] of blocks) {
    if (a > x) spans.push([x, Math.min(a, right)]);
    x = Math.max(x, b);
    if (x >= right) break;
  }
  if (x < right) spans.push([x, right]);
  return spans.filter(([a, b]) => b > a);
}

/** The safe area's insets, read from a probe laid out by the stylesheet's --safe-* margins. */
function safeInsets(doc: Document, w: number, h: number): Screen['safe'] {
  let probe = doc.getElementById('safe-probe');
  if (!probe) {
    probe = doc.createElement('div');
    probe.id = 'safe-probe';
    doc.body.append(probe);
  }
  const r = probe.getBoundingClientRect();
  if (r.width === 0) return { l: 10, t: 10, r: 10, b: 10 };
  return { l: r.left, t: r.top, r: w - r.right, b: h - r.bottom };
}

const PLACED = ['left', 'top', 'right', 'bottom', 'transform', 'maxWidth', 'visibility'] as const;

/**
 * Measures the page's visible parts, solves the layout, and moves them there. A part with no room
 * is hidden (visibility, so it is measured again next time).
 */
export function applyLayout(doc: Document, layout: HudLayout, touch: boolean): Solved {
  const win = doc.defaultView;
  const W = win?.innerWidth ?? 0;
  const H = win?.innerHeight ?? 0;
  const els = new Map<string, HTMLElement>();
  const sizes = new Map<string, Measured>();
  for (const p of layout.parts) {
    const el = doc.querySelector<HTMLElement>(p.selector);
    if (!el) continue;
    if (!p.fixed) {
      // Its size as the stylesheet lays it out (within the stylesheet's cap: wrapped text measures
      // its wrapped height); the solver only narrows it from there.
      for (const k of PLACED) el.style[k] = '';
    }
    if (el.getClientRects().length === 0) continue;
    const r = el.getBoundingClientRect();
    if (r.width === 0 || r.height === 0) continue;
    els.set(p.id, el);
    sizes.set(
      p.id,
      p.fixed
        ? { rect: { x: r.left, y: r.top, w: r.width, h: r.height } }
        : { w: r.width, h: r.height },
    );
  }
  const screen: Screen = { w: W, h: H, safe: safeInsets(doc, W, H), touch };
  const solved = solveLayout(layout, screen, sizes);
  for (const p of layout.parts) {
    const el = els.get(p.id);
    if (!el || p.fixed) continue;
    const r = solved.get(p.id);
    if (!r) {
      el.style.visibility = 'hidden';
      continue;
    }
    // Relative to its containing block (the viewport, or a full-screen layer).
    const parent = el.offsetParent?.getBoundingClientRect() ?? { left: 0, top: 0 };
    el.style.left = `${String(r.x - parent.left)}px`;
    el.style.top = `${String(r.y - parent.top)}px`;
    el.style.right = 'auto';
    el.style.bottom = 'auto';
    el.style.transform = 'none';
    // max-width counts the content box unless the part sizes its border box.
    const cs = win?.getComputedStyle(el);
    const edges =
      cs && cs.boxSizing !== 'border-box'
        ? ['paddingLeft', 'paddingRight', 'borderLeftWidth', 'borderRightWidth'].reduce(
            (sum, k) =>
              sum +
              (parseFloat(cs.getPropertyValue(k.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`))) ||
                0),
            0,
          )
        : 0;
    el.style.maxWidth = `${String(Math.floor(r.w - edges))}px`;
  }
  return solved;
}

/**
 * Runs the layout from the frame loop: at once when the screen is resized or a part changes size (a
 * part shown, the lines' text, the hotbar's rows; a ResizeObserver on each part) or a part is made, and
 * not otherwise: on a software renderer restyling the blurred parts costs frames.
 */
export class LayoutRunner {
  private dirty = true;
  private last = -Infinity;
  private readonly seen = new WeakSet<Element>();
  private readonly observer: ResizeObserver | null;

  constructor(
    private readonly doc: Document,
    private readonly layout: HudLayout,
    private readonly touch: boolean,
    private readonly everyMs = 250,
  ) {
    this.observer =
      typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(() => (this.dirty = true));
    doc.defaultView?.addEventListener('resize', () => (this.dirty = true));
  }

  frame(now: number): void {
    // Every `everyMs`, only a look for parts made since (cheap); the layout itself runs only when
    // something changed (without a ResizeObserver, every `everyMs`).
    if (now - this.last >= this.everyMs) {
      this.last = now;
      if (!this.observer) this.dirty = true;
      else
        for (const p of this.layout.parts) {
          const el = this.doc.querySelector(p.selector);
          if (el && !this.seen.has(el)) {
            this.seen.add(el);
            this.observer.observe(el);
            this.dirty = true;
          }
        }
    }
    if (!this.dirty) return;
    this.dirty = false;
    applyLayout(this.doc, this.layout, this.touch);
  }
}
