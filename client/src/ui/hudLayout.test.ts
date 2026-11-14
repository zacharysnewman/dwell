import { describe, expect, it } from 'vitest';
import { HUD_ELEMENTS } from './hud.gen';
import {
  clearRegions,
  type HudLayout,
  type Measured,
  overlaps,
  type Rect,
  type Screen,
  solveLayout,
} from './hudLayout';
import { HUD_LAYOUT } from './layout.gen';
import { minimapSize } from './minimap';

/** The screens the layout must hold on (owner, 2026-11-08), CSS px. */
const SCREENS: { name: string; w: number; h: number; touch: boolean }[] = [
  { name: 'iPhone portrait', w: 390, h: 844, touch: true },
  { name: 'iPhone landscape', w: 844, h: 390, touch: true },
  { name: 'Playwright iPhone 13 landscape', w: 750, h: 342, touch: true },
  { name: 'small Android', w: 360, h: 780, touch: true },
  { name: 'iPad portrait', w: 820, h: 1180, touch: true },
  { name: 'iPad landscape', w: 1180, h: 820, touch: true },
  { name: 'desktop 1280', w: 1280, h: 800, touch: false },
  { name: 'desktop 1920', w: 1920, h: 1080, touch: false },
  { name: 'narrow desktop window', w: 700, h: 600, touch: false },
];

const SAFE = { l: 10, t: 10, r: 10, b: 10 };

/** The parts' sizes as the stylesheet lays them out (measured in Chrome at 0.50.39, 19 hotbar items). */
function measured(s: { w: number; h: number; touch: boolean }): Map<string, Measured> {
  const m = new Map<string, Measured>();
  const landscape = s.w > s.h;
  m.set('crosshair', { rect: { x: s.w / 2 - 3, y: s.h / 2 - 3, w: 6, h: 6 } });
  const hw = s.touch ? 142 : 202;
  m.set('health', { rect: { x: s.w / 2 - hw / 2, y: s.h - 10 - 10, w: hw, h: 10 } });
  // Stamina and air (ADR 0069), just above it.
  m.set('vitals', { rect: { x: s.w / 2 - hw / 2, y: s.h - 10 - 14 - 8, w: hw, h: 8 } });
  m.set('menu-button', { w: 44, h: 44 });
  const map = minimapSize(s.w, s.h);
  m.set('minimap', { w: map, h: map });
  m.set('status-line', { w: 480, h: 22 });
  m.set('performance', { w: 365, h: 20 });
  m.set('flight-speed', { w: 250, h: 79 });
  m.set('loading-chip', { w: 220, h: 28 });
  if (s.touch) {
    m.set('info-button', { w: 44, h: 44 });
    m.set('touch-buttons', landscape ? { w: 116, h: 186 } : { w: 126, h: 280 });
    m.set('hotbar', { w: Math.min(19 * 44 + 8, s.w - 104), h: 66 });
    m.set('controls-hint', { w: Math.min(350, s.w / 2), h: 52 });
  } else {
    m.set('hotbar', { w: 19 * 44 + 8, h: 75 });
    m.set('controls-hint', { w: 276, h: 36 });
  }
  return m;
}

const screen = (s: { w: number; h: number; touch: boolean }): Screen => ({ ...s, safe: SAFE });

/** Every problem with a solved layout: a part with no room, off the safe area, or over another. */
function problems(layout: HudLayout, s: Screen, sizes: Map<string, Measured>): string[] {
  const solved = solveLayout(layout, s, sizes);
  const out: string[] = [];
  const placed: [string, Rect][] = [];
  for (const [id, r] of solved) {
    if (!r) {
      // The controls hint (the keys, for 9 s at the start; the lowest priority) may wait for room:
      // on a 342 px tall phone sideways, flying, there is none.
      if (id !== 'controls-hint') out.push(`${id}: no room`);
      continue;
    }
    const fixed = layout.parts.find((p) => p.id === id)?.fixed;
    if (
      !fixed &&
      (r.x < s.safe.l - 0.5 ||
        r.y < s.safe.t - 0.5 ||
        r.x + r.w > s.w - s.safe.r + 0.5 ||
        r.y + r.h > s.h - s.safe.b + 0.5)
    )
      out.push(`${id}: off screen ${JSON.stringify(r)}`);
    placed.push([id, r]);
  }
  for (let i = 0; i < placed.length; i++)
    for (let j = i + 1; j < placed.length; j++) {
      const [a, ra] = placed[i] as [string, Rect];
      const [b, rb] = placed[j] as [string, Rect];
      if (overlaps(ra, rb)) out.push(`${a} overlaps ${b}`);
    }
  for (const [id, r] of placed) {
    if (layout.parts.find((p) => p.id === id)?.fixed) continue;
    for (const c of clearRegions(layout, s))
      if (overlaps(r, c)) out.push(`${id} is over a kept-clear region`);
  }
  for (const id of sizes.keys()) if (!solved.has(id)) out.push(`${id}: not in the layout`);
  return out;
}

describe('the HUD layout (shared/ui/layout.json)', () => {
  for (const s of SCREENS) {
    it(`places every part on a ${s.name} (${String(s.w)}×${String(s.h)}) with none overlapping`, () => {
      expect(problems(HUD_LAYOUT, screen(s), measured(s))).toEqual([]);
    });
  }

  it('puts the minimap in its corner and the hotbar along the top up to it on a phone', () => {
    const s = screen({ w: 390, h: 844, touch: true });
    const r = solveLayout(HUD_LAYOUT, s, measured(s));
    const map = r.get('minimap') as Rect;
    const hotbar = r.get('hotbar') as Rect;
    const status = r.get('status-line') as Rect;
    const perf = r.get('performance') as Rect;
    const flight = r.get('flight-speed') as Rect;
    expect(map.x + map.w).toBe(390 - 10);
    expect(map.w).toBeLessThan(120); // the phone size
    expect(hotbar.y).toBe(10);
    expect(hotbar.x + hotbar.w).toBeLessThanOrEqual(map.x);
    // The lines below the hotbar on the left, clear of the minimap; the slider under the minimap.
    expect(status.x).toBe(10);
    expect(status.y).toBeGreaterThanOrEqual(hotbar.y + hotbar.h);
    expect(status.x + status.w).toBeLessThanOrEqual(map.x);
    expect(perf.y).toBeGreaterThan(status.y);
    expect(flight.y).toBeGreaterThanOrEqual(map.y + map.h);
  });

  it('keeps the touch buttons in the bottom right, clear of the held item, sideways', () => {
    const s = screen({ w: 844, h: 390, touch: true });
    const buttons = solveLayout(HUD_LAYOUT, s, measured(s)).get('touch-buttons') as Rect;
    expect(buttons.x + buttons.w).toBe(844 - 10);
    expect(buttons.y + buttons.h).toBe(390 - 10);
    expect(clearRegions(HUD_LAYOUT, s).length).toBe(1);
  });

  it('puts the desktop hotbar bottom centre, above the health bar', () => {
    const s = screen({ w: 1280, h: 800, touch: false });
    const sizes = measured(s);
    const r = solveLayout(HUD_LAYOUT, s, sizes);
    const hotbar = r.get('hotbar') as Rect;
    const health = r.get('health') as Rect;
    expect(hotbar.x + hotbar.w / 2).toBeCloseTo(640);
    expect(hotbar.y + hotbar.h).toBeLessThanOrEqual(health.y);
    // And above the stamina and air bars, which sit on the health bar (ADR 0069).
    const vitals = r.get('vitals') as Rect;
    expect(hotbar.y + hotbar.h).toBeLessThanOrEqual(vitals.y);
    expect(vitals.y + vitals.h).toBeLessThanOrEqual(health.y);
  });

  it('reflows when a part is hidden from Settings: no minimap, a wider hotbar', () => {
    const s = screen({ w: 390, h: 844, touch: true });
    const sizes = measured(s);
    const withMap = solveLayout(HUD_LAYOUT, s, sizes).get('hotbar') as Rect;
    sizes.delete('minimap');
    const without = solveLayout(HUD_LAYOUT, s, sizes);
    expect((without.get('hotbar') as Rect).w).toBeGreaterThan(withMap.w);
    // The slider moves up into the corner the minimap left.
    expect((without.get('flight-speed') as Rect).y).toBeLessThan(
      (solveLayout(HUD_LAYOUT, s, measured(s)).get('flight-speed') as Rect).y,
    );
    expect(problems(HUD_LAYOUT, s, sizes)).toEqual([]);
  });

  it('places a part added by data alone, clear of the rest', () => {
    const layout: HudLayout = {
      ...HUD_LAYOUT,
      parts: [
        ...HUD_LAYOUT.parts,
        { id: 'compass', selector: '#compass', anchor: 'top-right', priority: 2, after: 'minimap' },
      ],
    };
    for (const s of SCREENS) {
      const sizes = measured(s);
      sizes.set('compass', { w: 60, h: 24 });
      expect(problems(layout, screen(s), sizes), s.name).toEqual([]);
      const map = solveLayout(layout, screen(s), sizes);
      expect((map.get('compass') as Rect).y).toBeGreaterThan((map.get('minimap') as Rect).y);
    }
  });

  it('hides a part it finds no room for rather than overlapping', () => {
    const s = screen({ w: 300, h: 200, touch: true });
    const sizes = measured(s);
    sizes.set('flight-speed', { w: 280, h: 150 });
    const r = solveLayout(HUD_LAYOUT, s, sizes);
    expect(r.get('flight-speed')).toBeNull();
  });

  it("names Settings' parts by Settings' ids and selectors", () => {
    for (const p of HUD_LAYOUT.parts) {
      const e = HUD_ELEMENTS.find((h) => h.id === p.id);
      if (e) expect(p.selector).toBe(e.selector);
    }
  });
});
