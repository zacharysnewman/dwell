// The stamina and air bars (ADR 0069): shown only while not full, hidden where the mode has none.
import { describe, expect, it } from 'vitest';
import { HUD_ELEMENTS } from './hud.gen';
import { HUD_LAYOUT } from './layout.gen';
import { vitalsBar } from './hud';
import { VITALS } from './vitals.gen';

describe('vitals bars', () => {
  it('fade out when full and show a share otherwise', () => {
    expect(vitalsBar(1)).toEqual({ width: '100%', shown: false });
    expect(vitalsBar(0.24)).toEqual({ width: '24%', shown: true });
    expect(vitalsBar(0)).toEqual({ width: '0%', shown: true });
    expect(vitalsBar(1.4).shown).toBe(false);
  });

  it('are hidden in a mode without the rule', () => {
    expect(vitalsBar(null).shown).toBe(false);
  });

  it('are a part Settings can hide and the layout keeps clear', () => {
    expect(HUD_ELEMENTS.some((e) => e.id === 'vitals' && e.selector === '#vitals')).toBe(true);
    expect(HUD_LAYOUT.parts.some((p) => p.id === 'vitals' && p.fixed)).toBe(true);
    expect(VITALS.recoverFraction).toBe(0.25);
  });
});
