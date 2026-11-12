//! Theater mode's camera engine (docs/THEATER_MODE.md): camera paths as data, evaluated at a time
//! into a camera pose, played frame by frame, and presets that expand into paths.
//!
//! - [`path`]: the camera path format (keyframes, segments with a move and an easing), read with
//!   messages naming the bad entry and written back.
//! - [`easing`]: the easing curves.
//! - [`eval`]: a path at a time → a [`Pose`] (world position and look, the field of view, the
//!   viewer's time of day), every move kind, both faces through `FaceFrame`.
//! - [`player`]: playing a path with real frame times (speed ramps, smoothed tracking).
//! - [`preset`]: presets as data (`shared/theater/presets.json`) expanded around a subject.
//! - [`viewpoint`]: when the client sends the camera's eye as a simulation viewpoint (§7, T-4).

pub mod easing;
pub mod eval;
pub mod path;
pub mod player;
pub mod preset;
pub mod viewpoint;

pub use easing::Easing;
pub use eval::{NoTargets, Pose, Targets, pose_at};
pub use path::{Aim, Keyframe, Move, Path, Segment, TargetRef};
pub use player::Player;
pub use preset::{Context, Preset, parse_presets, presets};
