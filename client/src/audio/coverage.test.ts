// Every action a player can take makes a sound (docs/AUDIO.md §2 `controls`): each entry of the
// controls table (ui/controls.ts, the bindings' one description) needs a mapping in the sound data,
// so a new binding fails here until its sound is decided.
import { describe, expect, it } from 'vitest';
import { DEBUG_KEYS, KEYBOARD, TOUCH } from '../ui/controls';
import { SOUND_DATA } from './sounds.gen';

const actions = [
  ...KEYBOARD.flatMap((g) => g.controls),
  ...DEBUG_KEYS,
  ...TOUCH,
].map((c) => c.action);

describe('sound coverage', () => {
  it('gives every action of the controls table a sound, or a reason to be silent', () => {
    const missing = actions.filter((a) => !(a in SOUND_DATA.controls));
    expect(missing, 'add them to "controls" in shared/sounds/*.json').toEqual([]);
  });

  it('maps no action the controls table no longer has', () => {
    const stale = Object.keys(SOUND_DATA.controls).filter((a) => !actions.includes(a));
    expect(stale).toEqual([]);
  });

  it('only leaves continuous actions silent', () => {
    const silent = Object.entries(SOUND_DATA.controls)
      .filter(([, v]) => !Array.isArray(v))
      .map(([k]) => k);
    expect(silent).toEqual(['Look']);
  });

  it('names sounds that exist', () => {
    for (const [action, refs] of Object.entries(SOUND_DATA.controls)) {
      if (!Array.isArray(refs)) continue;
      for (const r of refs) {
        const [kind, name = ''] = r.split(':');
        const known =
          (kind === 'event' && SOUND_DATA.events.some((e) => e.name === name)) ||
          (kind === 'action' && name in SOUND_DATA.actions) ||
          (kind === 'ui' && name in SOUND_DATA.ui);
        expect(known, `${action}: ${r}`).toBe(true);
      }
    }
  });
});
