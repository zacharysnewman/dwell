import { describe, expect, it } from 'vitest';
import { DEBUG_KEYS } from '../ui/controls';
import { pauseFromQuery, togglePause, type Pausable } from './pause';

describe('the debug pause', () => {
  it('starts paused with ?pause=1 only', () => {
    expect(pauseFromQuery('?pause=1')).toBe(true);
    expect(pauseFromQuery('?local=1&pause=true')).toBe(true);
    expect(pauseFromQuery('?pause=on')).toBe(true);
    expect(pauseFromQuery('?pause=0')).toBe(false);
    expect(pauseFromQuery('')).toBe(false);
    expect(pauseFromQuery('?local=1')).toBe(false);
  });

  it('toggles the client and reports the new state', () => {
    let paused = false;
    const client: Pausable = {
      setLookPaused: (on) => (paused = on),
      lookPaused: () => paused,
    };
    expect(togglePause(client)).toBe(true);
    expect(paused).toBe(true);
    expect(togglePause(client)).toBe(false);
    expect(paused).toBe(false);
  });

  it('is a debug tool on F7', () => {
    expect(DEBUG_KEYS.find((d) => d.keys.includes('F7'))?.action).toBe('Pause the look');
  });
});
