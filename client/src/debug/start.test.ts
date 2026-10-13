import { describe, expect, it } from 'vitest';
import { hasDebugStart, parseDebugStart } from './start';

describe('debug start (?at, ?look, ?fly)', () => {
  it('reads a position, a view and flight', () => {
    const s = parseDebugStart('?local=1&at=15557.5,30,-12546&look=135,-12&fly=1');
    expect(s).toEqual({ at: [15557.5, 30, -12546], look: { yaw: 135, pitch: -12 }, fly: true });
    expect(hasDebugStart(s)).toBe(true);
  });

  it('ignores malformed values and asks for nothing without parameters', () => {
    expect(parseDebugStart('?at=1,2&look=x,3&fly=yes')).toEqual({
      at: null,
      look: null,
      fly: false,
    });
    expect(parseDebugStart('?at=1,2,3,4').at).toBeNull();
    expect(parseDebugStart('?at=1,,3').at).toBeNull();
    expect(hasDebugStart(parseDebugStart('?local=1'))).toBe(false);
  });
});
