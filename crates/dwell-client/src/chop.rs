//! The client's side of chopping (docs/adr/0067-stone-axe-and-chopping.md): the held axe's swing
//! direction (Use toggles it), the swing itself as weighty first-person melee (owner, 2026-11-08: a
//! short wind-up, a committed arc whose path is what hits, a hit-stop on impact, a looser
//! follow-through and a longer recovery on a miss: [`SwingRun`]), and the server's cuts kept for
//! drawing. A swing can start at any time, at nothing (a free swing); the server sweeps the same
//! arc (`ChopAction::Sweep`) and cuts what it meets.

use dwell_protocol::chopping::{CameraOffset, Cut, ToolDef};
use dwell_protocol::{ChopAction, CutsMessage};
use dwell_world::{CELL_H, m_of_row};
use std::collections::HashMap;

/// The chopping prototype's aim (docs/WORK_QUEUE.md item 7, dev `?chop=v2`): a swing's predicted
/// impact is where the look meets the trunk, as the prototype's server cuts it; off: the arc's.
static CROSSHAIR_AIM: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Turns the chopping prototype's crosshair aim on or off (the page's `?chop=v2`).
pub fn set_crosshair_aim(on: bool) {
    CROSSHAIR_AIM.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub fn crosshair_aim() -> bool {
    CROSSHAIR_AIM.load(std::sync::atomic::Ordering::Relaxed)
}

/// The chopping prototype's impact: the first solid cell the look from `eye` meets within the
/// target outline's reach past what it clears, the bite timed to the middle of the arc (the hit-stop there).
pub fn crosshair_impact(
    store: &mut dwell_world::VoxelWorld,
    tool: &ToolDef,
    eye: [f64; 3],
    look: [f64; 3],
) -> Option<Impact> {
    let hit = dwell_world::edit::raycast_block_past(
        store,
        eye,
        look.map(|v| v as f32),
        // As far as the block-target outline (a trunk you can outline you can chop).
        dwell_protocol::players::REACH_DISTANCE,
        // A wood's fell wedge (a stump's face) stops the look, as on the server.
        |_, m| {
            let name = dwell_blocks::block_of(m).name;
            tool.clears_block(name)
                && !dwell_protocol::chopping::WOODS
                    .iter()
                    .any(|w| w.fell_wedge == Some(name))
        },
    )?;
    let d = f64::from(hit.distance);
    let m = store.get_voxel(hit.cell[0], hit.cell[1], hit.cell[2]);
    Some(Impact {
        cell: hit.cell,
        face: hit.face ^ 1,
        point: [0, 1, 2].map(|i| eye[i] + look[i] * d),
        step: tool.swing.samples / 2,
        wood: dwell_protocol::chopping::chopped_wood(dwell_blocks::block_of(m).name).is_some(),
    })
}

#[derive(Debug, Default)]
pub struct Chopping {
    /// The server keeps cuts (Welcome's `CHOPPING`).
    pub on: bool,
    /// The server sweeps swings along their path (Welcome's `CHOP_SWEEP`); else a swing that meets
    /// a wood is sent as the older aimed `ChopAction::Swing`.
    pub sweep: bool,
    /// The swing's plane: vertical, else level (the default: across a standing trunk).
    pub vertical: bool,
    /// When the last swing went (ms).
    last_ms: Option<f64>,
    /// The cuts the server sent, by cell.
    pub cuts: HashMap<[i32; 3], Cut>,
    /// The swing under way, if any.
    pub run: Option<SwingRun>,
    /// The cells the swing under way clears on its path (the tool's `clears`; the server breaks
    /// them), shown when the head reaches the first of them (`SwingRun::take_clear`).
    pub cleared: Vec<[i32; 3]>,
}

/// What a swing's path met (the client's prediction of the server's sweep).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Impact {
    pub cell: [i32; 3],
    /// The face it entered (`axis × 2 + positive`, world) and where (world m).
    pub face: u8,
    pub point: [f64; 3],
    /// The arc step it was met at.
    pub step: u32,
    /// A wood the axe bites; else it bounces off (stone, dirt: a clank and a jolt, no effect).
    pub wood: bool,
}

/// One swing, from its start: the wind-up, the arc, and either the hit-stop and a pull back out
/// (a hit) or the follow-through and a longer recovery (a miss). All its times are the tool's
/// `swing` data.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SwingRun {
    pub tool: &'static ToolDef,
    pub vertical: bool,
    pub start_ms: f64,
    /// What its path meets, if anything (none: a free swing at the air).
    pub hit: Option<Impact>,
    /// The impact's effects have been shown.
    pub impacted: bool,
    /// The arc step it first goes through a cell it clears (none: it clears nothing), and whether
    /// that has been shown (a rustle and falling leaves: not a hit, so no hit-stop, no bounce).
    pub clear_step: Option<u32>,
    pub cleared_shown: bool,
}

/// How the arms' swing clip (the action layer) is driven for a swing this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ArmsDrive {
    /// Held at clip time `t` (s), laid over the arms' pose at `weight` (1: the clip's pose).
    At { t: f64, weight: f64 },
    /// The swing is over: its clip stops (BUGS.md #93: let go, it played on from where it was
    /// held, a second swing).
    Release,
}

impl ArmsDrive {
    /// Drives the arms' animator `a` so.
    pub fn apply(self, a: &mut dwell_characters::animator::Animator) {
        match self {
            ArmsDrive::At { t, weight } => a.drive_action_weighted(t as f32, weight as f32),
            ArmsDrive::Release => a.stop_action(),
        }
    }
}

/// Smooth 0..1.
fn ease(u: f64) -> f64 {
    let u = u.clamp(0.0, 1.0);
    u * u * (3.0 - 2.0 * u)
}

impl SwingRun {
    pub fn new(tool: &'static ToolDef, vertical: bool, start_ms: f64, hit: Option<Impact>) -> Self {
        Self {
            tool,
            vertical,
            start_ms,
            hit,
            impacted: false,
            clear_step: None,
            cleared_shown: false,
        }
    }

    /// Whether the moment it reaches the first cell it clears has come (true once).
    pub fn take_clear(&mut self, now_ms: f64) -> bool {
        let Some(step) = self.clear_step else {
            return false;
        };
        if self.cleared_shown || self.elapsed(now_ms) < self.tool.swing.time_of_step(step) {
            return false;
        }
        self.cleared_shown = true;
        true
    }

    /// Seconds since it started.
    pub fn elapsed(&self, now_ms: f64) -> f64 {
        ((now_ms - self.start_ms) / 1000.0).max(0.0)
    }

    /// When along it the head meets what it hits (s), if it hits.
    pub fn impact_s(&self) -> Option<f64> {
        self.hit.map(|h| self.tool.swing.time_of_step(h.step))
    }

    /// How long it lasts: shorter for a hit (stopped, pulled out) than a miss (followed through).
    pub fn length(&self) -> f64 {
        match self.hit {
            Some(h) => self.tool.swing.hit_length(h.step),
            None => self.tool.swing.miss_length(),
        }
    }

    pub fn done(&self, now_ms: f64) -> bool {
        self.elapsed(now_ms) >= self.length()
    }

    /// The impact, once, when its moment has come.
    pub fn take_impact(&mut self, now_ms: f64) -> Option<Impact> {
        let at = self.impact_s()?;
        if self.impacted || self.elapsed(now_ms) < at {
            return None;
        }
        self.impacted = true;
        self.hit
    }

    /// How the arms' swing clip is driven `now_ms` into this swing (`Bodies::drive_arms`, each
    /// frame the swing is under way and the frame it ends).
    pub fn arms(&self, now_ms: f64) -> ArmsDrive {
        if self.done(now_ms) {
            return ArmsDrive::Release;
        }
        let t = self.elapsed(now_ms);
        ArmsDrive::At {
            t: self.clip_time(t),
            weight: self.clip_weight(t),
        }
    }

    /// How much of the swing clip's pose the arms show `t` s in: all of it, except a hit's
    /// pull-out, which fades the held contact pose to the arms' rest over `recoverHitS` (straight
    /// back, not the wind-up played backwards: BUGS.md #93, a vertical hit went back up over the
    /// shoulder and down again, a second stroke).
    pub fn clip_weight(&self, t: f64) -> f64 {
        let s = &self.tool.swing;
        match self.impact_s() {
            Some(at) if t >= at + s.hit_stop_s => {
                let u = ((t - at - s.hit_stop_s) / s.recover_hit_s.max(1e-6)).clamp(0.0, 1.0);
                1.0 - ease(u)
            }
            _ => 1.0,
        }
    }

    /// Where in the arms' swing clip (whose length is a miss's: wind-up, arc, follow-through,
    /// recovery) the arms are `t` s in: a hit holds the impact's pose from contact on, through
    /// the hit-stop and the pull-out ([`clip_weight`](Self::clip_weight) fades it to rest).
    pub fn clip_time(&self, t: f64) -> f64 {
        let s = &self.tool.swing;
        match self.impact_s() {
            None => t.min(s.miss_length()),
            Some(at) => t.min(at),
        }
    }

    /// The view's sway with the swing `t` s in (docs/CHARACTERS.md §5.2): [`swing_camera`] for
    /// this run.
    pub fn camera(&self, t: f64) -> CameraOffset {
        let hit = self
            .impact_s()
            .map(|at| (at, self.hit.is_some_and(|h| h.wood)));
        swing_camera(&self.tool.swing, self.vertical, hit, t)
    }
}

/// The view's sway (the tool's `swing.camera` for the swing's kind; owner, 2026-11-12: "The camera
/// could have some sway with the chop") `t` s into a swing, an offset to the view drawn, never the
/// aim. The keys follow the miss's timeline (wind-up, arc, recovery) up to the head's contact; a
/// hit (`hit`: when, and whether into a wood) then holds the contact's offset through the hit-stop
/// and eases it home over the pull-out, while a miss runs on through its keys. The kick lands at
/// the contact, a hit's or the arc's middle on a miss (at `kick.miss` of its size; a bounce off
/// what is not a wood at `kick.bounce`), dying away over `decay_s` and gone by the swing's end.
/// Zero before the swing, at its end and after.
pub fn swing_camera(
    s: &dwell_protocol::chopping::SwingDef,
    vertical: bool,
    hit: Option<(f64, bool)>,
    t: f64,
) -> CameraOffset {
    let cam = s.camera(vertical);
    let (contact, end, k) = match hit {
        Some((at, wood)) => (
            at,
            at + s.hit_stop_s + s.recover_hit_s,
            if wood { 1.0 } else { cam.kick.bounce },
        ),
        None => (s.centre_s(), s.miss_length(), cam.kick.miss),
    };
    if !(0.0..end).contains(&t) {
        return CameraOffset::ZERO;
    }
    let keyed = match hit {
        Some((at, _)) if t >= at => {
            let home = 1.0 - ease((t - at - s.hit_stop_s) / s.recover_hit_s.max(1e-6));
            cam.at(s.phase_time(at)).scaled(home)
        }
        _ => cam.at(s.phase_time(t)),
    };
    if t < contact {
        return keyed;
    }
    // The kick: at once at the contact, dying away, and faded out by the swing's end.
    let after = t - contact;
    let left = (end - contact).max(1e-6);
    let envelope = (-after / cam.kick.decay_s.max(1e-6)).exp() * (1.0 - ease(after / left));
    keyed.plus(cam.kick.offset.scaled(k * envelope))
}

/// A camera's place and turn: the eye (world m), yaw and pitch (degrees, the game's) and roll
/// (radians, the renderer's `aim`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewPose {
    pub eye: [f64; 3],
    pub yaw: f64,
    pub pitch: f64,
    pub roll: f64,
}

/// The view drawn from the aim's pose `aim` with the swing's sway `sway` added (off: `on` false,
/// the Camera sway setting) and any other turn of the view `extra` (a blast's shake: yaw, pitch
/// degrees, the game's). Face-local through `face`: the sway's yaw toward the screen's right is
/// `FaceFrame::handed` of the game's yaw (which turns left on face A), its roll and bob are along
/// the view's own axes. The aim (targeting, the crosshair, the server, the facing) keeps `aim`.
pub fn swayed_view(
    aim: ViewPose,
    sway: CameraOffset,
    on: bool,
    extra: (f64, f64),
    face: dwell_world::FaceFrame,
) -> ViewPose {
    let sway = if on { sway } else { CameraOffset::ZERO };
    let yaw = aim.yaw + extra.0 + face.handed(-sway.yaw);
    let pitch = aim.pitch + extra.1 + sway.pitch;
    // Leaning right is the view's right side down: a negative turn about its back axis.
    let roll = aim.roll - sway.roll.to_radians();
    let b = crate::render::camera::aim(yaw, pitch, face, roll);
    let eye = std::array::from_fn(|k| {
        aim.eye[k] + b.right[k] * sway.bob[0] + b.up[k] * sway.bob[1] + b.back[k] * sway.bob[2]
    });
    ViewPose {
        eye,
        yaw,
        pitch,
        roll,
    }
}

impl Chopping {
    /// Use with the axe held: the swing turns between level and vertical.
    pub fn toggle(&mut self) -> bool {
        self.vertical = !self.vertical;
        self.vertical
    }

    /// Whether a new swing may start: the last one is over, and the server's wait has passed.
    pub fn ready(&self, tool: &ToolDef, now_ms: f64) -> bool {
        self.run.is_none_or(|r| r.done(now_ms))
            && self
                .last_ms
                .is_none_or(|l| now_ms - l >= tool.swing_seconds * 1000.0)
    }

    /// Starts a swing of `tool` (any time the last is over: at nothing too), its path meeting
    /// `hit`, looking along `look` (world, unit). None if it can't swing yet; else the message for
    /// the server: the swing's start to a server that sweeps, else the aimed swing at a wood it
    /// meets (an older server), else nothing.
    pub fn start(
        &mut self,
        tool: &'static ToolDef,
        look: [f64; 3],
        hit: Option<Impact>,
        now_ms: f64,
    ) -> Option<Option<ChopAction>> {
        if !self.on || !self.ready(tool, now_ms) {
            return None;
        }
        self.last_ms = Some(now_ms);
        self.run = Some(SwingRun::new(tool, self.vertical, now_ms, hit));
        Some(if self.sweep {
            Some(ChopAction::Sweep {
                tool: tool.name.to_owned(),
                look: look.map(|v| v as f32),
                vertical: self.vertical,
            })
        } else {
            hit.filter(|h| h.wood).map(|h| ChopAction::Swing {
                tool: tool.name.to_owned(),
                cell: h.cell,
                face: h.face,
                point: h.point.map(|v| v as f32),
                vertical: self.vertical,
            })
        })
    }

    /// The swing just started clears `cleared` on its path (the sweep's, in the head's order).
    pub fn set_cleared(&mut self, cleared: &[dwell_world::swing::Cleared]) {
        self.cleared = cleared.iter().map(|c| c.cell).collect();
        if let Some(run) = &mut self.run {
            run.clear_step = cleared.iter().map(|c| c.step).min();
            run.cleared_shown = false;
        }
    }

    pub fn apply(&mut self, m: CutsMessage) {
        for c in m.gone {
            self.cuts.remove(&c);
        }
        self.cuts.extend(m.cuts);
    }

    /// The hint shown while a tool of `kind` is held, in the current stance (the kind's `hints`).
    pub fn hint(&self, kind: dwell_protocol::chopping::ToolKind) -> Option<&'static str> {
        dwell_protocol::chopping::hint(kind, self.vertical)
    }
}

/// A wood state's grain: its `axis` property as a world axis (0 x, 1 y, 2 z).
pub fn grain_of(m: dwell_blocks::StateId) -> usize {
    match dwell_blocks::state_property(m, "axis") {
        Some("x") => 0,
        Some("z") => 2,
        _ => 1,
    }
}

/// A standing tree's lean as it is cut (the wood's `sway`): the part above `hinge` within
/// `radius` across and `height` up turns by `angle` (radians) about `axis` through it, its top
/// toward the notch (the way it will fall). Drawn by the terrain's vertex shaders
/// (render/shaders.rs `tree_lean`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TreeLean {
    /// The hinge: the uncut side of the cut cell, at its middle height (world).
    pub hinge: [f64; 3],
    /// The tree's up (world: the face's).
    pub up: [f64; 3],
    /// The turn's axis (world, unit): up × the notch's outward normal.
    pub axis: [f64; 3],
    pub angle: f64,
    pub radius: f64,
    pub height: f64,
}

impl TreeLean {
    /// The lean of the standing wood `cell` cut `cut`, `since_s` after the last hit: the wood's
    /// sway by how far it is cut (none before its `fromFraction`), a shiver after the hit.
    pub fn of(
        cell: [i32; 3],
        cut: &Cut,
        wood: &dwell_protocol::chopping::WoodDef,
        since_s: f64,
    ) -> Option<Self> {
        let deg = wood.sway_deg(cut.cross(1));
        if deg <= 0.0 {
            return None;
        }
        let side = cut.deepest(1)?;
        let n = dwell_protocol::chopping::side_normal(1, side).map(f64::from);
        let f = dwell_world::FaceFrame::at_row(cell[1]);
        let up = f.up();
        let c = dwell_world::centre_of_cell(cell);
        let shiver = 0.45 * deg * (-since_s.max(0.0) * 2.5).exp() * (since_s * 13.0).sin();
        let breathe = 0.12 * deg * (since_s * 1.6).sin();
        let axis = [
            up[1] * n[2] - up[2] * n[1],
            up[2] * n[0] - up[0] * n[2],
            up[0] * n[1] - up[1] * n[0],
        ];
        Some(Self {
            hinge: std::array::from_fn(|k| c[k] - 0.5 * n[k]),
            up,
            axis,
            angle: (deg + shiver + breathe).to_radians(),
            radius: wood.leaves.radius_m.max(1.0),
            height: wood.leaves.height_m.max(2.0) + 2.0,
        })
    }

    /// Where world point `p` is drawn: turned with the tree when above the hinge and within it.
    pub fn apply(&self, p: [f64; 3]) -> [f64; 3] {
        let d: [f64; 3] = std::array::from_fn(|k| p[k] - self.hinge[k]);
        let h = (0..3).map(|k| d[k] * self.up[k]).sum::<f64>();
        let across: [f64; 3] = std::array::from_fn(|k| d[k] - self.up[k] * h);
        if h <= 0.0
            || h > self.height
            || across.iter().map(|v| v * v).sum::<f64>().sqrt() > self.radius
        {
            return p;
        }
        let k = self.axis;
        let (s, c) = self.angle.sin_cos();
        let kd = (0..3).map(|i| k[i] * d[i]).sum::<f64>();
        let cross = [
            k[1] * d[2] - k[2] * d[1],
            k[2] * d[0] - k[0] * d[2],
            k[0] * d[1] - k[1] * d[0],
        ];
        std::array::from_fn(|i| self.hinge[i] + d[i] * c + cross[i] * s + k[i] * kd * (1.0 - c))
    }
}

/// Where a ray from `eye` along `dir` enters cell `cell`'s box: the face (`axis × 2 + positive`,
/// world) and the point; none if it misses or starts inside.
pub fn ray_cell_hit(eye: [f64; 3], dir: [f64; 3], cell: [i32; 3]) -> Option<(u8, [f64; 3])> {
    let lo = [f64::from(cell[0]), m_of_row(cell[1]), f64::from(cell[2])];
    let hi = [lo[0] + 1.0, lo[1] + CELL_H, lo[2] + 1.0];
    let (mut t_in, mut t_out, mut face) = (f64::NEG_INFINITY, f64::INFINITY, None);
    for a in 0..3 {
        if dir[a].abs() < 1e-12 {
            if eye[a] < lo[a] || eye[a] > hi[a] {
                return None;
            }
            continue;
        }
        let (t0, t1) = ((lo[a] - eye[a]) / dir[a], (hi[a] - eye[a]) / dir[a]);
        let (near, far) = if t0 < t1 { (t0, t1) } else { (t1, t0) };
        if near > t_in {
            t_in = near;
            // Entering through the low side means the face whose normal points back: negative.
            face = Some((a * 2) as u8 + u8::from(dir[a] < 0.0));
        }
        t_out = t_out.min(far);
    }
    if t_in > t_out || t_in < 0.0 {
        return None;
    }
    Some((face?, std::array::from_fn(|k| eye[k] + dir[k] * t_in)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn axe() -> &'static ToolDef {
        dwell_protocol::chopping::tool("dwell:stone_axe").unwrap()
    }

    fn impact(step: u32, wood: bool) -> Impact {
        Impact {
            cell: [3, 0, 0],
            face: 0,
            point: [3.0, 0.2, 0.5],
            step,
            wood,
        }
    }

    #[test]
    fn a_ray_enters_a_cell_through_the_face_it_meets() {
        // From −x, level: the −x face (0), at x = 3.
        let (f, p) = ray_cell_hit([0.5, 0.2, 0.5], [1.0, 0.0, 0.0], [3, 0, 0]).unwrap();
        assert_eq!(f, 0);
        assert!((p[0] - 3.0).abs() < 1e-9);
        // From above: the +y face (3), at the row's top.
        let (f, p) = ray_cell_hit([3.5, 5.0, 0.5], [0.0, -1.0, 0.0], [3, 0, 0]).unwrap();
        assert_eq!(f, 3);
        assert!((p[1] - CELL_H).abs() < 1e-9);
        // From below (face B's up): the −y face (2).
        assert_eq!(
            ray_cell_hit([3.5, -5.0, 0.5], [0.0, 1.0, 0.0], [3, 0, 0])
                .unwrap()
                .0,
            2
        );
        assert_eq!(
            ray_cell_hit([0.5, 0.2, 0.5], [-1.0, 0.0, 0.0], [3, 0, 0]),
            None
        );
    }

    #[test]
    fn a_free_swing_goes_at_any_time_at_nothing_and_follows_through() {
        let mut c = Chopping {
            on: true,
            sweep: true,
            ..Default::default()
        };
        // Nothing on the path: it still swings, and tells the server (which sweeps it too).
        let m = c.start(axe(), [1.0, 0.0, 0.0], None, 1000.0);
        assert!(
            matches!(
                m,
                Some(Some(ChopAction::Sweep {
                    vertical: false,
                    ..
                }))
            ),
            "{m:?}"
        );
        let run = c.run.expect("a swing under way");
        assert_eq!(run.length(), axe().swing.miss_length());
        assert_eq!(run.impact_s(), None);
        // Not again until it is over (the follow-through and recovery), then at once.
        let end = 1000.0 + run.length() * 1000.0;
        assert!(c.start(axe(), [1.0, 0.0, 0.0], None, end - 5.0).is_none());
        assert!(c.start(axe(), [1.0, 0.0, 0.0], None, end + 1.0).is_some());
        // An older server (no sweep) is sent only a swing that meets a wood, as the aimed swing.
        let mut old = Chopping {
            on: true,
            ..Default::default()
        };
        assert_eq!(old.start(axe(), [1.0, 0.0, 0.0], None, 0.0), Some(None));
        let m = old.start(axe(), [1.0, 0.0, 0.0], Some(impact(3, true)), 5000.0);
        assert!(
            matches!(m, Some(Some(ChopAction::Swing { face: 0, .. }))),
            "{m:?}"
        );
        assert!(
            Chopping::default()
                .start(axe(), [1.0, 0.0, 0.0], None, 0.0)
                .is_none(),
            "no CHOPPING"
        );
    }

    #[test]
    fn a_hit_stops_on_impact_and_pulls_back_sooner_than_a_miss_recovers() {
        let s = axe().swing;
        let mut hit = SwingRun::new(axe(), false, 0.0, Some(impact(4, true)));
        let miss = SwingRun::new(axe(), false, 0.0, None);
        let at = hit.impact_s().unwrap();
        assert!(
            at > s.windup_s && at < s.windup_s + s.active_s,
            "on the arc: {at}"
        );
        // The hit-stop: the arms hold the impact's pose for its length.
        assert_eq!(hit.clip_time(at + 0.5 * s.hit_stop_s), at);
        assert_eq!(hit.clip_time(at + 0.99 * s.hit_stop_s), at);
        // Then back out to the rest; the miss runs on through its follow-through.
        // The pull-out holds the contact's pose and fades it to rest (BUGS.md #93).
        assert_eq!(hit.clip_time(at + s.hit_stop_s + 0.5 * s.recover_hit_s), at);
        assert_eq!(hit.clip_weight(at + 0.99 * s.hit_stop_s), 1.0);
        let w = hit.clip_weight(at + s.hit_stop_s + 0.5 * s.recover_hit_s);
        assert!(w > 0.0 && w < 1.0, "{w}");
        assert!(hit.clip_weight(hit.length()) < 1e-9);
        assert_eq!(miss.clip_weight(miss.length() - 1e-3), 1.0);
        assert!(miss.clip_time(at + s.hit_stop_s) > at + s.hit_stop_s - 1e-9);
        assert!(hit.length() < miss.length(), "a miss recovers longer");
        // The impact fires once, at its moment.
        assert_eq!(hit.take_impact(at * 1000.0 - 1.0), None);
        assert_eq!(hit.take_impact(at * 1000.0 + 1.0), Some(impact(4, true)));
        assert_eq!(hit.take_impact(at * 1000.0 + 2.0), None);
    }

    #[test]
    fn the_arms_swing_clips_last_as_long_as_a_miss() {
        // The clips are timed to a miss (the arc, the follow-through); a hit is driven over them.
        let ch = dwell_characters::player();
        for role in ["chop_level", "chop_vertical"] {
            let clip = dwell_characters::TABLES.clip(ch.arms_clip(role).unwrap());
            assert!(
                (f64::from(clip.duration) - axe().swing.miss_length()).abs() < 1e-3,
                "{role}: {} s",
                clip.duration
            );
        }
    }

    dwell_world::both_faces!(
        fn a_tree_leans_toward_its_notch_more_toward_the_fall(f: FaceFrame) {
            let log = dwell_protocol::chopping::wood("dwell:log").unwrap();
            let cell = f.cell([3, 80, 0]);
            // Notched on its −x side (side 0).
            let cut_at = |frac: f64| {
                let mut c = Cut::default();
                c.notch[0] = (frac * 1.0) as f32;
                c
            };
            assert_eq!(
                TreeLean::of(cell, &cut_at(0.2), log, 5.0),
                None,
                "still at 20%"
            );
            let mid = TreeLean::of(cell, &cut_at(0.48), log, 5.0).expect("leaning");
            let near = TreeLean::of(cell, &cut_at(0.54), log, 5.0).expect("leaning");
            assert!(
                near.angle > mid.angle && mid.angle > 0.0,
                "{} {}",
                mid.angle,
                near.angle
            );
            // Its top goes toward the notch (−x), face-locally level; the ground below stays.
            let top = f.point([3.5, dwell_world::m_of_row(80) + 4.0, 0.5]);
            let moved = f.point(near.apply(top));
            let was = f.point(top);
            assert!(
                moved[0] < was[0] - 0.05,
                "toward the notch: {was:?} to {moved:?}"
            );
            let ground = f.point([3.5, dwell_world::m_of_row(78), 0.5]);
            assert_eq!(near.apply(ground), ground);
            // A shiver right after a hit.
            let shaken = TreeLean::of(cell, &cut_at(0.48), log, 0.1).unwrap();
            assert!((shaken.angle - mid.angle).abs() > 1e-4);
        }
    );

    #[test]
    fn the_swing_carries_its_plane() {
        let mut c = Chopping {
            on: true,
            sweep: true,
            ..Default::default()
        };
        assert!(c.toggle());
        let m = c.start(axe(), [1.0, 0.0, 0.0], None, 0.0);
        assert!(matches!(
            m,
            Some(Some(ChopAction::Sweep { vertical: true, .. }))
        ));
        assert!(c.run.unwrap().vertical);
    }

    #[test]
    fn a_swing_through_only_leaves_rustles_once_and_follows_through_with_no_bounce() {
        // face-a-only: timing of the swing's run, no world.
        assert!(axe().clears_block("dwell:leaves") && axe().clears_block("dwell:snowy_leaves"));
        assert!(axe().swing.rustle.is_some(), "a rustle, not the clank");
        let mut c = Chopping {
            on: true,
            sweep: true,
            ..Default::default()
        };
        c.start(axe(), [1.0, 0.0, 0.0], None, 0.0).unwrap();
        let cells = [
            dwell_world::swing::Cleared {
                cell: [2, 0, 0],
                step: 4,
            },
            dwell_world::swing::Cleared {
                cell: [2, 1, 0],
                step: 6,
            },
        ];
        c.set_cleared(&cells);
        assert_eq!(c.cleared, vec![[2, 0, 0], [2, 1, 0]]);
        let mut run = c.run.unwrap();
        let at = axe().swing.time_of_step(4) * 1000.0;
        assert!(!run.take_clear(at - 1.0), "not before the head gets there");
        assert!(run.take_clear(at + 1.0));
        assert!(!run.take_clear(at + 2.0), "once");
        // Not a hit: no impact (no hit-stop, no clank), a miss's follow-through.
        assert_eq!(run.take_impact(at + 500.0), None);
        assert_eq!(run.length(), axe().swing.miss_length());
        // A swing that clears nothing has no clear moment.
        let mut plain = SwingRun::new(axe(), false, 0.0, Some(impact(5, true)));
        assert!(!plain.take_clear(1e6));
    }
}

#[cfg(test)]
mod sway_tests {
    //! The view's sway with the swing (docs/CHARACTERS.md §5.2; owner, 2026-11-12).
    use super::*;
    use dwell_world::FaceFrame;

    fn axe() -> &'static ToolDef {
        dwell_protocol::chopping::tool("dwell:stone_axe").unwrap()
    }

    fn hit_at(step: u32, wood: bool) -> Impact {
        Impact {
            cell: [3, 0, 0],
            face: 0,
            point: [3.0, 0.2, 0.5],
            step,
            wood,
        }
    }

    /// Every run of both swing kinds: a miss, a hit into wood, a bounce.
    fn runs() -> Vec<SwingRun> {
        let mid = axe().swing.samples / 2;
        [false, true]
            .into_iter()
            .flat_map(|v| {
                [None, Some(hit_at(mid, true)), Some(hit_at(mid, false))]
                    .map(|h| SwingRun::new(axe(), v, 0.0, h))
            })
            .collect()
    }

    #[test]
    fn the_sway_is_zero_at_rest_moves_during_the_swing_and_is_back_to_zero_after() {
        for run in runs() {
            let len = run.length();
            for t in [-0.2, 0.0, len, len + 0.01, len + 2.0] {
                assert_eq!(run.camera(t), CameraOffset::ZERO, "{run:?} at {t}");
            }
            // It settles: just before the end it is nearly home (no snap back).
            assert!(run.camera(len - 1e-3).size() < 0.05, "{run:?}");
            // And it sways: wound up at the wind-up's end by more than half a degree.
            let wound = run.camera(run.tool.swing.windup_s);
            assert!(wound.size() > 0.5, "{run:?}: {wound:?}");
            let peak = (0..200)
                .map(|i| run.camera(len * f64::from(i) / 200.0).size())
                .fold(0.0, f64::max);
            assert!(peak < 6.0, "small: {peak}");
        }
    }

    #[test]
    fn the_level_strike_turns_the_view_left_the_way_the_swing_goes_after_a_lean_right() {
        let s = axe().swing;
        let miss = SwingRun::new(axe(), false, 0.0, None);
        let wound = miss.camera(s.windup_s);
        let struck = miss.camera(s.windup_s + s.active_s);
        // The wind-up leans back and right (the head drawn back over the right shoulder).
        assert!(wound.yaw > 0.5 && wound.roll > 0.5, "{wound:?}");
        assert!(
            wound.bob[0] > 0.0 && wound.bob[2] > 0.0,
            "back and right: {wound:?}"
        );
        // The strike turns it left, as the head goes right → left across the screen.
        assert!(struck.yaw < -0.5, "{struck:?}");
        let (mut last, mut leftward) = (wound.yaw, 0);
        for i in 1..=10 {
            let y = miss
                .camera(s.windup_s + s.active_s * f64::from(i) / 10.0)
                .yaw;
            leftward += usize::from(y < last);
            last = y;
        }
        assert!(
            leftward >= 8,
            "the yaw falls through the strike ({leftward}/10)"
        );
    }

    #[test]
    fn the_vertical_chop_pitches_up_on_the_wind_up_and_dips_at_contact() {
        let s = axe().swing;
        let mid = s.samples / 2;
        let hit = SwingRun::new(axe(), true, 0.0, Some(hit_at(mid, true)));
        let at = hit.impact_s().unwrap();
        assert!(hit.camera(s.windup_s).pitch > 1.0);
        let before = hit.camera(at - 1e-3).pitch;
        let after = hit.camera(at + 0.02).pitch;
        assert!(
            after < before - 0.5,
            "a dip at contact: {before} then {after}"
        );
        assert!(after < 0.0);
    }

    #[test]
    fn a_hit_kicks_harder_than_a_miss_and_a_bounce_harder_than_wood() {
        let s = axe().swing;
        let mid = s.samples / 2;
        for vertical in [false, true] {
            // The kick's jump across the contact moment.
            let jump = |run: SwingRun, at: f64| {
                let a = run.camera(at - 1e-4);
                let b = run.camera(at + 1e-4);
                CameraOffset {
                    yaw: b.yaw - a.yaw,
                    pitch: b.pitch - a.pitch,
                    roll: b.roll - a.roll,
                    bob: std::array::from_fn(|k| b.bob[k] - a.bob[k]),
                }
                .size()
            };
            let miss = jump(SwingRun::new(axe(), vertical, 0.0, None), s.centre_s());
            let contact = s.time_of_step(mid);
            let wood = jump(
                SwingRun::new(axe(), vertical, 0.0, Some(hit_at(mid, true))),
                contact,
            );
            let stone = jump(
                SwingRun::new(axe(), vertical, 0.0, Some(hit_at(mid, false))),
                contact,
            );
            assert!(miss > 0.1, "a miss sways (and kicks) too: {miss}");
            assert!(wood > miss, "a hit kicks harder: {wood} vs {miss}");
            assert!(stone > wood, "a bounce harder still: {stone} vs {wood}");
        }
    }

    fn pose(f: FaceFrame) -> ViewPose {
        ViewPose {
            eye: f.point([0.98, dwell_world::m_of_row(35) + 0.2, 0.5]),
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
        }
    }

    #[test]
    fn with_the_setting_off_the_view_is_the_aim_plus_a_blasts_shake() {
        let f = FaceFrame::A;
        let run = SwingRun::new(axe(), false, 0.0, None);
        let sway = run.camera(axe().swing.windup_s);
        let aim = pose(f);
        let off = swayed_view(aim, sway, false, (0.3, -0.2), f);
        assert_eq!(
            off,
            ViewPose {
                yaw: 0.3,
                pitch: -0.2,
                ..aim
            }
        );
        assert_ne!(swayed_view(aim, sway, true, (0.3, -0.2), f), off);
        // No swing: the view is the aim.
        assert_eq!(
            swayed_view(aim, CameraOffset::ZERO, true, (0.0, 0.0), f),
            aim
        );
    }

    dwell_world::both_faces!(
        /// The sway is face-local: a yaw toward the screen's right turns the forward toward the
        /// view's right, a roll leaning right tips its top toward the right, a bob right moves the
        /// eye to the right, on either face.
        fn the_sway_goes_the_same_way_on_screen_on_either_face(f: FaceFrame) {
            use crate::render::camera::{aim, dot, sub};
            let p = pose(f);
            let rest = aim(p.yaw, p.pitch, f, p.roll);
            let turn = |o: CameraOffset| {
                let v = swayed_view(p, o, true, (0.0, 0.0), f);
                (v, aim(v.yaw, v.pitch, f, v.roll))
            };
            let one = |yaw, roll, bob| CameraOffset {
                yaw,
                pitch: 0.0,
                roll,
                bob,
            };
            let (_, b) = turn(one(3.0, 0.0, [0.0; 3]));
            assert!(dot(b.forward(), rest.right) > 0.03, "yaw right");
            let (_, b) = turn(one(0.0, 3.0, [0.0; 3]));
            assert!(dot(b.up, rest.right) > 0.03, "lean right");
            assert!(
                dot(b.forward(), rest.forward()) > 0.9999,
                "a roll keeps the look"
            );
            let (v, _) = turn(one(0.0, 0.0, [0.02, 0.0, 0.0]));
            assert!(
                (dot(sub(v.eye, p.eye), rest.right) - 0.02).abs() < 1e-9,
                "bob right"
            );
        }
    );

    dwell_world::both_faces!(
        /// The sway turns the view only: through a swing the block the aim picks (what targeting,
        /// the crosshair, the server's swing and the facing use) is the same with the sway on and
        /// off, while the swayed view itself would have picked another cell at some moment (so the
        /// fixture can tell).
        fn the_sway_never_changes_the_aim(f: FaceFrame) {
            let mut w = dwell_world::VoxelWorld::new(dwell_world::empty_generator(), None);
            let log = dwell_blocks::require("dwell:log");
            for x in -3..4 {
                for row in 28..44 {
                    let [cx, cy, cz] = f.cell([x, row, 3]);
                    w.set_voxel(cx, cy, cz, log);
                }
            }
            let mut targeting = dwell_game::block_interaction::BlockInteraction::new();
            let aim = pose(f);
            let mut view_moved = false;
            for run in runs() {
                for i in 0..60 {
                    let t = run.length() * f64::from(i) / 60.0;
                    let sway = run.camera(t);
                    let mut picked = Vec::new();
                    for on in [false, true] {
                        let view = swayed_view(aim, sway, on, (0.0, 0.0), f);
                        // The game targets from the aim; the view only goes to the renderer.
                        let _ = view;
                        let t = targeting.update(&mut w, Some(aim.eye), aim.yaw, aim.pitch, f);
                        picked.push(t.map(|t| t.cell));
                    }
                    assert_eq!(picked[0], picked[1]);
                    assert_eq!(picked[0], Some(f.cell([0, 35, 3])));
                    let v = swayed_view(aim, sway, true, (0.0, 0.0), f);
                    let seen = targeting.update(&mut w, Some(v.eye), v.yaw, v.pitch, f);
                    view_moved |= seen.map(|t| t.cell) != picked[0];
                }
            }
            assert!(
                view_moved,
                "the swayed view crosses a cell edge at some moment"
            );
        }
    );
}
