// Menu sounds (docs/AUDIO.md §6), wired once by delegated listeners on the document, so every
// button, tab, link and slider of every menu sounds the same without per-button wiring, and a new
// button sounds as soon as it exists. The vocabulary is fixed (hover, click, back, tab, toggle on
// and off, slider, error, success) and its sounds never vary. An element (or an ancestor) with
// `data-sound="<kind>"` plays that kind instead; `data-sound="none"` is silent (controls whose
// action has its own sound: the hotbar, the touch buttons, the game menu's open and close).
import type { UiKind } from './types';

/** The parts of a DOM element the classification reads (a fake in tests). */
export interface UiElement {
  tagName: string;
  parentElement: UiElement | null;
  getAttribute(name: string): string | null;
  textContent: string | null;
  /** Inputs. */
  type?: string;
  checked?: boolean;
  disabled?: boolean;
}

const KINDS: readonly string[] = [
  'hover',
  'click',
  'back',
  'tab',
  'toggle_on',
  'toggle_off',
  'slider',
  'error',
  'success',
];

/** Slider ticks at most this often (ms). */
export const SLIDER_INTERVAL_MS = 60;
/** A focus this soon after a pointer press is the press's, not keyboard navigation (ms). */
const FOCUS_AFTER_POINTER_MS = 600;

function up(el: UiElement | null, test: (e: UiElement) => boolean): UiElement | null {
  for (let e = el; e; e = e.parentElement) if (test(e)) return e;
  return null;
}

/** The interactive element an event's target belongs to. */
export function interactive(el: UiElement | null): UiElement | null {
  return up(el, (e) => {
    const tag = e.tagName.toLowerCase();
    const role = e.getAttribute('role');
    return (
      tag === 'button' ||
      tag === 'select' ||
      tag === 'summary' ||
      (tag === 'a' && e.getAttribute('href') !== null) ||
      (tag === 'input' && e.type !== 'hidden') ||
      role === 'button' ||
      role === 'tab'
    );
  });
}

/** `data-sound` on the element or an ancestor: a kind, 'none', or null when not set. */
function override(el: UiElement): string | null {
  const marked = up(el, (e) => e.getAttribute('data-sound') !== null);
  return marked?.getAttribute('data-sound') ?? null;
}

const BACK = /^(close|back|cancel|resume|quit|done)\b/i;

/** The sound a click on `target` makes, or null. */
export function clickSound(target: UiElement | null): UiKind | null {
  const el = interactive(target);
  if (!el || el.disabled) return null;
  const o = override(el);
  if (o !== null) return KINDS.includes(o) ? (o as UiKind) : null;
  const tag = el.tagName.toLowerCase();
  if (tag === 'input') {
    if (el.type === 'checkbox' || el.type === 'radio') return el.checked ? 'toggle_on' : 'toggle_off';
    if (el.type === 'button' || el.type === 'submit' || el.type === 'reset') return 'click';
    return null; // text fields, sliders (their ticks are `input` events)
  }
  if (tag === 'select') return null; // opening a select: its change sounds
  if (el.getAttribute('role') === 'tab') return 'tab';
  const pressed = el.getAttribute('aria-pressed');
  if (pressed === 'true') return 'toggle_on';
  if (pressed === 'false') return 'toggle_off';
  const label = el.getAttribute('aria-label') ?? el.textContent ?? '';
  if (BACK.test(label.trim())) return 'back';
  return 'click';
}

/** Whether pointing at or focusing `target` makes the hover sound. */
export function hoverSound(target: UiElement | null): boolean {
  const el = interactive(target);
  if (!el || el.disabled) return false;
  const o = override(el);
  if (o === 'none') return false;
  return !(el.tagName.toLowerCase() === 'input' && el.type === 'text');
}

export interface UiSoundRoot {
  addEventListener(type: string, listener: (e: Event) => void, options?: AddEventListenerOptions): void;
  removeEventListener(type: string, listener: (e: Event) => void, options?: AddEventListenerOptions): void;
}

/**
 * Installs the delegated listeners on `root` (the document); returns a function that removes
 * them. `play` gets each kind; `now` is a clock in ms.
 */
export function installUiSounds(
  root: UiSoundRoot,
  play: (kind: UiKind) => void,
  now: () => number = () => performance.now(),
): () => void {
  let hovered: UiElement | null = null;
  let lastPointer = -Infinity;
  let lastSlider = -Infinity;
  const target = (e: Event): UiElement | null => e.target as unknown as UiElement | null;
  const listeners: [string, (e: Event) => void][] = [
    [
      'click',
      (e) => {
        const kind = clickSound(target(e));
        if (kind) play(kind);
      },
    ],
    [
      'pointerover',
      (e) => {
        // A finger has no hover: a tap would sound twice.
        if ((e as PointerEvent).pointerType !== 'mouse') return;
        const el = interactive(target(e));
        if (el === hovered) return;
        hovered = el;
        if (el && hoverSound(el)) play('hover');
      },
    ],
    [
      'pointerdown',
      () => {
        lastPointer = now();
      },
    ],
    [
      'focusin',
      (e) => {
        // Keyboard navigation (Tab): a focus not caused by a press.
        if (now() - lastPointer < FOCUS_AFTER_POINTER_MS) return;
        if (hoverSound(target(e))) play('hover');
      },
    ],
    [
      'input',
      (e) => {
        const el = target(e);
        if (!el || el.tagName.toLowerCase() !== 'input' || el.type !== 'range') return;
        if (override(el) === 'none') return;
        const t = now();
        if (t - lastSlider < SLIDER_INTERVAL_MS) return;
        lastSlider = t;
        play('slider');
      },
    ],
    [
      'change',
      (e) => {
        const el = target(e);
        if (el?.tagName.toLowerCase() === 'select' && override(el) !== 'none') play('click');
      },
    ],
  ];
  // Clicks bubble, so a toggle's `aria-pressed` is read after its own handler set it; the rest
  // capture (a handler may stop them).
  const capture = (type: string): boolean => type !== 'click';
  for (const [type, l] of listeners)
    root.addEventListener(type, l, { capture: capture(type), passive: true });
  return () => {
    for (const [type, l] of listeners) root.removeEventListener(type, l, { capture: capture(type) });
  };
}
