// The loading screen on joining a world and on respawning (ARCHITECTURE.md §6.3): the Rust client
// decides when it shows and what it reads (`Client.hud().loading`: the near ring's chunks drawn out
// of those needed, or null once open, from shared/look/loading.json's radius and timeout); this
// draws it over the view. Opted out for testing with `?loading=off` or the `dwell.loading` store set
// to "off" (the e2e suite's storage does), it never shows.
import type { KeyValueStore } from '../local/worldIndex';

/** The dev store that turns the screen off ("off"); anything else, or nothing, leaves it on. */
export const LOADING_KEY = 'dwell.loading';

/** `Client.hud().loading`: null once open. */
export interface LoadingHud {
  text: string;
  ready: number;
  needed: number;
}

/** Whether the loading screen is on for this page: off for `?loading=off` or the store's "off". */
export function loadingScreenEnabled(search: string, store: KeyValueStore | null): boolean {
  if (new URLSearchParams(search).get('loading') === 'off') return false;
  let stored: string | null;
  try {
    stored = store?.getItem(LOADING_KEY) ?? null;
  } catch {
    stored = null; // storage blocked: the default
  }
  if (stored === null) return true;
  try {
    return JSON.parse(stored) !== 'off';
  } catch {
    return stored !== 'off';
  }
}

/** The bar's fill, 0–1 (an empty ring is full). */
export function loadingFraction(h: LoadingHud): number {
  return h.needed > 0 ? Math.min(1, Math.max(0, h.ready / h.needed)) : 1;
}

export class LoadingScreen {
  private readonly root: HTMLDivElement;
  private readonly text: HTMLDivElement;
  private readonly fill: HTMLDivElement;
  private shown: string | null = null;

  constructor(parent: HTMLElement) {
    this.root = document.createElement('div');
    this.root.id = 'loading-screen';
    this.root.hidden = true;
    this.root.setAttribute('role', 'status');
    this.root.setAttribute('aria-live', 'polite');
    const sheet = document.createElement('div');
    sheet.className = 'ls-sheet';
    const title = document.createElement('div');
    title.className = 'ls-title';
    title.textContent = 'Dwell';
    this.text = document.createElement('div');
    this.text.className = 'ls-text';
    const bar = document.createElement('div');
    bar.className = 'ls-bar';
    this.fill = document.createElement('div');
    this.fill.className = 'ls-fill';
    bar.append(this.fill);
    sheet.append(title, this.text, bar);
    this.root.append(sheet);
    parent.append(this.root);
  }

  /** The latest frame's loading state (null: open, hidden). */
  set(h: LoadingHud | null): void {
    this.root.hidden = h === null;
    if (h === null) {
      this.shown = null;
      return;
    }
    const key = `${h.text}|${String(h.ready)}|${String(h.needed)}`;
    if (key === this.shown) return;
    this.shown = key;
    this.text.textContent = h.text;
    this.fill.style.width = `${(loadingFraction(h) * 100).toFixed(1)}%`;
  }

  get visible(): boolean {
    return !this.root.hidden;
  }
}
