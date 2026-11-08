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
  return typeof name === 'string' && list.backgrounds.some((b) => b.name === name)
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

/** Whether the player asked for reduced motion (the OS setting). */
function reducedMotion(): boolean {
  return typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;
}

/** The panorama behind the main menu: `root` goes under its content. */
export class MenuPanorama {
  readonly root = document.createElement('div');
  private readonly strip = document.createElement('div');
  private shown: string | null = null;

  constructor(name: string) {
    this.root.className = 'mm-panorama';
    this.root.setAttribute('aria-hidden', 'true');
    this.strip.className = 'mm-pano-strip';
    this.root.append(this.strip);
    this.show(name);
    if (typeof matchMedia === 'function') {
      matchMedia('(prefers-reduced-motion: reduce)').addEventListener('change', () => {
        const name = this.shown;
        this.shown = null;
        if (name) this.show(name);
      });
    }
  }

  /** Shows the background named (the default for a name this build doesn't know). */
  show(name: string): void {
    const bg = findBackground(name);
    if (bg.name === this.shown) return;
    this.shown = bg.name;
    this.root.dataset.background = bg.name;
    const img = new Image();
    img.onload = () => {
      if (this.shown !== bg.name) return;
      const style = panoramaStyle(img.naturalWidth / img.naturalHeight, reducedMotion());
      // From script: the page's CSP takes no inline style sheet (element styles are allowed).
      this.strip.style.backgroundImage = `url("${bg.url}")`;
      this.strip.style.aspectRatio = style.aspectRatio;
      this.strip.style.animationDuration = style.animationDuration;
      this.strip.style.animationName = style.animationName;
      this.root.classList.add('loaded');
    };
    img.src = bg.url;
  }
}
