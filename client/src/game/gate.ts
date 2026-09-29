// When the client's prediction may tick (PLAYER_CONTROLLER.md §8.4).
import { PlayerState } from '../protocol/constants.gen';

/**
 * Prediction waits until the terrain around the player has arrived, since its collision needs it.
 * A flying player keeps predicting: flight outruns streaming (the chunks come from where the
 * server's copy of the player is), and waiting would stop the inputs the server needs.
 */
export function predictionMayRun(
  active: boolean,
  state: PlayerState,
  terrainReady: boolean,
): boolean {
  return active && (terrainReady || state === PlayerState.Flying);
}
