import { describe, expect, it } from 'vitest';
import { BRUSH_PRESETS } from './brushes.gen';
import { DEBUG_KEYS, KEYBOARD, START_HINT, TOUCH, keyLabel, keysLabel } from './controls';

describe('controls', () => {
  it('names every debug key the client handles (dwell-game input.rs toggles)', () => {
    const keys = DEBUG_KEYS.flatMap((c) => c.keys);
    for (const k of ['F3', 'F4', 'F6']) expect(keys).toContain(k);
    for (const k of ['F3', 'F4']) expect(START_HINT).toContain(k);
  });

  it("names compare mode's keys (web/app.rs handles them in compare mode)", () => {
    const keys = DEBUG_KEYS.flatMap((c) => c.keys);
    for (const k of ['F7', 'F8']) expect(keys).toContain(k);
  });

  it('lists each action once, with keys', () => {
    const all = [...KEYBOARD.flatMap((g) => g.controls), ...DEBUG_KEYS, ...TOUCH];
    for (const c of all) expect(c.keys.length).toBeGreaterThan(0);
    const keyboard = KEYBOARD.flatMap((g) => g.controls.map((c) => c.action));
    expect(new Set(keyboard).size).toBe(keyboard.length);
  });

  // Owner, 2026-10-15: flight has its own key (dwell-game input.rs FLY_KEY), not a double-tapped Space.
  it('flies with F, not a double-tapped Space', () => {
    const fly = KEYBOARD.flatMap((g) => g.controls).find((c) => c.action === 'Fly on or off');
    expect(fly?.keys).toEqual(['F']);
    expect(fly?.when ?? '').not.toMatch(/double/i);
    expect(START_HINT).toBe('Esc menu · F3 debug · F4 map · F to fly');
    const all = [...KEYBOARD.flatMap((g) => g.controls), ...DEBUG_KEYS, ...TOUCH];
    for (const c of all) {
      expect(c.keys.join(' ')).not.toContain('Space+Space');
      expect(`${c.action} ${c.when ?? ''}`).not.toMatch(/double/i);
    }
  });

  it('lists the region tools (dwell-game region_tools.rs tool_for_key)', () => {
    const tools = KEYBOARD.find((g) => g.title === 'Region tools')?.controls ?? [];
    const keys = tools.flatMap((c) => c.keys);
    for (const k of [
      'B',
      'N',
      'X',
      'G',
      'H',
      'T',
      'Delete',
      'R',
      'M',
      'Ctrl+C',
      'Ctrl+V',
      'Ctrl+Z',
    ])
      expect(keys).toContain(k);
  });

  it('lists the brush tool and every preset key from the data (brush_tool.rs brush_key)', () => {
    const brushes = KEYBOARD.find((g) => g.title === 'Brushes')?.controls ?? [];
    const keys = brushes.flatMap((c) => c.keys);
    for (const k of ['V', 'Q', 'E', 'Z', 'P', '[', ']', 'Middle click']) expect(keys).toContain(k);
    for (const p of BRUSH_PRESETS.filter((q) => q.key !== undefined)) {
      expect(keys).toContain(`${keyLabel(p.key ?? '')} ${p.label}`);
    }
    expect(keyLabel('KeyU')).toBe('U');
    expect(keyLabel('Semicolon')).toBe(';');
    // No brush key is a region-tools or movement key.
    const others = KEYBOARD.filter((g) => g.title !== 'Brushes').flatMap((g) =>
      g.controls.flatMap((c) => c.keys),
    );
    for (const k of ['V', 'Q', 'E', 'Z', 'P', '[', ']'])
      expect(others.filter((o) => o === k)).toEqual([]);
  });

  it('joins alternatives', () => {
    expect(keysLabel({ action: 'Crouch', keys: ['C', 'Ctrl'] })).toBe('C / Ctrl');
  });
});
