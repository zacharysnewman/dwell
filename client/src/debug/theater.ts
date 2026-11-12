// Theater mode's dev API (docs/THEATER_MODE.md §8, docs/DEV_TOOLS.md "Theater"): `__dwell.theater(op,
// args)` drives the camera path engine (crates/dwell-theater, through the client) for the bench's
// `theater` step and the record skill. The in-game UI (T-2) builds on the same calls.

/** What the dev API needs from the WASM client. */
export interface TheaterClient {
  theaterPlay(json: string): void;
  theaterStop(): void;
  theaterPause(paused: boolean): void;
  theaterState(): unknown;
  theaterPresets(): string;
  theaterPreset(name: string, params: string, subject: string): string;
}

export type TheaterOp = 'play' | 'stop' | 'pause' | 'state' | 'presets' | 'preset' | 'hud';

const HUD_CLASS = 'theater-clean';
const HUD_STYLE_ID = 'theater-clean-style';

/**
 * Hides (or shows again) every element but the game canvas (`#view`): the clean frame for a
 * recording. A class on `body` and one style rule, so the elements the game re-shows stay hidden.
 */
export function setHudShown(doc: Document, shown: boolean): void {
  if (!doc.getElementById(HUD_STYLE_ID)) {
    const style = doc.createElement('style');
    style.id = HUD_STYLE_ID;
    style.textContent = `body.${HUD_CLASS} > *:not(#view):not(script):not(style) { visibility: hidden !important; }`;
    doc.head.appendChild(style);
  }
  doc.body.classList.toggle(HUD_CLASS, !shown);
}

/**
 * One theater operation:
 * - `play {path}`: plays a camera path (an object or its JSON text) from its start;
 * - `stop`: back to the player's eyes (and the HUD shown);
 * - `pause {paused}`; `state` → `{ time, duration, finished }` or null;
 * - `presets` → the presets file; `preset {name, params?, subject?}` → the expanded path (an object);
 * - `hud {shown}`: hides or shows the HUD.
 * Throws with the engine's message on a bad path or preset.
 */
export function theaterOp(
  client: TheaterClient,
  doc: Document,
  op: TheaterOp,
  args: Record<string, unknown> = {},
): unknown {
  switch (op) {
    case 'play': {
      const p = args.path;
      client.theaterPlay(typeof p === 'string' ? p : JSON.stringify(p));
      return true;
    }
    case 'stop':
      client.theaterStop();
      setHudShown(doc, true);
      return true;
    case 'pause':
      client.theaterPause(args.paused !== false);
      return true;
    case 'state':
      return client.theaterState();
    case 'presets':
      return JSON.parse(client.theaterPresets()) as unknown;
    case 'preset':
      return JSON.parse(
        client.theaterPreset(
          String(args.name),
          JSON.stringify(args.params ?? {}),
          JSON.stringify(args.subject ?? 'player'),
        ),
      ) as unknown;
    case 'hud':
      setHudShown(doc, args.shown !== false);
      return true;
    default:
      throw new Error(
        `theater: unknown op ${String(op)} (play, stop, pause, state, presets, preset, hud)`,
      );
  }
}
