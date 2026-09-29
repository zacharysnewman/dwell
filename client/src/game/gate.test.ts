import { describe, expect, it } from 'vitest';
import { PlayerState } from '../protocol/constants.gen';
import { predictionMayRun } from './gate';

describe('prediction gate', () => {
  it('waits for the terrain around a walking player', () => {
    expect(predictionMayRun(true, PlayerState.Walking, false)).toBe(false);
    expect(predictionMayRun(true, PlayerState.Walking, true)).toBe(true);
    expect(predictionMayRun(false, PlayerState.Idle, true)).toBe(false);
  });

  it('keeps predicting a flying player that has outrun the streamed terrain', () => {
    expect(predictionMayRun(true, PlayerState.Flying, false)).toBe(true);
  });
});
