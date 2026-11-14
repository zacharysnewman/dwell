// Which panel is open over the game, and the pointer lock that goes with it (BUGS.md #69).
//
// One owner of "what is open": playing (no panel), the inventory, the game menu, the big map or
// theater mode's panel.
// Opening a panel frees the mouse; closing one locks it again at once, from the closing gesture
// (the key, the button, Resume). A lock lost while a panel is open is the panel's own doing and
// opens nothing; a lock lost while playing is the browser's (Esc, alt-tab) and opens the game
// menu. A lock request the browser refuses (no user activation, as for Esc, or its cooldown after
// an Esc) is retried on the next click on the game, still without opening the menu.

/** What is open over the game: `none` while playing. */
export type Panel = 'none' | 'inventory' | 'menu' | 'map' | 'theater';

/**
 * The panels' own views, shown or hidden by the owner. `show` returns false when the panel has no
 * view yet (the inventory before the join): it then stays closed.
 */
export interface PanelViews {
  show(panel: Exclude<Panel, 'none'>, open: boolean): boolean;
  /**
   * Esc while `panel` is open: true when the panel used it itself and stays open (theater mode
   * stops a take or a playing path first); absent or false, Esc closes it.
   */
  escape?(panel: Exclude<Panel, 'none'>): boolean;
}

/** The parts of `document` the owner reads. */
export interface LockDocument {
  readonly pointerLockElement: Element | null;
  exitPointerLock(): void;
}

/** The element that takes the lock (the game's canvas). */
export interface LockTarget {
  requestPointerLock?: () => unknown;
}

/** How long after the browser released the lock its Esc keydown is still that release's (ms). */
export const RELEASE_ESC_MS = 300;

export class Panels {
  private current: Panel = 'none';
  private locked = false;
  private releasedAt = -Infinity;

  constructor(
    private readonly doc: LockDocument,
    private readonly target: LockTarget,
    private readonly views: PanelViews,
  ) {}

  /** The panel open now (`none` while playing). */
  get panel(): Panel {
    return this.current;
  }

  isOpen(panel: Panel): boolean {
    return this.current === panel;
  }

  /** Opens `panel` (closing any other) and frees the mouse. */
  open(panel: Exclude<Panel, 'none'>): void {
    if (this.current === panel) return;
    const was = this.current;
    this.current = panel;
    if (was !== 'none') this.views.show(was, false);
    if (!this.views.show(panel, true)) {
      this.current = 'none';
      return;
    }
    this.release();
  }

  /** Closes the open panel (or only `panel`, if given and open) and locks the mouse again. */
  close(panel?: Exclude<Panel, 'none'>): void {
    const was = this.current;
    if (was === 'none' || (panel !== undefined && panel !== was)) return;
    this.current = 'none';
    this.views.show(was, false);
    this.lock();
  }

  toggle(panel: Exclude<Panel, 'none'>): void {
    if (this.current === panel) this.close();
    else this.open(panel);
  }

  /**
   * A panel's view opened or closed by its own control (the menu's Back or backdrop, the ☰
   * button, the map's tap): the owner follows it, as if `open` or `close` had been called.
   */
  shown(panel: Exclude<Panel, 'none'>, open: boolean): void {
    if (open) this.open(panel);
    else if (this.current === panel) {
      this.current = 'none';
      this.lock();
    }
  }

  /** Requests the lock while playing; a refusal is retried by the next `click`. */
  lock(): void {
    if (this.current !== 'none' || this.doc.pointerLockElement === this.target) return;
    const request = this.target.requestPointerLock;
    if (typeof request !== 'function') return;
    try {
      Promise.resolve(request.call(this.target)).catch(() => undefined);
    } catch {
      // Older browsers throw rather than reject; the next click asks again.
    }
  }

  /** `pointerlockchange`: a lock lost while playing opens the game menu. */
  lockChanged(now: number): void {
    const locked = this.doc.pointerLockElement === this.target;
    if (this.locked && !locked && this.current === 'none') {
      this.releasedAt = now;
      this.open('menu');
    }
    this.locked = locked;
  }

  /** Escape: whether it was the panels' (false: the browser's, releasing the lock). */
  escape(now: number): boolean {
    if (this.current === 'menu') {
      // The Esc that released the lock (if a browser delivers it at all) doesn't close the menu.
      if (now - this.releasedAt >= RELEASE_ESC_MS) this.close();
      return true;
    }
    if (this.current !== 'none') {
      if (!this.views.escape?.(this.current)) this.close();
      return true;
    }
    if (this.locked) return false;
    this.open('menu');
    return true;
  }

  /** A click on the game: locks the mouse again if playing without it (a refused request). */
  click(): void {
    if (!this.locked) this.lock();
  }

  private release(): void {
    if (this.doc.pointerLockElement) this.doc.exitPointerLock();
  }
}
