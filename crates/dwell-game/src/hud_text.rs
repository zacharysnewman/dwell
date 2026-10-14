//! What the HUD says (ARCHITECTURE.md §5): the centre message is for states that stop play
//! (joining, death); background work such as terrain loading is a small status in a corner.

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HudTextInput {
    pub dead: bool,
    /// The session has spawned the player.
    pub active: bool,
    pub terrain_ready: bool,
    pub terrain_error: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HudText {
    pub center: String,
    pub status: String,
}

pub fn hud_text(s: &HudTextInput) -> HudText {
    if s.dead {
        return HudText {
            center: "You died — respawning…".into(),
            status: String::new(),
        };
    }
    if !s.active && s.terrain_error.is_none() {
        return HudText {
            center: "Joining…".into(),
            status: String::new(),
        };
    }
    let status = match &s.terrain_error {
        Some(e) => format!("Terrain unavailable: {e}"),
        None if s.terrain_ready => String::new(),
        None => "Loading terrain…".into(),
    };
    HudText {
        center: String::new(),
        status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playing() -> HudTextInput {
        HudTextInput {
            dead: false,
            active: true,
            terrain_ready: true,
            terrain_error: None,
        }
    }

    fn text(center: &str, status: &str) -> HudText {
        HudText {
            center: center.into(),
            status: status.into(),
        }
    }

    #[test]
    fn shows_terrain_loading_as_a_corner_status_never_over_the_view() {
        // Regression (phone playtest): flying fast kept "Loading terrain…" in the middle of the view.
        let loading = HudTextInput {
            terrain_ready: false,
            ..playing()
        };
        assert_eq!(hud_text(&loading), text("", "Loading terrain…"));
        let failed = HudTextInput {
            terrain_error: Some("worker failed".into()),
            ..playing()
        };
        assert_eq!(
            hud_text(&failed),
            text("", "Terrain unavailable: worker failed")
        );
    }

    #[test]
    fn keeps_the_centre_for_what_stops_play_joining_and_death() {
        let joining = HudTextInput {
            active: false,
            terrain_ready: false,
            ..playing()
        };
        assert_eq!(hud_text(&joining).center, "Joining…");
        let dead = HudTextInput {
            dead: true,
            ..playing()
        };
        assert_eq!(hud_text(&dead).center, "You died — respawning…");
        assert_eq!(hud_text(&playing()), text("", ""));
    }
}
