// What each key and touch control does, for the game menu's Controls and Debug pages, the debug
// overlay's key hints and the hint shown when a game starts. The bindings themselves are the Rust
// client's (dwell-game input.rs, touch.rs) and the shell's (main.ts: Esc, − and = while flying);
// this table describes them, one entry per action, so a new binding is a new entry here.

export interface Control {
  /** What it does, as the player would say it. */
  action: string;
  /** The alternatives ("C" or "Ctrl"); within one, `+` separates keys used together ("W+A+S+D",
   * "Space+Space" for a double tap). Touch controls are short phrases. */
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
      {
        action: 'Fly on or off',
        keys: ['Space+Space'],
        when: 'double-tap, where the world allows',
      },
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
    title: 'Game',
    controls: [{ action: 'Menu and settings', keys: ['Esc'] }],
  },
];

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
export const START_HINT = 'Esc menu · F3 debug · F4 map · double-tap Space to fly';

/** The same on a touch screen. */
export const TOUCH_START_HINT = '☰ settings and controls · ⓘ debug';
