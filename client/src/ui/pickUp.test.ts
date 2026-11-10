import { describe, expect, it } from 'vitest';
import { pickUpButton, pickUpPrompt } from './hud';

// Picking up a ground item (ADR 0071): E with a keyboard, a tappable button on touch (owner,
// 2026-11-09: "How do I pickup a rock on mobile?").
describe('the pick-up prompt', () => {
  it('names the key with a keyboard', () => {
    expect(pickUpPrompt('Rock', false)).toBe('E  Pick up Rock');
  });

  it('is a button to tap on touch, not a key to press', () => {
    expect(pickUpPrompt('Rock', true)).toBeNull();
    expect(pickUpButton('Rock')).toBe('Pick up Rock');
  });

  it('shows nothing when the crosshair is on no ground item', () => {
    expect(pickUpPrompt(null, false)).toBeNull();
    expect(pickUpButton(null)).toBeNull();
  });
});
