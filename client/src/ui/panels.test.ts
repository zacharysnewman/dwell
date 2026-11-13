import { describe, expect, it } from 'vitest';
import { Panels, RELEASE_ESC_MS, type Panel } from './panels';

/**
 * A fake document and canvas: `requestPointerLock` locks only inside a user gesture (as browsers
 * do), and the lock's changes reach the panels as the browser's `pointerlockchange` would.
 */
function rig() {
  const shown: Record<Exclude<Panel, 'none'>, boolean> = {
    inventory: false,
    menu: false,
    map: false,
  };
  const missing = new Set<Panel>();
  let gesture = false;
  const requests: boolean[] = [];
  const target = {
    requestPointerLock: () => {
      requests.push(gesture);
      if (!gesture) return Promise.reject(new Error('NotAllowedError'));
      doc.pointerLockElement = target as unknown as Element;
      changed();
      return Promise.resolve();
    },
  };
  const doc = {
    pointerLockElement: null as Element | null,
    exitPointerLock: () => {
      doc.pointerLockElement = null;
      changed();
    },
  };
  let now = 1000;
  const panels = new Panels(doc, target, {
    show: (p, open) => {
      if (missing.has(p)) return false;
      shown[p] = open;
      return true;
    },
  });
  function changed(): void {
    panels.lockChanged(now);
  }
  return {
    panels,
    shown,
    missing,
    requests,
    locked: () => doc.pointerLockElement !== null,
    /** Runs `f` as a user gesture (a key other than Esc, a click). */
    gesture: (f: () => void) => {
      gesture = true;
      f();
      gesture = false;
    },
    /** The browser takes the lock away (Esc while locked, alt-tab). */
    browserReleases: () => {
      doc.pointerLockElement = null;
      changed();
    },
    later: (ms: number) => {
      now += ms;
    },
    get now() {
      return now;
    },
  };
}

/** A rig that is playing with the mouse locked. */
function playing() {
  const r = rig();
  r.gesture(() => {
    r.panels.click();
  });
  expect(r.locked()).toBe(true);
  return r;
}

describe('Panels (BUGS.md #137)', () => {
  it('opening the inventory opens only the inventory and frees the mouse', () => {
    const r = playing();
    r.gesture(() => {
      r.panels.open('inventory');
    });
    expect(r.locked()).toBe(false);
    expect(r.shown.inventory).toBe(true);
    expect(r.shown.menu).toBe(false);
    expect(r.panels.panel).toBe('inventory');
  });

  it('closing the inventory locks the mouse again within the same gesture', () => {
    const r = playing();
    r.gesture(() => {
      r.panels.toggle('inventory');
    });
    r.gesture(() => {
      r.panels.toggle('inventory');
    });
    expect(r.shown.inventory).toBe(false);
    expect(r.requests.at(-1)).toBe(true);
    expect(r.locked()).toBe(true);
    expect(r.shown.menu).toBe(false);
  });

  it('Esc in the inventory closes it, not the menu', () => {
    const r = playing();
    r.gesture(() => {
      r.panels.open('inventory');
    });
    r.later(1000);
    // Esc grants no user activation: the lock request is refused, and nothing else opens.
    expect(r.panels.escape(r.now)).toBe(true);
    expect(r.shown.inventory).toBe(false);
    expect(r.shown.menu).toBe(false);
    expect(r.panels.panel).toBe('none');
    // The next click on the game locks it.
    r.gesture(() => {
      r.panels.click();
    });
    expect(r.locked()).toBe(true);
    expect(r.shown.menu).toBe(false);
  });

  it('a lock lost to the browser (Esc, alt-tab) while playing opens the menu', () => {
    const r = playing();
    r.browserReleases();
    expect(r.shown.menu).toBe(true);
    expect(r.panels.panel).toBe('menu');
    // That release's own Esc keydown doesn't close it again.
    expect(r.panels.escape(r.now + RELEASE_ESC_MS / 2)).toBe(true);
    expect(r.shown.menu).toBe(true);
  });

  it("the menu's Resume (its view closing) locks the mouse again from the click", () => {
    const r = playing();
    r.browserReleases();
    r.later(2000);
    r.gesture(() => {
      // Resume hides the menu itself; the owner follows the view.
      r.shown.menu = false;
      r.panels.shown('menu', false);
    });
    expect(r.panels.panel).toBe('none');
    expect(r.locked()).toBe(true);
  });

  it('a refused re-lock (the browser cooldown after Esc) waits for a click, without the menu', () => {
    const r = playing();
    r.browserReleases();
    r.later(2000);
    // Closed by the controller or Esc: no gesture, refused.
    r.panels.close();
    expect(r.locked()).toBe(false);
    expect(r.shown.menu).toBe(false);
    r.gesture(() => {
      r.panels.click();
    });
    expect(r.locked()).toBe(true);
    expect(r.shown.menu).toBe(false);
  });

  it('Esc in the open menu closes it and asks for the lock', () => {
    const r = playing();
    r.browserReleases();
    r.later(2000);
    expect(r.panels.escape(r.now)).toBe(true);
    expect(r.shown.menu).toBe(false);
    expect(r.requests.at(-1)).toBe(false);
  });

  it('Esc while playing without the lock opens the menu; with it, the browser has it', () => {
    const r = rig();
    expect(r.panels.escape(r.now)).toBe(true);
    expect(r.shown.menu).toBe(true);
    const p = playing();
    expect(p.panels.escape(p.now)).toBe(false);
    expect(p.shown.menu).toBe(false);
  });

  it('the panels are one at a time: the menu over the inventory closes the inventory', () => {
    const r = playing();
    r.gesture(() => {
      r.panels.open('inventory');
    });
    r.panels.open('menu');
    expect(r.shown.inventory).toBe(false);
    expect(r.shown.menu).toBe(true);
    r.gesture(() => {
      r.panels.close();
    });
    expect(r.panels.panel).toBe('none');
    expect(r.locked()).toBe(true);
  });

  it('a panel with no view yet stays closed, and the lock comes back on a click', () => {
    const r = playing();
    r.missing.add('menu');
    r.browserReleases();
    expect(r.panels.panel).toBe('none');
    r.gesture(() => {
      r.panels.click();
    });
    expect(r.locked()).toBe(true);
  });

  it('the big map opened by its own control frees the mouse and opens no menu', () => {
    const r = playing();
    r.shown.map = true;
    r.panels.shown('map', true);
    expect(r.locked()).toBe(false);
    expect(r.shown.menu).toBe(false);
    expect(r.panels.escape(r.now + 1000)).toBe(true);
    expect(r.shown.map).toBe(false);
    expect(r.shown.menu).toBe(false);
  });
});
