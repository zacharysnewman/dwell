import { describe, expect, it } from 'vitest';
import {
  afterIntro,
  back,
  firstView,
  INTRO_SEEN_KEY,
  INTRO_VIEWS,
  introSeen,
  isIntro,
  markIntroSeen,
} from './menuNav';
import { newWorldRoute } from './mainMenu';
import { DEFAULT_MODE } from './modes.gen';

/** A sessionStorage stand-in. */
function store(): Pick<Storage, 'getItem' | 'setItem'> {
  const m = new Map<string, string>();
  return {
    getItem: (k) => m.get(k) ?? null,
    setItem: (k, v) => {
      m.set(k, v);
    },
  };
}

describe('the main menu navigation (menu redesign M1)', () => {
  it('opens on the logo screen, then a key or a tap goes to the home', () => {
    expect(INTRO_VIEWS[0]).toBe('logo');
    expect(firstView(false)).toBe('logo');
    expect(isIntro('logo')).toBe(true);
    expect(afterIntro('logo')).toBe('home');
    expect(isIntro('home')).toBe(false);
  });

  it('goes back from each page to where it was opened, and stays on the home', () => {
    expect(back('worlds')).toBe('home');
    expect(back('new-world')).toBe('worlds');
    expect(back('online')).toBe('home');
    expect(back('versions')).toBe('home');
    expect(back('home')).toBe('home');
    // logo → home → Worlds → New world → Back → Back
    let view = afterIntro(firstView(false));
    view = 'worlds';
    view = 'new-world';
    view = back(view);
    expect(view).toBe('worlds');
    expect(back(view)).toBe('home');
  });

  it('skips the logo screen on later visits in the same session', () => {
    const s = store();
    expect(introSeen(s)).toBe(false);
    expect(firstView(introSeen(s))).toBe('logo');
    markIntroSeen(s);
    expect(s.getItem(INTRO_SEEN_KEY)).toBe('1');
    expect(firstView(introSeen(s))).toBe('home');
  });

  it('opens on the home when it has a message to show', () => {
    expect(firstView(false, true)).toBe('home');
  });

  it('survives blocked storage: the logo shows, nothing throws', () => {
    const blocked = {
      getItem: () => {
        throw new Error('blocked');
      },
      setItem: () => {
        throw new Error('blocked');
      },
    };
    expect(introSeen(blocked)).toBe(false);
    expect(introSeen(null)).toBe(false);
    expect(() => {
      markIntroSeen(blocked);
    }).not.toThrow();
  });
});

describe('the New world form', () => {
  it('keeps its defaults: an untouched form makes "New world", a random seed, the default mode', () => {
    expect(
      newWorldRoute({ name: '', seed: '', type: 'terrain', mode: DEFAULT_MODE, spawn: '' }),
    ).toEqual({
      new: '1',
      name: 'New world',
      seed: '',
      type: 'terrain',
    });
  });

  it('passes what the player chose', () => {
    expect(
      newWorldRoute({ name: ' Home ', seed: ' 42 ', type: 'flat', mode: 'other-mode', spawn: 'b' }),
    ).toEqual({ new: '1', name: 'Home', seed: '42', type: 'flat', spawn: 'b', mode: 'other-mode' });
  });
});
