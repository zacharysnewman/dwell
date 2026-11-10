// The main menu's background (owner, 2026-11-08: "a 360° slowly rotating background image of a real
// spot in a world"): a panorama rendered from the game (shared/ui/menu-backgrounds.json, `npm run
// panoramas`), drawn behind the menu as a CSS strip that turns once in `turnS` seconds — no game
// engine, so it is cheap on phones — and held still for players who prefer reduced motion. Which
// one is a setting (Settings → On screen → Menu background), kept in `dwell.menu` (a record,
// append-only: fields a newer build stores are kept on rewrite).
import {
  DEFAULT_MENU_BACKGROUND,
  MENU_BACKGROUND_TURN_S,
  MENU_BACKGROUNDS,
  type MenuBackground,
} from './menuBackgrounds.gen';

export { DEFAULT_MENU_BACKGROUND, MENU_BACKGROUNDS };

/**
 * The choice that picks a background at random and fades to another after each full turn (owner,
 * 2026-11-08: "a default random setting that picks one at random and after doing a full 360 they
 * fade into the next one").
 */
export const RANDOM_BACKGROUND = 'random';

/** A different background from `current` (any one when there is only one), by `random()` in [0, 1). */
export function nextRandomBackground(
  current: string | null,
  random: () => number = Math.random,
  list: BackgroundList = BACKGROUNDS,
): string {
  const names = list.backgrounds.map((b) => b.name);
  const others = names.length > 1 ? names.filter((n) => n !== current) : names;
  const name = others[Math.min(others.length - 1, Math.floor(random() * others.length))];
  if (name === undefined) throw new Error('no menu backgrounds');
  return name;
}

/** The backgrounds and the default (the generated data; tests pass their own). */
export interface BackgroundList {
  backgrounds: readonly MenuBackground[];
  default: string;
}

export const BACKGROUNDS: BackgroundList = {
  backgrounds: MENU_BACKGROUNDS,
  default: DEFAULT_MENU_BACKGROUND,
};

/** The background named, or the default one when the data doesn't list it (a newer build's). */
export function findBackground(name: string, list: BackgroundList = BACKGROUNDS): MenuBackground {
  const found = list.backgrounds.find((b) => b.name === name);
  if (found) return found;
  const fallback = list.backgrounds.find((b) => b.name === list.default) ?? list.backgrounds[0];
  if (!fallback) throw new Error('no menu backgrounds');
  return fallback;
}

/** The stored choice (`dwell.menu`'s `background`), or the default when missing or unknown. */
export function sanitizeMenuBackground(
  stored: unknown,
  list: BackgroundList = BACKGROUNDS,
): string {
  const name =
    typeof stored === 'object' && stored !== null && !Array.isArray(stored)
      ? (stored as Record<string, unknown>).background
      : undefined;
  return typeof name === 'string' &&
    (name === RANDOM_BACKGROUND || list.backgrounds.some((b) => b.name === name))
    ? name
    : list.default;
}

/** The record to store: the stored one's other fields kept (append-only), `background` written. */
export function menuRecord(stored: unknown, background: string): Record<string, unknown> {
  const kept =
    typeof stored === 'object' && stored !== null && !Array.isArray(stored)
      ? (stored as Record<string, unknown>)
      : {};
  return { ...kept, background };
}

/**
 * The turning strip's style: two copies of the image side by side (`aspect` its width over its
 * height), moved left by one copy each turn, so the panorama's ends meet and it never jumps. With
 * reduced motion it holds still.
 */
export function panoramaStyle(
  aspect: number,
  reducedMotion: boolean,
  turnS: number = MENU_BACKGROUND_TURN_S,
): { aspectRatio: string; animationDuration: string; animationName: string } {
  return {
    aspectRatio: String(2 * aspect),
    animationDuration: `${String(turnS)}s`,
    animationName: reducedMotion ? 'none' : 'mm-pano-turn',
  };
}

/**
 * The screen's shorter side (CSS px) below which the menu loads a background's small copy: a phone.
 * The full strip (shots of 2160 px, ~6800 × 1950) decodes to ~53 MB, two of them while one fades
 * into the next; the small one is a quarter of that, and a phone's screen is small enough not to miss it.
 */
export const SMALL_SCREEN_PX = 600;

/** The image to load for a screen of `width` × `height` CSS px: the small copy on a phone. */
export function backgroundUrl(bg: MenuBackground, width: number, height: number): string {
  return Math.min(width, height) < SMALL_SCREEN_PX ? bg.smallUrl : bg.url;
}

/** The screen's size (CSS px), for `backgroundUrl`; a desktop's when there is no screen. */
function screenSize(): [number, number] {
  return typeof screen === 'object' && screen.width > 0
    ? [screen.width, screen.height]
    : [1920, 1080];
}

/** Whether the player asked for reduced motion (the OS setting). */
function reducedMotion(): boolean {
  return typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;
}

/** The panorama behind the main menu: `root` goes under its content. */
export class MenuPanorama {
  readonly root = document.createElement('div');
  /** Two layers, so a new view fades in over the old one. */
  private readonly layers: [HTMLDivElement, HTMLDivElement];
  private front: 0 | 1 = 0;
  private shown: string | null = null;
  private choice: string;
  private timer: ReturnType<typeof setTimeout> | null = null;

  constructor(name: string) {
    this.root.className = 'mm-panorama';
    this.root.setAttribute('aria-hidden', 'true');
    const layer = () => {
      const l = document.createElement('div');
      l.className = 'mm-pano-layer';
      const strip = document.createElement('div');
      strip.className = 'mm-pano-strip';
      l.append(strip);
      this.root.append(l);
      return l;
    };
    this.layers = [layer(), layer()];
    this.choice = name;
    this.show(name);
    if (typeof matchMedia === 'function') {
      matchMedia('(prefers-reduced-motion: reduce)').addEventListener('change', () => {
        const shown = this.shown;
        this.shown = null;
        if (shown) this.display(shown);
      });
    }
  }

  /**
   * Shows the choice: a background by name (the default for a name this build doesn't know), or
   * `random`: one picked at random, then another after each full turn, faded in over it.
   */
  show(name: string): void {
    this.choice = name;
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
    if (name === RANDOM_BACKGROUND) {
      if (this.shown === null) this.display(nextRandomBackground(null));
      else this.schedule();
      return;
    }
    this.display(findBackground(name).name);
  }

  /** In `random`, the next view after one full turn. */
  private schedule(): void {
    if (this.choice !== RANDOM_BACKGROUND || this.timer !== null) return;
    this.timer = setTimeout(() => {
      this.timer = null;
      if (this.choice !== RANDOM_BACKGROUND) return;
      this.display(nextRandomBackground(this.shown));
    }, MENU_BACKGROUND_TURN_S * 1000);
  }

  private display(name: string): void {
    const bg = findBackground(name);
    if (bg.name === this.shown) {
      this.schedule();
      return;
    }
    this.shown = bg.name;
    this.root.dataset.background = bg.name;
    const back = this.front === 0 ? 1 : 0;
    const [layer, old] = back === 0 ? this.layers : [this.layers[1], this.layers[0]];
    const strip = layer.firstElementChild as HTMLDivElement;
    const url = backgroundUrl(bg, ...screenSize());
    const img = new Image();
    img.onload = () => {
      if (this.shown !== bg.name) return;
      const style = panoramaStyle(img.naturalWidth / img.naturalHeight, reducedMotion());
      // From script: the page's CSP takes no inline style sheet (element styles are allowed).
      strip.style.backgroundImage = `url("${url}")`;
      strip.style.aspectRatio = style.aspectRatio;
      strip.style.animationDuration = style.animationDuration;
      strip.style.animationName = 'none';
      strip.getBoundingClientRect(); // a reflow: the turn restarts from its start
      strip.style.animationName = style.animationName;
      layer.classList.add('shown');
      old.classList.remove('shown');
      this.front = back;
      this.root.classList.add('loaded');
      this.schedule();
    };
    img.src = url;
  }
}
