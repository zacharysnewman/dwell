// Theater mode's HUD switch (docs/THEATER_MODE.md §2): one switch the whole HUD obeys. Every HUD
// element is a child of `body` (each listed in HUD_ELEMENTS, which hud.test.ts audits against the
// sources), so the switch hides every child of `body` but the game canvas and theater's own bar,
// and turns off what the game draws over the scene (the arms and the held item). While a take
// records or a path plays, the bar hides too.

/** The elements the clean frame keeps: the game canvas, and theater's bar (except in a take). */
export const HUD_KEEP = ['view', 'theater-bar', 'theater-touch'] as const;

/**
 * Every HUD part the shell puts on the page, by what makes it (a class built on `document.body`,
 * or a module appending to it): hud.test.ts fails when a source adds one not listed here, so a new
 * HUD part is hidden by the switch by being a child of `body`, and listed.
 */
export const HUD_ELEMENTS = [
  'Hud',
  'MapOverlay',
  'FlightSpeedControl',
  'TouchControls',
  'SettingsMenu',
  'Hotbar',
  'CreativeInventory',
  'Backpack',
  'LoadingScreen',
  'MainMenu',
  'minimap',
  'upgradePrompt',
  'registryPrompt',
  'hudLayout probe',
  'TheaterMode #theater-open',
  'main (the blueprint have/need panel)',
  'index.html #net-status',
  'index.html #perf-status',
  'index.html #build-info',
  'index.html #fatal',
] as const;

export const CLEAN_CLASS = 'theater-clean';
export const TAKE_CLASS = 'theater-take';

/** The switch's rule: every child of `body` but the kept ones hidden; in a take, the bar too. */
/**
 * The switch's rules, in src/style.css (the page's CSP allows no inline style, so they can't be
 * injected; hud.test.ts checks style.css has them): every child of `body` but the kept ones hidden
 * (opacity too: a part that sets its own children `visibility: visible` would show through a
 * hidden parent); in a take, the bar too.
 */
export const HUD_RULES = [
  'body.theater-clean > *:not(#view):not(#theater-bar):not(#theater-touch):not(script):not(style)',
  'body.theater-take > #theater-bar',
  'body.theater-take > #theater-touch',
] as const;

/** Whether a child of `body` with this id shows in the clean frame (`take`: while recording). */
export function shownWhenClean(id: string, take: boolean): boolean {
  if (id === 'view') return true;
  if (id === 'theater-bar' || id === 'theater-touch') return !take;
  return false;
}

/** The parts of the doc the switch touches. */
export type HudDoc = Pick<Document, 'body'>;

/** What the switch turns off in the game's own drawing. */
export interface HudClient {
  /** Shows the first-person arms as the player's setting has them (true), or hides them. */
  arms(shown: boolean): void;
}

/**
 * The HUD switch: `shown` false hides every HUD part (the DOM's and the game's arms); `take`
 * hides theater's bar as well.
 */
export function setHud(doc: HudDoc, client: HudClient | null, shown: boolean, take = false): void {
  doc.body.classList.toggle(CLEAN_CLASS, !shown);
  doc.body.classList.toggle(TAKE_CLASS, !shown && take);
  client?.arms(shown);
}
