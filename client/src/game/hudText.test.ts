import { describe, expect, it } from 'vitest';
import { hudText } from './hudText';

const playing = { dead: false, active: true, terrainReady: true, terrainError: null };

describe('HUD text', () => {
  it('shows terrain loading as a corner status, never over the view', () => {
    // Regression (phone playtest): flying fast kept "Loading terrain…" in the middle of the view.
    expect(hudText({ ...playing, terrainReady: false })).toEqual({
      center: '',
      status: 'Loading terrain…',
    });
    expect(hudText({ ...playing, terrainError: 'worker failed' })).toEqual({
      center: '',
      status: 'Terrain unavailable: worker failed',
    });
  });

  it('keeps the centre for what stops play: joining and death', () => {
    expect(hudText({ ...playing, active: false, terrainReady: false }).center).toBe('Joining…');
    expect(hudText({ ...playing, dead: true }).center).toBe('You died — respawning…');
    expect(hudText(playing)).toEqual({ center: '', status: '' });
  });
});
