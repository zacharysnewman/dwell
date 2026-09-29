import { describe, expect, it } from 'vitest';
import { InputButtons } from '../protocol/constants.gen';
import { DOUBLE_TAP_MS, FlightToggle } from './flight';
import { IDLE_INPUT, quantizeInput } from './input';

describe('creative flight toggle (PLAYER_CONTROLLER.md §6.7)', () => {
  it('a quick double jump toggles flight; slow presses do not', () => {
    const f = new FlightToggle();
    const changes: boolean[] = [];
    f.onChange = (on) => changes.push(on);
    f.jumpPressed(1000);
    f.jumpPressed(1000 + DOUBLE_TAP_MS + 1);
    expect(f.flying).toBe(false);
    f.jumpPressed(1400);
    expect(f.flying).toBe(true);
    // The pair is used up: a third press starts a new pair.
    f.jumpPressed(1450);
    expect(f.flying).toBe(true);
    f.jumpPressed(1500);
    expect(f.flying).toBe(false);
    expect(changes).toEqual([true, false]);
  });

  it('a server that disallows flight turns it off and keeps it off', () => {
    const f = new FlightToggle();
    f.toggle();
    expect(f.flying).toBe(true);
    f.allowed = false;
    expect(f.flying).toBe(false);
    f.toggle();
    f.jumpPressed(0);
    f.jumpPressed(10);
    expect(f.flying).toBe(false);
  });

  it('flying sets the fly bit of every input frame', () => {
    expect(quantizeInput({ ...IDLE_INPUT, fly: true }, 1).buttons).toBe(InputButtons.fly);
    expect(quantizeInput(IDLE_INPUT, 1).buttons).toBe(0);
  });
});
