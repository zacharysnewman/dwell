import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import data from '../../../shared/ui/menu-backgrounds.json';
import {
  BACKGROUNDS,
  DEFAULT_MENU_BACKGROUND,
  findBackground,
  MENU_BACKGROUNDS,
  menuRecord,
  panoramaStyle,
  sanitizeMenuBackground,
} from './menuBackground';
import { loadSettingsState, menuBackgroundChoice, saveSettingsState } from './settingsMenu';
import { MENU_KEY } from './settingsPins';
import { WORLD_PAGES } from './menuPage';

describe('menu backgrounds', () => {
  it('are the data: every entry, by name and label, the default the beach', () => {
    expect(MENU_BACKGROUNDS.map((b) => b.name)).toEqual(data.backgrounds.map((b) => b.name));
    expect(MENU_BACKGROUNDS.map((b) => b.label)).toEqual(data.backgrounds.map((b) => b.label));
    expect(DEFAULT_MENU_BACKGROUND).toBe('beach-peaks');
    for (const b of MENU_BACKGROUNDS) expect(b.url).toMatch(/\.webp/);
  });

  it('a name this build does not know shows the default', () => {
    expect(findBackground('lake-shore').name).toBe('lake-shore');
    expect(findBackground('from-a-newer-build').name).toBe(DEFAULT_MENU_BACKGROUND);
  });

  it('a fixture entry added by data alone is offered in Settings', () => {
    const list = {
      backgrounds: [
        ...BACKGROUNDS.backgrounds,
        { name: 'sky-island', label: 'Sky island', url: 'x.webp' },
      ],
      default: BACKGROUNDS.default,
    };
    const choice = menuBackgroundChoice(list);
    expect(choice.choices).toContain('sky-island');
    expect(choice.labels['sky-island']).toBe('Sky island');
    expect(sanitizeMenuBackground({ background: 'sky-island' }, list)).toBe('sky-island');
    expect(findBackground('sky-island', list).url).toBe('x.webp');
  });

  it('turn once in turnS seconds, and hold still with reduced motion', () => {
    expect(panoramaStyle(3.5, false, 150)).toEqual({
      aspectRatio: '7',
      animationDuration: '150s',
      animationName: 'mm-pano-turn',
    });
    expect(panoramaStyle(3.5, true, 150).animationName).toBe('none');
    // About one turn in 2–3 minutes (owner, 2026-11-08).
    expect(data.turnS).toBeGreaterThanOrEqual(120);
    expect(data.turnS).toBeLessThanOrEqual(180);
  });
});

describe('the menu background setting', () => {
  const store = new Map<string, string>();
  beforeEach(() => {
    store.clear();
    vi.stubGlobal('localStorage', {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => store.set(k, v),
      removeItem: (k: string) => store.delete(k),
    });
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('defaults to the beach, and is not stored until chosen', () => {
    const state = loadSettingsState(false);
    expect(state.settings.menuBackground).toBe('beach-peaks');
    saveSettingsState(state);
    expect(store.has(MENU_KEY)).toBe(false);
  });

  it('sanitizes: missing, unknown or malformed reads as the default', () => {
    expect(sanitizeMenuBackground(null)).toBe(DEFAULT_MENU_BACKGROUND);
    expect(sanitizeMenuBackground({ background: 'nowhere' })).toBe(DEFAULT_MENU_BACKGROUND);
    expect(sanitizeMenuBackground({ background: 3 })).toBe(DEFAULT_MENU_BACKGROUND);
    expect(sanitizeMenuBackground(['lake-shore'])).toBe(DEFAULT_MENU_BACKGROUND);
    expect(sanitizeMenuBackground({ background: 'river-bank' })).toBe('river-bank');
  });

  it('is kept append-only: a newer build’s fields in the record survive a rewrite', () => {
    expect(menuRecord({ background: 'lake-shore', music: 'calm' }, 'river-bank')).toEqual({
      background: 'river-bank',
      music: 'calm',
    });
    store.set(MENU_KEY, JSON.stringify({ background: 'lake-shore', future: 1 }));
    const state = loadSettingsState(false);
    expect(state.settings.menuBackground).toBe('lake-shore');
    expect(state.pins.has('menuBackground')).toBe(true);
    saveSettingsState({
      ...state,
      settings: { ...state.settings, menuBackground: 'river-bank' },
    });
    expect(JSON.parse(store.get(MENU_KEY) ?? 'null')).toEqual({
      background: 'river-bank',
      future: 1,
    });
  });
});

describe('Settings from the main menu', () => {
  it('lists the pages that need a world with a note each', () => {
    expect(WORLD_PAGES.map(([page]) => page)).toEqual(['structures', 'debug', 'host']);
    for (const [, note] of WORLD_PAGES) expect(note).toMatch(/^Open (a|one of your) world/);
  });
});
