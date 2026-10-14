//! When the client's prediction may tick (PLAYER_CONTROLLER.md §8.4).

use dwell_protocol::PlayerState;

/// Prediction waits until the terrain around the player has arrived, since its collision needs it.
/// A flying player keeps predicting: flight outruns streaming (the chunks come from where the
/// server's copy of the player is), and waiting would stop the inputs the server needs.
pub fn prediction_may_run(active: bool, state: PlayerState, terrain_ready: bool) -> bool {
    active && (terrain_ready || state == PlayerState::Flying)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_for_the_terrain_around_a_walking_player() {
        assert!(!prediction_may_run(true, PlayerState::Walking, false));
        assert!(prediction_may_run(true, PlayerState::Walking, true));
        assert!(!prediction_may_run(false, PlayerState::Idle, true));
    }

    #[test]
    fn keeps_predicting_a_flying_player_that_has_outrun_the_streamed_terrain() {
        assert!(prediction_may_run(true, PlayerState::Flying, false));
    }
}
