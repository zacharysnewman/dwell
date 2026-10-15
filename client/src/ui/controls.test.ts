import { describe, expect, it } from 'vitest';
import { DEBUG_KEYS, KEYBOARD, START_HINT, TOUCH, keysLabel } from './controls';

describe('controls', () => {
  it('names every debug key the client handles (dwell-game input.rs toggles)', () => {
    const keys = DEBUG_KEYS.flatMap((c) => c.keys);
    for (const k of ['F3', 'F4', 'F6']) expect(keys).toContain(k);
    for (const k of ['F3', 'F4']) expect(START_HINT).toContain(k);
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

  it('joins alternatives', () => {
    expect(keysLabel({ action: 'Crouch', keys: ['C', 'Ctrl'] })).toBe('C / Ctrl');
  });
});
