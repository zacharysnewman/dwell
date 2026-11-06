import { describe, expect, it } from 'vitest';
import { upgradeText } from './upgradePrompt';

describe('upgrade prompt (owner, 2026-11-06)', () => {
  it('names both versions and lines and says what changes', () => {
    const t = upgradeText('Hills', '0.47.3', '0.48.2');
    const all = t.points.join(' ');
    expect(all).toContain('Dwell 0.47.3 (line 0.47.x)');
    expect(all).toContain('Dwell 0.48.2 (line 0.48.x)');
    expect(all).toMatch(/terrain/);
    expect(all).toMatch(/placeholders/);
    expect(all).toContain('Dwell 0.47.3 can’t open it again');
    expect([t.copy, t.upgrade, t.cancel]).toEqual(['Play a copy', 'Upgrade this world', 'Cancel']);
  });
});
