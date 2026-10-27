//! Dwell's client game logic (ADR 0028, Phase 24e), ported from `client/src/game`, `predict`,
//! `interact` and `world`: remote player interpolation, first-person eye smoothing, the camera
//! across the midplane between the faces, the HUD's text, the prediction gate, creative flight, the
//! keyboard/mouse and touch input state machines, block targeting and edit requests, and the
//! client's chunk streaming. Platform-free: time is passed in, collaborators (renderer, mesher,
//! worldgen, network) are traits or callbacks, so it all runs natively under test and in the browser.

pub mod block_interaction;
pub mod brush_tool;
pub mod chunk_diff;
pub mod chunk_stream;
pub mod edit_prediction;
pub mod eye;
pub mod face_flip;
pub mod flight;
pub mod flight_speed;
pub mod gamepad;
pub mod gate;
pub mod hud_text;
pub mod input;
pub mod items;
pub mod loading;
pub mod region_tools;
pub mod remotes;
pub mod sound;
pub mod touch;

/// Rounds half up, like JavaScript's `Math.round`.
pub(crate) fn js_round(v: f64) -> f64 {
    (v + 0.5).floor()
}

/// JavaScript's `%` on numbers (the result takes the dividend's sign), then wrapped into [0, m).
pub(crate) fn wrap_degrees(v: f64) -> f64 {
    ((v % 360.0) + 360.0) % 360.0
}

/// Vitest's `toBeCloseTo(expected, digits)`: |actual − expected| < 10^−digits / 2.
#[cfg(test)]
#[track_caller]
pub(crate) fn assert_close(actual: f64, expected: f64, digits: i32) {
    let tolerance = 10f64.powi(-digits) / 2.0;
    assert!(
        (actual - expected).abs() < tolerance,
        "expected {actual} to be within {tolerance} of {expected}"
    );
}
