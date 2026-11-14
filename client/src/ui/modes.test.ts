// The New world form's game modes (ADR 0065): the generated table's, creative the default,
// adventure marked experimental.
import { describe, expect, it } from 'vitest';
import { isGameMode, modeLabel } from './modes';
import { DEFAULT_MODE, GAME_MODES } from './modes.gen';

describe('game modes', () => {
  it('offers creative (the default) and adventure (experimental)', () => {
    expect(DEFAULT_MODE).toBe('creative');
    expect(GAME_MODES.map((m) => modeLabel(m))).toEqual(['Creative', 'Adventure (experimental)']);
    const adventure = GAME_MODES.find((m) => m.name === 'adventure');
    expect(adventure?.abilities).toEqual(['damage', 'map', 'stamina', 'oxygen', 'blueprints']);
  });

  it('knows its modes by name only', () => {
    expect(isGameMode('adventure')).toBe(true);
    expect(isGameMode('Adventure')).toBe(false);
    expect(isGameMode('')).toBe(false);
    expect(isGameMode(undefined)).toBe(false);
  });
});
