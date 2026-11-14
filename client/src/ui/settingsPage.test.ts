// The Settings page (menu M3): every stored setting still has a row, each row is wired to its
// setting, and Back, Esc and a controller's B leave the page for the home.
import { describe, expect, it } from 'vitest';
import { back } from './menuNav';
import {
  baseSettings,
  SETTING_FIELDS,
  SETTINGS_PANES,
  settingsPageModel,
  type Settings,
} from './settingsMenu';

describe('the Settings page structure (menu M3)', () => {
  for (const mobile of [false, true]) {
    const device = mobile ? 'a phone' : 'a desktop';
    const model = settingsPageModel(mobile);

    it(`lists the panes in order with their titles on ${device}`, () => {
      expect(model.map((p) => p.pane)).toEqual([...SETTINGS_PANES]);
      expect(model.map((p) => p.title)).toEqual(['Graphics', 'Sound', 'Gameplay', 'On screen']);
      for (const p of model) expect(p.rows.length).toBeGreaterThan(0);
    });

    it(`draws a row for every stored setting on ${device}`, () => {
      const pins = new Set(model.flatMap((p) => p.rows.map((r) => r.pin)));
      // The interface parts' toggles are rows too (On screen, shared/ui/hud.json). Not drawn, on
      // purpose: the model detail (no Settings control since 0.52.5, shared/models/models.json)
      // and, on a phone, the menu button (a phone may not hide it: hud.json's `phone: false`).
      const unshown = mobile ? ['hud.shown.menu-button', 'modelDetail'] : ['modelDetail'];
      const missing = SETTING_FIELDS.map((f) => f.id).filter((id) => !pins.has(id));
      expect(missing).toEqual(unshown);
      // ...and no row changes a setting that isn't stored.
      const ids = new Set(SETTING_FIELDS.map((f) => f.id));
      for (const pin of pins) expect(ids.has(pin), pin).toBe(true);
    });

    it(`wires each row to its setting on ${device}: set a value, read it back`, () => {
      const base: Settings = baseSettings(mobile);
      for (const row of model.flatMap((p) => p.rows)) {
        for (const v of row.values) {
          const next = row.with(base, v as never);
          const got = row.get(next);
          if (typeof v === 'number') expect(got as number, row.label).toBeCloseTo(v, 6);
          else expect(got, row.label).toBe(v);
        }
      }
    });
  }
});

describe('the Settings page navigation (menu M3)', () => {
  it('goes Back (the button, Esc, a controller B) to the home', () => {
    expect(back('settings')).toBe('home');
  });
});
