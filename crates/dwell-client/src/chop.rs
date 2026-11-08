//! The client's side of chopping (docs/adr/0067-stone-axe-and-chopping.md): the held axe's swing
//! direction (Use toggles it), the swing itself as weighty first-person melee (owner, 2026-11-08: a
//! short wind-up, a committed arc whose path is what hits, a hit-stop on impact, a looser
//! follow-through and a longer recovery on a miss: [`SwingRun`]), and the server's cuts kept for
//! drawing. A swing can start at any time, at nothing (a free swing); the server sweeps the same
//! arc (`ChopAction::Sweep`) and cuts what it meets.

use dwell_protocol::chopping::{Cut, ToolDef};
use dwell_protocol::{ChopAction, CutsMessage};
use dwell_world::{CELL_H, m_of_row};
use std::collections::HashMap;

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
        }
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

    /// Where in the arms' swing clip (whose length is a miss's: wind-up, arc, follow-through,
    /// recovery) the arms are `t` s in: a hit holds the impact's pose through the hit-stop, then
    /// plays the clip back from it to the rest (the axe pulled out of the cut, or bounced back).
    pub fn clip_time(&self, t: f64) -> f64 {
        let s = &self.tool.swing;
        let Some(at) = self.impact_s() else {
            return t.min(s.miss_length());
        };
        if t < at {
            t
        } else if t < at + s.hit_stop_s {
            at
        } else {
            let u = ((t - at - s.hit_stop_s) / s.recover_hit_s.max(1e-6)).clamp(0.0, 1.0);
            at * (1.0 - ease(u))
        }
    }

    /// The camera's motion selling the swing (degrees: to the screen's right, up), `t` s in: drawn
    /// back with the wind-up, swept through the arc, a jolt against the swing at a hit (bigger
    /// off what it can't bite), a looser overshoot on a miss, then home.
    pub fn camera(&self, t: f64) -> (f64, f64) {
        let s = &self.tool.swing;
        let a = 0.6 * s.kick_deg.max(0.0) + 0.4;
        // Along the swing: + is where it winds up (the right, or up), − where it goes.
        let along = |v: f64| {
            if self.vertical {
                (0.0, v)
            } else {
                (v, 0.15 * v)
            }
        };
        let arc_end = s.windup_s + s.active_s;
        match self.impact_s() {
            Some(at) if t >= at => {
                let u = ((at - s.windup_s) / s.active_s.max(1e-6)).clamp(0.0, 1.0);
                let held = a * (1.0 - 2.0 * u);
                let after = t - at;
                let bite = if self.hit.is_some_and(|h| h.wood) {
                    1.0
                } else {
                    1.6
                };
                let kick = s.kick_deg * bite * (-after / (s.hit_stop_s + 0.06).max(1e-3)).exp();
                let home = 1.0 - ease((after - s.hit_stop_s) / s.recover_hit_s.max(1e-6));
                along((held + kick) * home)
            }
            _ if t < s.windup_s => along(a * ease(t / s.windup_s.max(1e-6))),
            _ if t < arc_end => {
                let u = (t - s.windup_s) / s.active_s.max(1e-6);
                along(a * (1.0 - 2.0 * u))
            }
            _ => {
                // The miss's overshoot, then home over the recovery.
                let u = ((t - arc_end) / s.recover_miss_s.max(1e-6)).clamp(0.0, 1.0);
                let over = -a * (1.0 + 0.4 * (std::f64::consts::PI * u).sin());
                along(over * (1.0 - ease(u)))
            }
        }
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

    pub fn apply(&mut self, m: CutsMessage) {
        for c in m.gone {
            self.cuts.remove(&c);
        }
        self.cuts.extend(m.cuts);
    }

    /// The hint shown while the axe is held.
    pub fn hint(&self) -> &'static str {
        if self.vertical {
            "Axe: vertical swing (right click: level)"
        } else {
            "Axe: level swing (right click: vertical)"
        }
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
        assert!(hit.clip_time(at + s.hit_stop_s + 0.5 * s.recover_hit_s) < at);
        assert!(hit.clip_time(hit.length()) < 1e-9);
        assert!(miss.clip_time(at + s.hit_stop_s) > at + s.hit_stop_s - 1e-9);
        assert!(hit.length() < miss.length(), "a miss recovers longer");
        // The impact fires once, at its moment.
        assert_eq!(hit.take_impact(at * 1000.0 - 1.0), None);
        assert_eq!(hit.take_impact(at * 1000.0 + 1.0), Some(impact(4, true)));
        assert_eq!(hit.take_impact(at * 1000.0 + 2.0), None);
        // The camera: drawn back with the wind-up, a jolt at the hit, home at the end.
        let (wound, _) = hit.camera(s.windup_s);
        assert!(wound > 0.0, "wound back to the right");
        let (before, _) = hit.camera(at - 1e-3);
        let (jolt, _) = hit.camera(at + 1e-3);
        assert!(
            jolt - before > 0.5 * s.kick_deg,
            "a jolt against the swing: {before} then {jolt}"
        );
        let (home, _) = hit.camera(hit.length());
        assert!(home.abs() < 0.05, "home: {home}");
    }

    #[test]
    fn a_swing_off_stone_bounces_with_a_bigger_jolt_than_into_wood() {
        let s = axe().swing;
        let wood = SwingRun::new(axe(), true, 0.0, Some(impact(5, true)));
        let stone = SwingRun::new(axe(), true, 0.0, Some(impact(5, false)));
        let at = wood.impact_s().unwrap();
        let (_, w) = wood.camera(at + 1e-3);
        let (_, st) = stone.camera(at + 1e-3);
        assert!(st > w, "the bounce jolts harder: {st} vs {w}");
        // Both stop and come back alike: a bounce is a hit with no effect.
        assert_eq!(stone.clip_time(at + 0.5 * s.hit_stop_s), at);
        assert_eq!(stone.length(), wood.length());
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
}
