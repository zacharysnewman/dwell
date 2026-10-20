// What each key and touch control does, for the game menu's Controls and Debug pages, the debug
// overlay's key hints and the hint shown when a game starts. The bindings themselves are the Rust
// client's (dwell-game input.rs, touch.rs) and the shell's (main.ts: Esc, − and = while flying);
// this table describes them, one entry per action, so a new binding is a new entry here.

import { BRUSH_PRESETS } from './brushes.gen';

export interface Control {
  /** What it does, as the player would say it. */
  action: string;
  /** The alternatives ("C" or "Ctrl"); within one, `+` separates keys used together ("W+A+S+D").
   * Touch controls are short phrases. */
  keys: string[];
  /** When it applies, if not always ("while flying"). */
  when?: string;
}

export interface ControlGroup {
  title: string;
  controls: Control[];
}

export const KEYBOARD: ControlGroup[] = [
  {
    title: 'Move',
    controls: [
      { action: 'Walk', keys: ['W+A+S+D'] },
      { action: 'Look', keys: ['Mouse'], when: 'click the view to capture the mouse' },
      { action: 'Jump', keys: ['Space'] },
      { action: 'Run', keys: ['Shift'] },
      { action: 'Crouch', keys: ['C', 'Ctrl'] },
      { action: 'Fly on or off', keys: ['F'], when: 'where the world allows' },
      { action: 'Fly up', keys: ['Space'], when: 'while flying' },
      { action: 'Fly down', keys: ['C'], when: 'while flying' },
      { action: 'Slower / faster flight', keys: ['−', '='], when: 'while flying' },
    ],
  },
  {
    title: 'Build',
    controls: [
      { action: 'Break block', keys: ['Left click'] },
      { action: 'Place block', keys: ['Right click'] },
      { action: 'Choose block', keys: ['1 – 9', 'Wheel'] },
    ],
  },
  {
    // The region tools (dwell-game region_tools.rs `tool_for_key`), while the mouse is captured.
    title: 'Region tools',
    controls: [
      { action: 'Select first corner', keys: ['B'], when: 'the targeted block' },
      { action: 'Select second corner', keys: ['N'], when: 'the targeted block' },
      { action: 'Clear the selection', keys: ['X'] },
      { action: 'Fill with the chosen block', keys: ['G'], when: 'up to 32 a side' },
      { action: 'Hollow box of the chosen block', keys: ['H'] },
      { action: 'Replace the targeted kind of block', keys: ['T'], when: 'with the chosen block' },
      { action: 'Remove the blocks', keys: ['Delete', 'Backspace'] },
      { action: 'Copy', keys: ['Ctrl+C', '⌘+C'] },
      { action: 'Paste mode on or off', keys: ['Ctrl+V', '⌘+V'] },
      { action: 'Paste', keys: ['Right click'], when: 'in paste mode; left click leaves it' },
      { action: 'Rotate / mirror the paste', keys: ['R', 'M'], when: 'in paste mode' },
      { action: 'Undo', keys: ['Ctrl+Z', '⌘+Z'] },
      { action: 'Redo', keys: ['Ctrl+Shift+Z', 'Ctrl+Y', '⌘+Shift+Z'] },
    ],
  },
  {
    // The brush tool (dwell-game brush_tool.rs `brush_key`; BRUSHES.md), in sandbox worlds.
    title: 'Brushes',
    controls: [
      { action: 'Brush tool on or off', keys: ['V'], when: 'in sandbox worlds' },
      { action: 'Paint at the targeted block', keys: ['Left click'], when: 'up to 128 m away' },
      { action: 'Paint in front of it', keys: ['Right click'] },
      { action: 'Pick the block to replace', keys: ['Middle click', '⌥+Click', 'Alt+Click'] },
      { action: 'Previous / next preset', keys: ['Q', 'E'] },
      { action: 'Next / previous brush shape', keys: ['Z', 'Shift+Z'] },
      { action: 'Next / previous performer', keys: ['P', 'Shift+P'] },
      { action: 'Smaller / bigger brush', keys: ['[', ']', 'Alt+Wheel'] },
      { action: 'Exact-cell preview on or off', keys: ['\\'], when: 'off, the brush’s box shows' },
      {
        action: 'Pick a preset by its key',
        keys: BRUSH_PRESETS.flatMap((p) => (p.key ? [`${keyLabel(p.key)} ${p.label}`] : [])),
        when: 'the keys are the presets’ (shared/brushes/brushes.json)',
      },
      { action: 'Undo / redo a stroke', keys: ['Ctrl+Z', '⌘+Z'], when: 'as the region tools' },
    ],
  },
  {
    title: 'Game',
    controls: [{ action: 'Menu and settings', keys: ['Esc'] }],
  },
];

/** A `KeyboardEvent.code` as the key's label (`KeyU` → U, `Semicolon` → ;). */
export function keyLabel(code: string): string {
  const named: Record<string, string> = {
    Semicolon: ';',
    Quote: "'",
    Comma: ',',
    Period: '.',
    Slash: '/',
    BracketLeft: '[',
    BracketRight: ']',
  };
  return named[code] ?? code.replace(/^Key|^Digit/, '');
}

/** Debug tools: also buttons in the game menu's Debug page. */
export const DEBUG_KEYS: Control[] = [
  { action: 'Debug overlay', keys: ['F3'] },
  { action: 'Terrain map', keys: ['F4'] },
  { action: 'Map zoom', keys: ['−', '='], when: 'while the map is open' },
  { action: 'Copy block info', keys: ['F6'], when: 'the targeted block, as JSON' },
  {
    action: 'Compare view: split, blink, difference',
    keys: ['F7'],
    when: 'in compare mode (?renderer=compare, local builds)',
  },
  { action: 'Blink: swap the sides', keys: ['F8'], when: 'in compare mode' },
  { action: 'Move the split', keys: ['Drag the divider'], when: 'in compare mode, split view' },
];

export const TOUCH: Control[] = [
  { action: 'Walk', keys: ['Drag on the left'] },
  { action: 'Look', keys: ['Drag on the right'] },
  { action: 'Break or place', keys: ['Tap the view'], when: 'the Break/Place button switches' },
  { action: 'Jump, Run, Crouch, Fly', keys: ['Buttons, bottom right'] },
  { action: 'Choose block', keys: ['Tap the hotbar'] },
  { action: 'Debug overlay', keys: ['ⓘ, top right'] },
];

/** A control's keys as one line: alternatives join with " / ". */
export function keysLabel(c: Control): string {
  return c.keys.join(' / ');
}

/** The short line shown when a game starts on a keyboard. */
export const START_HINT = 'Esc menu · F3 debug · F4 map · F to fly';

/** The same on a touch screen. */
export const TOUCH_START_HINT = '☰ settings and controls · ⓘ debug';
