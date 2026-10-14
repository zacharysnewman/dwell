import { describe, expect, it } from 'vitest';
import { slotForKey, slotKey } from './hotbar';

describe('hotbar keys', () => {
  it('number keys 1–9 pick the first nine slots and 0 the tenth', () => {
    expect(slotForKey('Digit1')).toBe(0);
    expect(slotForKey('Digit9')).toBe(8);
    expect(slotForKey('Digit0')).toBe(9);
    expect(slotForKey('KeyA')).toBeNull();
    expect([0, 8, 9, 10].map(slotKey)).toEqual(['1', '9', '0', '']);
  });
});
