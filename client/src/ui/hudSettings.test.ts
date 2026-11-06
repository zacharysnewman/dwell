import { describe, expect, it } from 'vitest';
import { HUD_ELEMENTS, type HudElement } from './hud.gen';
import {
  applyHud,
  defaultHud,
  hideableParts,
  hudRecord,
  partShown,
  sanitizeHud,
} from './hudSettings';

const fixture: HudElement = {
  id: 'compass',
  label: 'Compass',
  hint: 'North',
  selector: '#compass',
  shown: true,
  phone: true,
};

describe('HUD settings', () => {
  it('shows every part by default, as the data says', () => {
    for (const e of HUD_ELEMENTS) expect(defaultHud().shown[e.id]).toBe(e.shown);
  });

  it('keeps flags it reads and defaults the rest', () => {
    const s = sanitizeHud({ shown: { 'status-line': false, hotbar: 'no' } });
    expect(s.shown['status-line']).toBe(false);
    expect(s.shown.hotbar).toBe(true);
    expect(sanitizeHud(null)).toEqual(defaultHud());
  });

  it("keeps a newer build's parts and fields when it rewrites the record", () => {
    const stored = { shown: { minimap: false, hotbar: true }, layout: 'compact' };
    const s = sanitizeHud(stored);
    s.shown.hotbar = false;
    expect(hudRecord(stored, s)).toEqual({
      layout: 'compact',
      shown: { ...s.shown, minimap: false, hotbar: false },
    });
  });

  it('never hides the menu button on a phone, and does not offer it there', () => {
    const menu = HUD_ELEMENTS.find((e) => e.id === 'menu-button');
    expect(menu).toBeDefined();
    const hidden = { shown: { 'menu-button': false } };
    if (!menu) return;
    expect(partShown(menu, hidden, true)).toBe(true);
    expect(partShown(menu, hidden, false)).toBe(false);
    expect(hideableParts(true).map((e) => e.id)).not.toContain('menu-button');
    expect(hideableParts(false).map((e) => e.id)).toContain('menu-button');
  });

  it('hides a part added by data alone', () => {
    // A stand-in for the page: its body's classes.
    const classes = new Set<string>();
    const doc = {
      body: {
        classList: {
          toggle: (c: string, on: boolean) => (on ? classes.add(c) : classes.delete(c)),
        },
      },
    } as unknown as Document;
    const parts = [...HUD_ELEMENTS, fixture];
    applyHud(doc, { shown: { compass: false } }, false, parts);
    expect(classes.has('hud-hide-compass')).toBe(true);
    expect(classes.has('hud-hide-hotbar')).toBe(false);
    applyHud(doc, { shown: { compass: true } }, false, parts);
    expect(classes.has('hud-hide-compass')).toBe(false);
  });
});
