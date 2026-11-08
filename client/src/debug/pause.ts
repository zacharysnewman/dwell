// The debug pause (docs/DEV_TOOLS.md "Pausing the look"): every drawn animation stands still at one
// instant — the day clock, the clouds, the water, the plants, the flames, motes and flecks — while
// the game runs on, so the same moment can be shot from any angle (the menu panoramas). The client
// keeps one render clock (crates/dwell-client/src/render_clock.rs); this is the shell's switch.

/** What the switch needs of the client. */
export interface Pausable {
  setLookPaused(on: boolean): void;
  lookPaused(): boolean;
}

/** `?pause=1` (or `true`, `on`) starts a page with the look paused. */
export function pauseFromQuery(search: string): boolean {
  const v = new URLSearchParams(search).get('pause');
  return v === '1' || v === 'true' || v === 'on';
}

/** Flips the pause; the new state. */
export function togglePause(client: Pausable): boolean {
  const on = !client.lookPaused();
  client.setLookPaused(on);
  return on;
}
