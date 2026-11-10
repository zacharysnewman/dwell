// The main menu's navigation (owner, 2026-11-09, menu redesign M1): a logo screen first, waiting
// for a key or a tap (the gesture that also unlocks audio), then the home — Worlds, Play with
// others, Settings, Versions — and each action's page over the same panorama, with Back. The logo
// screen shows once a session: a later visit in the same tab (back from a world) opens on the home.
// Pure state, no DOM, so the flow is tested (menuNav.test.ts); ui/mainMenu.ts draws it.

/** The menu's views: the intro screens, the home, and the pages the home opens. */
export type MenuView = 'logo' | 'home' | 'worlds' | 'new-world' | 'online' | 'versions';

/**
 * The intro screens in order, before the home. A studio intro (Dropkick) goes before `logo` when
 * it exists: add its view here and draw it in ui/mainMenu.ts.
 */
export const INTRO_VIEWS: readonly MenuView[] = ['logo'];

/** Each page's parent, where Back goes; the home and the intro screens have none. */
const PARENT: Partial<Record<MenuView, MenuView>> = {
  worlds: 'home',
  'new-world': 'worlds',
  online: 'home',
  versions: 'home',
};

/** The session key (sessionStorage: this tab only) that says the intro was seen. */
export const INTRO_SEEN_KEY = 'dwell.menuIntroSeen';

/**
 * Where the menu opens: the first intro screen, or the home once this session has seen it — or
 * when the menu opens with a message (why it came up instead of a world), which the home shows.
 */
export function firstView(introSeen: boolean, message = false): MenuView {
  return introSeen || message ? 'home' : (INTRO_VIEWS[0] ?? 'home');
}

/** After an intro screen (a key or a tap): the next one, then the home. */
export function afterIntro(view: MenuView): MenuView {
  const i = INTRO_VIEWS.indexOf(view);
  if (i < 0) return view;
  return INTRO_VIEWS[i + 1] ?? 'home';
}

/** Whether a view is an intro screen. */
export function isIntro(view: MenuView): boolean {
  return INTRO_VIEWS.includes(view);
}

/** Back (the Back button, Esc, a controller's B): the page's parent; the home stays put. */
export function back(view: MenuView): MenuView {
  return PARENT[view] ?? view;
}

/** Whether this session has seen the intro (false where sessionStorage is blocked). */
export function introSeen(store: Pick<Storage, 'getItem'> | null): boolean {
  try {
    return store?.getItem(INTRO_SEEN_KEY) === '1';
  } catch {
    return false;
  }
}

/** Notes that this session has seen the intro. */
export function markIntroSeen(store: Pick<Storage, 'setItem'> | null): void {
  try {
    store?.setItem(INTRO_SEEN_KEY, '1');
  } catch {
    // blocked storage: the intro shows again on the next visit
  }
}
