import { describe, expect, it } from 'vitest';
import {
  clickSound,
  hoverSound,
  installUiSounds,
  SLIDER_INTERVAL_MS,
  type UiElement,
} from './uiSounds';
import type { UiKind } from './types';

/** A minimal element (the suite runs without a DOM). */
function el(
  tagName: string,
  attrs: Record<string, string> = {},
  parent: UiElement | null = null,
  extra: Partial<UiElement> = {},
): UiElement {
  return {
    tagName: tagName.toUpperCase(),
    parentElement: parent,
    getAttribute: (n) => attrs[n] ?? null,
    textContent: attrs.text ?? null,
    ...extra,
  };
}

/** An event whose target is a fake element. */
function event(type: string, target: UiElement, init: Record<string, unknown> = {}): Event {
  const e = new Event(type);
  Object.defineProperty(e, 'target', { value: target });
  for (const [k, v] of Object.entries(init)) Object.defineProperty(e, k, { value: v });
  return e;
}

describe('UI sounds', () => {
  it('give any button the same click, wherever the press lands inside it', () => {
    const b = el('button', { text: 'Create and play' });
    expect(clickSound(b)).toBe('click');
    expect(clickSound(el('span', {}, b))).toBe('click');
    expect(clickSound(el('a', { href: '#', text: 'About' }))).toBe('click');
    expect(clickSound(el('div'))).toBeNull();
    expect(clickSound(el('button', {}, null, { disabled: true }))).toBeNull();
  });

  it('know tabs, toggles, back buttons, checkboxes and overrides', () => {
    expect(clickSound(el('button', { role: 'tab' }))).toBe('tab');
    expect(clickSound(el('button', { 'aria-pressed': 'true' }))).toBe('toggle_on');
    expect(clickSound(el('button', { 'aria-pressed': 'false' }))).toBe('toggle_off');
    expect(clickSound(el('button', { 'aria-label': 'Close menu' }))).toBe('back');
    expect(clickSound(el('button', { text: 'Cancel' }))).toBe('back');
    expect(clickSound(el('input', {}, null, { type: 'checkbox', checked: true }))).toBe(
      'toggle_on',
    );
    expect(clickSound(el('input', {}, null, { type: 'checkbox', checked: false }))).toBe(
      'toggle_off',
    );
    expect(clickSound(el('input', {}, null, { type: 'text' }))).toBeNull();
    expect(clickSound(el('input', {}, null, { type: 'range' }))).toBeNull();
    expect(clickSound(el('button', { 'data-sound': 'none' }))).toBeNull();
    expect(clickSound(el('button', { 'data-sound': 'success' }))).toBe('success');
    // An ancestor's override covers its buttons (a panel of controls with their own sounds).
    expect(clickSound(el('button', {}, el('div', { 'data-sound': 'none' })))).toBeNull();
    expect(hoverSound(el('button', { 'data-sound': 'none' }))).toBe(false);
    expect(hoverSound(el('button'))).toBe(true);
  });

  it('sound a button made after they were installed, without any wiring', () => {
    const root = new EventTarget();
    const played: UiKind[] = [];
    let t = 0;
    const remove = installUiSounds(
      root,
      (k) => played.push(k),
      () => t,
    );
    // Buttons created later (a new menu page, a world's row): delegation hears them.
    const later = el('button', { text: 'Play' });
    root.dispatchEvent(event('click', later));
    root.dispatchEvent(event('click', el('button', { role: 'tab' })));
    expect(played).toEqual(['click', 'tab']);
    // Hover once per element, for a mouse only.
    root.dispatchEvent(event('pointerover', later, { pointerType: 'mouse' }));
    root.dispatchEvent(event('pointerover', el('span', {}, later), { pointerType: 'mouse' }));
    root.dispatchEvent(event('pointerover', el('button'), { pointerType: 'touch' }));
    expect(played).toEqual(['click', 'tab', 'hover']);
    // Slider ticks are throttled.
    const slider = el('input', {}, null, { type: 'range' });
    for (let i = 0; i < 10; i++) {
      t += SLIDER_INTERVAL_MS / 4;
      root.dispatchEvent(event('input', slider));
    }
    expect(played.filter((k) => k === 'slider').length).toBe(3);
    // Keyboard focus sounds; the focus that follows a press does not.
    played.length = 0;
    t += 10_000;
    root.dispatchEvent(event('focusin', el('button')));
    root.dispatchEvent(event('pointerdown', el('button')));
    root.dispatchEvent(event('focusin', el('button')));
    expect(played).toEqual(['hover']);
    remove();
    root.dispatchEvent(event('click', later));
    expect(played).toEqual(['hover']);
  });
});
