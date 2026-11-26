// @vitest-environment happy-dom
import { describe, expect, it } from 'vitest';
import {
  BRUSH_CM,
  CharacterPainter,
  LOOK_KEY,
  brushAfter,
  nextRamp,
  slotOfKey,
  type PainterClient,
} from './characterPainter';

/** A fake client recording the painter's calls. */
function fake(): PainterClient & { calls: string[]; slots: string[] } {
  const slots = ['skin_fair', 'hair_brown', 'heartwood', 'wool_green', 'flint', 'wool_red', 'brass'];
  const calls: string[] = [];
  return {
    calls,
    slots,
    openPainter: () => (calls.push('open'), true),
    closePainter: (keep) => (calls.push(`close ${String(keep)}`), true),
    paint: (op, slot, r) => (calls.push(`${op} ${String(slot)} ${String(r)}`), true),
    paintSlot: (k, ramp) => {
      calls.push(`slot ${String(k)} ${ramp}`);
      slots[k] = ramp;
      return true;
    },
    painterSlots: () => JSON.stringify(slots),
    ownLook: () => '{"slots":[],"faces":""}',
    theaterFly: () => calls.push('fly'),
  };
}

describe('the Character painter (owner, 2026-11-26)', () => {
  it('keys pick slots and size the brush within its range', () => {
    expect(slotOfKey('Digit1')).toBe(0);
    expect(slotOfKey('Digit7')).toBe(6);
    expect(slotOfKey('Digit8')).toBeNull();
    expect(brushAfter(BRUSH_CM.max, 3)).toBe(BRUSH_CM.max);
    expect(brushAfter(BRUSH_CM.min, -3)).toBe(BRUSH_CM.min);
    expect(brushAfter(100, 1)).toBeGreaterThan(100);
    expect(nextRamp(['a', 'b', 'c'], 'c')).toBe('a');
    expect(nextRamp(['a', 'b', 'c'], 'a', -1)).toBe('c');
  });

  it('paints with the chosen slot, fills on right click, undoes, and saves the look on Esc', () => {
    const client = fake();
    const store = new Map<string, string>();
    let locks = 0;
    const p = new CharacterPainter(
      document,
      client,
      [['wool_blue', '#4166ad'], ['skin_deep', '#6b4334']],
      { setItem: (k, v) => store.set(k, v) },
      { lock: () => (locks += 1), unlock: () => undefined, closed: () => undefined },
    );
    expect(p.enter()).toBe(true);
    expect(locks).toBe(1);
    expect(document.querySelectorAll('.cp-slot')).toHaveLength(7);
    p.key('Digit3', true, false, false);
    p.mouseDown(0);
    p.mouseUp(0);
    p.mouseDown(2);
    p.key('KeyZ', true, false, true);
    expect(client.calls).toEqual([
      'open',
      `brush 2 ${String(BRUSH_CM.start)}`,
      `end 2 ${String(BRUSH_CM.start)}`,
      `fill 2 ${String(BRUSH_CM.start)}`,
      `undo 2 ${String(BRUSH_CM.start)}`,
    ]);
    // C opens the palette: a swatch sets the slot's colour.
    p.key('KeyC', true, false, false);
    expect(p.choosing).toBe(true);
    (document.querySelector('.cp-swatch') as HTMLButtonElement).click();
    expect(client.slots[2]).toBe('wool_blue');
    expect(p.choosing).toBe(false);
    p.key('Escape', true, false, false);
    expect(p.active).toBe(false);
    expect(client.calls.at(-1)).toBe('close true');
    expect(store.get(LOOK_KEY)).toBe('{"slots":[],"faces":""}');
  });
});
