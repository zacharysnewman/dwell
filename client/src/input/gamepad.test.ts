import { describe, expect, it } from 'vitest';
import { GAMEPAD, PLAY_ACTIONS } from './gamepad.gen';
import { menuHeld, nearestToward, Repeat, type Rect } from './gamepadMenu';
import { pickGamepad } from './gamepad';
import {
  buttonAt,
  defaultGamepad,
  gamepadRecord,
  rebind,
  sanitizeGamepad,
} from './gamepadSettings';

const rect = (left: number, top: number): Rect => ({ left, top, width: 10, height: 10 });

describe('controller settings', () => {
  it('start on the data’s buttons, every action on its own', () => {
    const d = defaultGamepad();
    expect(d.bindings).toEqual(GAMEPAD.play);
    expect(new Set(Object.values(d.bindings)).size).toBe(PLAY_ACTIONS.length);
  });

  it('rebinding swaps with the action that had the button', () => {
    const d = defaultGamepad();
    const s = rebind(d, 'jump', d.bindings.fly);
    expect(s.bindings.jump).toBe(d.bindings.fly);
    expect(s.bindings.fly).toBe(d.bindings.jump);
  });

  it('stored settings are put in range; unknown entries and clashes resolved', () => {
    const s = sanitizeGamepad({
      bindings: { jump: 'x', spyglass: 'x', fly: 'nope', dance: 'a' },
      lookSpeed: 99,
      invertY: true,
    });
    expect(s.bindings.jump).toBe('x');
    // spyglass's stored x is taken by jump: it keeps its default (x too), so a free button.
    expect(s.bindings.spyglass).not.toBe('x');
    expect(s.bindings.fly).toBe(GAMEPAD.play.fly);
    expect(new Set(Object.values(s.bindings)).size).toBe(PLAY_ACTIONS.length);
    expect(s.lookSpeed).toBe(3);
    expect(s.invertY).toBe(true);
    expect(sanitizeGamepad('junk')).toEqual(defaultGamepad());
  });

  it('the stored record keeps fields a newer build wrote (append-only)', () => {
    const stored = { future: 1, bindings: { laterAction: 'rs' } };
    const r = gamepadRecord(stored, defaultGamepad());
    expect(r.future).toBe(1);
    expect((r.bindings as Record<string, string>).laterAction).toBe('rs');
    expect((r.bindings as Record<string, string>).jump).toBe(GAMEPAD.play.jump);
  });

  it('names buttons by their standard-mapping index', () => {
    for (const b of GAMEPAD.buttons) expect(buttonAt(b.index)).toBe(b.name);
    expect(buttonAt(99)).toBeUndefined();
  });
});

describe('menus by controller', () => {
  it('moves the focus to the nearest control that way, straight ahead first', () => {
    const from = rect(100, 100);
    const rects = [rect(100, 160), rect(160, 120), rect(40, 100), rect(100, 40)];
    expect(nearestToward(from, rects, 'down')).toBe(0);
    expect(nearestToward(from, rects, 'right')).toBe(1);
    expect(nearestToward(from, rects, 'left')).toBe(2);
    expect(nearestToward(from, rects, 'up')).toBe(3);
    expect(nearestToward(from, [rect(100, 40)], 'down')).toBe(-1);
  });

  it('a held direction repeats after a delay', () => {
    const r = new Repeat(300, 100);
    expect(r.step(true, 0)).toBe(true);
    expect(r.step(true, 200)).toBe(false);
    expect(r.step(true, 310)).toBe(true);
    expect(r.step(true, 350)).toBe(false);
    expect(r.step(true, 420)).toBe(true);
    expect(r.step(false, 430)).toBe(false);
    expect(r.step(true, 440)).toBe(true);
  });

  it('reads the D-pad, the left stick and the face buttons', () => {
    const buttons = Array<number>(17).fill(0);
    const index = (n: string) => GAMEPAD.buttons.find((b) => b.name === n)?.index ?? -1;
    buttons[index(GAMEPAD.menu.confirm)] = 1;
    const held = menuHeld(buttons, [0, 0.9, 0, 0]);
    expect(held.confirm && held.down).toBe(true);
    expect(held.up || held.back).toBe(false);
  });

  it('reads the first connected pad, a standard-mapped one first', () => {
    const pad = (connected: boolean, mapping: string) =>
      ({ connected, mapping, buttons: [], axes: [] }) as unknown as Gamepad;
    const odd = pad(true, '');
    const std = pad(true, 'standard');
    expect(pickGamepad([null, odd, std])).toBe(std);
    expect(pickGamepad([pad(false, 'standard'), odd])).toBe(odd);
    expect(pickGamepad([null])).toBeNull();
  });
});
