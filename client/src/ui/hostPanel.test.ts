import { describe, expect, it } from 'vitest';
import { guestsText, qrModules } from './hostPanel';

describe('host panel', () => {
  it('counts guests against the limit', () => {
    expect(guestsText(0, 8)).toBe('No guests yet (up to 8)');
    expect(guestsText(1, 8)).toBe('1 of 8 guests playing');
    expect(guestsText(3, 4)).toBe('3 of 4 guests playing');
  });

  it('draws a square QR code with finder patterns for the invite link', () => {
    const modules = qrModules('https://dropkickarcade.com/dwell/?code=KQ7XM4');
    const n = modules.length;
    expect(n).toBeGreaterThanOrEqual(21);
    expect(modules.every((row) => row.length === n)).toBe(true);
    // Each corner finder's outer ring is dark (top-left, top-right, bottom-left).
    expect(modules[0]?.slice(0, 7).every(Boolean)).toBe(true);
    expect(modules[0]?.slice(n - 7).every(Boolean)).toBe(true);
    expect(modules[n - 1]?.slice(0, 7).every(Boolean)).toBe(true);
  });
});
