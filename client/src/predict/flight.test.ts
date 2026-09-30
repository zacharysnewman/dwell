import { describe, expect, it } from 'vitest';
import { InputButtons, Players } from '../protocol/constants.gen';
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

  it('the speed level rides in the flySpeed bits, capped at the highest level', () => {
    const level = (flySpeed: number) =>
      (quantizeInput({ ...IDLE_INPUT, fly: true, flySpeed }, 1).buttons & InputButtons.flySpeed) >>
      Players.flySpeedShift;
    expect(level(0)).toBe(0);
    expect(level(17)).toBe(17);
    expect(level(99)).toBe(Players.flySpeedMaxLevel);
    expect(level(-4)).toBe(0);
    expect(
      quantizeInput({ ...IDLE_INPUT, fly: true, flySpeed: 3 }, 1).buttons & InputButtons.fly,
    ).toBe(InputButtons.fly);
  });

  it('keeps the speed level in range and reports changes once', () => {
    const f = new FlightToggle();
    const seen: number[] = [];
    f.onSpeedChange = (l) => seen.push(l);
    f.speedLevel = 5;
    f.speedLevel = 5;
    f.speedLevel = 1000;
    f.speedLevel = -1;
    expect(seen).toEqual([5, Players.flySpeedMaxLevel, 0]);
    expect(f.speedLevel).toBe(0);
  });
});
