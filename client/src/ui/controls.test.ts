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

  it('joins alternatives', () => {
    expect(keysLabel({ action: 'Crouch', keys: ['C', 'Ctrl'] })).toBe('C / Ctrl');
  });
});
