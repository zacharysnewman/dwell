// Game modes in the shell (ADR 0065): the New world form's choices, from the generated table
// (shared/modes/modes.json via shared/modes/gen.mjs). The server enforces a world's mode; the shell
// only offers and names them.
import { GAME_MODES, type GameMode } from './modes.gen';

/** Whether `name` is a mode of this build. */
export function isGameMode(name: unknown): name is string {
  return typeof name === 'string' && GAME_MODES.some((m) => m.name === name);
}

/** The label the New world form shows: "Adventure (experimental)" for an experimental mode. */
export function modeLabel(mode: GameMode): string {
  return mode.experimental ? `${mode.label} (experimental)` : mode.label;
}
