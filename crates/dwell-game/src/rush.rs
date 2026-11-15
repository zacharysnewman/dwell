//! The grid's moving water heard (BUGS.md #176; docs/AUDIO.md §5): a flood, a river in spate, a
//! burst dam. The ambience beds hear only the water within the probe's few metres; this reads the
//! current table (`VoxelWorld::currents`, docs/WATER_SURFACE_GRID.md §2c) out to `RUSH_RADIUS_M`,
//! cheaply — every `RUSH_STRIDE`-th column — each moving column adding its speed × depth × the area
//! it stands for, weighted by its distance, and each waterfall its discharge. The sum drives one
//! rushing loop (its loudness and brightness: the sound's intensity), placed toward where the flow
//! is, and a flood's burst once when much water starts moving at once. Still water adds nothing.
//! The tunables are data (shared/sounds `ambient.rush`). Face-local: a column's water is read in
//! its face's frame (`dwell_world::falls::column`); the currents are the same vector on both faces.

use std::collections::VecDeque;

use dwell_world::currents::{CS, CurrentKey};
use dwell_world::{CHUNK_SIZE, FaceFrame, VoxelWorld, falls, row_of_m};

use crate::sound::{LoopTrack, SoundEvent, SoundKind, ambient};

/// The rushing loop's voice: past the waterfalls'.
pub const RUSH_VOICE: u32 = 0x4_0000;
/// Further than this round the radius the moving chunks are still remembered, so one coming into
/// the radius as the listener moves is not heard as water starting to move.
const MARGIN_M: f64 = 32.0;

/// A moving column's weight at `d` m from the listener: full beside it, halved at `RUSH_NEAR_M`,
/// faded smoothly to nothing at `RUSH_RADIUS_M`.
pub fn weight(d: f64) -> f64 {
    let x = (d / ambient::RUSH_RADIUS_M).clamp(0.0, 1.0);
    (1.0 - x * x).powi(2) / (1.0 + (d / ambient::RUSH_NEAR_M).powi(2))
}

/// The level (0–1) of a weighted flux: the square root of its share of `RUSH_FULL_FLUX`.
pub fn level(flux: f64) -> f64 {
    (flux / ambient::RUSH_FULL_FLUX).clamp(0.0, 1.0).sqrt()
}

/// What the listener hears of the moving water round it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Heard {
    /// The weighted flux (Σ speed × depth × area × weight, and the falls').
    pub flux: f64,
    /// Where it comes from: the flux-weighted middle of what moves (world m), none with nothing.
    pub from: Option<[f64; 3]>,
    /// Each moving water chunk's flux, unweighted, and its weight's share (weighted / unweighted):
    /// what the burst compares a moment later.
    pub chunks: Vec<(CurrentKey, f64, f64)>,
    /// The world's chunk columns (x, z) loaded within reach, sorted: water whose land arrives
    /// later streamed in (a join, a teleport), it did not start to move.
    pub land: Vec<(i32, i32)>,
}

impl Heard {
    /// The level of the loop (0–1).
    pub fn level(&self) -> f64 {
        level(self.flux)
    }
}

/// The moving water round world point `eye` in `world`: every `RUSH_STRIDE`-th column of the
/// current table within `RUSH_RADIUS_M` (and the margin, unweighted) faster than
/// `RUSH_MIN_SPEED_MS`, its depth from the world's water there. `falls` are the waterfalls near the
/// listener: where each sounds (world m) and its discharge (m³/s).
pub fn hear(world: &VoxelWorld, eye: [f64; 3], falls: &[([f64; 3], f64)]) -> Heard {
    let stride = ambient::RUSH_STRIDE.max(1);
    let area = f64::from(stride * stride);
    let reach = ambient::RUSH_RADIUS_M + MARGIN_M;
    let mut keys: Vec<CurrentKey> = world.currents.keys().collect();
    keys.sort_unstable();
    let mut heard = Heard::default();
    let mut mid = [0.0f64; 3];
    let add = |p: [f64; 3], f: f64, mid: &mut [f64; 3], flux: &mut f64| {
        for k in 0..3 {
            mid[k] += p[k] * f;
        }
        *flux += f;
    };
    for key in keys {
        let (b, cx, cz) = key;
        let f = if b { FaceFrame::B } else { FaceFrame::A };
        let (x0, z0) = (cx * CS, cz * CS);
        let half = f64::from(CS) / 2.0;
        let (mx, mz) = (f64::from(x0) + half, f64::from(z0) + half);
        if (mx - eye[0]).hypot(mz - eye[2]) > reach + half * std::f64::consts::SQRT_2 {
            continue;
        }
        let Some(cells) = world.currents.get(key) else {
            continue;
        };
        let local_eye = f.y(eye[1]);
        let mut near = row_of_m(local_eye);
        let mut rows = (reach / dwell_world::CELL_H).ceil() as i32;
        let (mut raw, mut weighted) = (0.0, 0.0);
        for dz in (0..CS).step_by(stride as usize) {
            for dx in (0..CS).step_by(stride as usize) {
                let c = cells[(dx + CS * dz) as usize];
                let speed = f64::from(c[0].hypot(c[1]));
                if speed < ambient::RUSH_MIN_SPEED_MS {
                    continue;
                }
                let (x, z) = (x0 + dx, z0 + dz);
                let Some(col) = falls::column(world, f, x, z, near, rows) else {
                    continue;
                };
                near = col.top;
                rows = 8;
                let q = speed * (col.level - col.floor) * area;
                let p = f.point([f64::from(x) + 0.5, col.level, f64::from(z) + 0.5]);
                let d = (0..3).map(|k| (p[k] - eye[k]).powi(2)).sum::<f64>().sqrt();
                raw += q;
                let w = q * weight(d);
                weighted += w;
                if w > 0.0 {
                    add(p, w, &mut mid, &mut heard.flux);
                }
            }
        }
        if raw > 0.0 {
            heard.chunks.push((key, raw, weighted / raw));
        }
    }
    for &(p, discharge) in falls {
        let d = (0..3).map(|k| (p[k] - eye[k]).powi(2)).sum::<f64>().sqrt();
        let w = discharge * ambient::RUSH_FALL_WEIGHT * weight(d);
        if w > 0.0 {
            add(p, w, &mut mid, &mut heard.flux);
        }
    }
    if heard.flux > 0.0 {
        heard.from = Some(mid.map(|v| v / heard.flux));
    }
    let side = f64::from(CHUNK_SIZE);
    let mut land: Vec<(i32, i32)> = world
        .chunks()
        .map(|(c, _)| (c.x, c.z))
        .filter(|&(x, z)| {
            let mid = |v: i32| (f64::from(v) + 0.5) * side;
            (mid(x) - eye[0]).hypot(mid(z) - eye[2]) <= reach + side
        })
        .collect();
    land.sort_unstable();
    land.dedup();
    heard.land = land;
    heard
}

/// A water chunk's land: the world chunk column holding its first column.
fn land_of((_, cx, cz): CurrentKey) -> (i32, i32) {
    (
        (cx * CS).div_euclid(CHUNK_SIZE),
        (cz * CS).div_euclid(CHUNK_SIZE),
    )
}

/// Where the loop sounds: `RUSH_PAN_M` from the listener toward `from` (the shell's distance
/// rolloff stays at its reference, so the level here is the whole of the distance's effect), or
/// under the listener (along `down`) when the water is all round it.
pub fn placed(listener: [f64; 3], from: [f64; 3], down: [f64; 3]) -> [f64; 3] {
    let d: [f64; 3] = std::array::from_fn(|k| from[k] - listener[k]);
    let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    let dir = if n < 1.0 { down } else { d.map(|v| v / n) };
    std::array::from_fn(|k| listener[k] + dir[k] * ambient::RUSH_PAN_M)
}

/// One hearing remembered: its time (s), what each moving chunk carried (unweighted) and where
/// the listener was.
type Hearing = (f64, Vec<(CurrentKey, f64)>, [f64; 3], Vec<(i32, i32)>);

/// The rushing loop and the flood's burst, from what is heard now and then.
#[derive(Clone, Debug)]
pub struct RushSounds {
    track: LoopTrack,
    /// The recent hearings, oldest first.
    history: VecDeque<Hearing>,
    t: f64,
    burst_at: Option<f64>,
}

impl Default for RushSounds {
    fn default() -> Self {
        Self {
            track: LoopTrack::new(
                SoundKind::Rush,
                SoundKind::RushLevel,
                SoundKind::RushEnd,
                RUSH_VOICE,
            ),
            history: VecDeque::new(),
            t: 0.0,
            burst_at: None,
        }
    }
}

impl RushSounds {
    pub fn new() -> Self {
        Self::default()
    }

    /// The weighted flux the water starting to move adds now: the flux of each chunk over what it
    /// carried `RUSH_BURST_WINDOW_S` ago (0 for a chunk then still), weighted as it is now. None
    /// while the history is shorter than the window, or the listener moved past the margin in it
    /// (a jump: what is near now was not near then).
    fn onset(&self, heard: &Heard, listener: [f64; 3]) -> Option<f64> {
        let (t0, then, at, land) = self.history.front()?;
        if self.t - t0 < ambient::RUSH_BURST_WINDOW_S * 0.999 {
            return None;
        }
        let moved = (0..3)
            .map(|k| (at[k] - listener[k]).powi(2))
            .sum::<f64>()
            .sqrt();
        if moved > MARGIN_M {
            return None;
        }
        let rise: f64 = heard
            .chunks
            .iter()
            .filter(|(key, _, _)| land.binary_search(&land_of(*key)).is_ok())
            .map(|(key, raw, share)| {
                let before = then.iter().find(|(k, _)| k == key).map_or(0.0, |(_, r)| *r);
                (raw - before).max(0.0) * share
            })
            .sum();
        Some(rise)
    }

    /// One hearing, `dt` s after the last: the loop's level and place, and the burst if the water
    /// near the listener just started to move. `down` is the listener's face's down.
    pub fn update(
        &mut self,
        listener: [f64; 3],
        down: [f64; 3],
        heard: &Heard,
        dt: f64,
        out: &mut Vec<SoundEvent>,
    ) {
        self.t += dt;
        let lvl = heard.level();
        let at = heard.from.map(|p| placed(listener, p, down));
        self.track.set(lvl, 0, at, dt, out);
        if let Some(rise) = self.onset(heard, listener) {
            let rested = self
                .burst_at
                .is_none_or(|b| self.t - b >= ambient::RUSH_BURST_COOLDOWN_S);
            let (lift, share) = (level(rise), rise / heard.flux.max(1e-9));
            if rested
                && lift >= ambient::RUSH_BURST_MIN_LEVEL
                && share >= ambient::RUSH_BURST_MIN_SHARE
            {
                let intensity = (lift / ambient::RUSH_BURST_FULL_LEVEL).min(1.0);
                out.push(SoundEvent::new(SoundKind::FloodBurst, 0, intensity, at));
                self.burst_at = Some(self.t);
            }
        }
        self.history.push_back((
            self.t,
            heard.chunks.iter().map(|(k, r, _)| (*k, *r)).collect(),
            listener,
            heard.land.clone(),
        ));
        while self
            .history
            .get(1)
            .is_some_and(|(t, ..)| self.t - t >= ambient::RUSH_BURST_WINDOW_S)
        {
            self.history.pop_front();
        }
    }

    /// Ends the loop (leaving the world) and forgets what was heard.
    pub fn finish(&mut self, out: &mut Vec<SoundEvent>) {
        self.track.end(out);
        self.history.clear();
    }

    /// Whether the loop is sounding.
    pub fn playing(&self) -> bool {
        self.track.playing
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_blocks::require;
    use dwell_world::currents::{CA, current_key};
    use dwell_world::{FLAT_TOP_Y, M, both_faces, empty_generator, m_of_row};

    const BASE: i32 = FLAT_TOP_Y + 40;

    /// A channel in face `f`'s frame: stone floor at row −1, water rows 0..`depth_rows` over x
    /// 0..`len`, z 0..`width`, its current `c` (m/s) along it.
    fn channel(f: FaceFrame, len: i32, width: i32, depth_rows: i32, c: [f32; 2]) -> VoxelWorld {
        let mut w = VoxelWorld::new(empty_generator(), None);
        let stone = require("dwell:stone");
        for x in -2..len + 2 {
            for z in -2..width + 2 {
                let [a, b, cc] = f.cell([x, BASE - 1, z]);
                w.set_voxel(a, b, cc, stone);
                for r in 0..depth_rows + 4 {
                    let inside = (0..len).contains(&x) && (0..width).contains(&z);
                    let m = if inside && r < depth_rows {
                        M.water
                    } else if inside {
                        M.air
                    } else {
                        stone
                    };
                    let [a, b, cc] = f.cell([x, BASE + r, z]);
                    w.set_voxel(a, b, cc, m);
                }
            }
        }
        for x in 0..len {
            for z in 0..width {
                let (key, i) = current_key(f, x, z);
                let mut cells = w
                    .currents
                    .get(key)
                    .cloned()
                    .unwrap_or_else(|| Box::new([[0.0; 2]; CA]));
                cells[i] = c;
                w.currents.set(key, Some(cells));
            }
        }
        w
    }

    /// A world point `h` m over the water's floor, at (x, z), on face `f`.
    fn over(f: FaceFrame, x: f64, h: f64, z: f64) -> [f64; 3] {
        f.point([x, m_of_row(BASE) + h, z])
    }

    both_faces!(
        /// Still water is silent: a deep lake with no current adds nothing, however near.
        fn still_water_is_silent(f: FaceFrame) {
            let w = channel(f, 64, 32, 8, [0.0, 0.0]);
            let h = hear(&w, over(f, 32.0, 6.0, 16.0), &[]);
            assert_eq!(h.flux, 0.0, "{h:?}");
            assert_eq!(h.level(), 0.0);
            assert!(h.from.is_none());
        }
    );

    both_faces!(
        /// Moving water near the listener is heard, louder for faster and deeper water, and from
        /// its side: a 64 × 16 m flood 2 m deep at 2 m/s, the listener over its bank.
        fn moving_water_near_the_listener_is_heard_from_its_side(f: FaceFrame) {
            let w = channel(f, 64, 16, 4, [2.0, 0.0]);
            let bank = over(f, 32.0, 4.0, -6.0);
            let h = hear(&w, bank, &[]);
            assert!(
                h.level() > 0.5,
                "a flood beside the listener is loud: {h:?}"
            );
            let from = h.from.unwrap();
            assert!(
                from[2] > bank[2] + 3.0,
                "heard toward the water (+z): {from:?}"
            );
            let slow = hear(&channel(f, 64, 16, 4, [0.5, 0.0]), bank, &[]);
            let shallow = hear(&channel(f, 64, 16, 1, [2.0, 0.0]), bank, &[]);
            assert!(slow.flux < h.flux * 0.3, "{} vs {}", slow.flux, h.flux);
            assert!(
                shallow.flux < h.flux * 0.3,
                "{} vs {}",
                shallow.flux,
                h.flux
            );
        }
    );

    both_faces!(
        /// The level falls off with distance and is still heard at 60–100 m from a big flood (the
        /// dam break's camera), and nothing past the radius.
        fn a_flood_falls_off_with_distance_and_is_heard_from_afar(f: FaceFrame) {
            let w = channel(f, 128, 16, 6, [2.5, 0.0]);
            let at = |d: f64| hear(&w, over(f, 64.0, 3.0 + d * 0.6, 8.0 - d * 0.8), &[]).level();
            let (near, mid, far, past) = (at(10.0), at(60.0), at(100.0), at(250.0));
            assert!(
                near > mid && mid > far && far > past,
                "{near} {mid} {far} {past}"
            );
            assert!(mid > 0.3, "a flood 60 m off is heard: {mid}");
            assert!(far > 0.12, "and 100 m off: {far}");
            assert_eq!(past, 0.0);
        }
    );

    both_faces!(
        /// A waterfall adds to it, more than its flow would standing still.
        fn a_waterfall_is_louder_than_the_flow_alone(f: FaceFrame) {
            let w = channel(f, 16, 4, 2, [1.0, 0.0]);
            let eye = over(f, 8.0, 6.0, -20.0);
            let plain = hear(&w, eye, &[]);
            let fall = hear(&w, eye, &[(over(f, 17.0, 0.0, 2.0), 4.0)]);
            assert!(
                fall.flux > plain.flux * 1.5,
                "{} vs {}",
                fall.flux,
                plain.flux
            );
        }
    );

    both_faces!(
        /// The loop starts with the flow, placed toward it, and the burst sounds once when the
        /// water starts to move all at once — not while it keeps moving, and not for still water.
        fn the_loop_follows_the_flow_and_the_burst_marks_its_onset(f: FaceFrame) {
            let still = channel(f, 128, 16, 8, [0.0, 0.0]);
            let moving = channel(f, 128, 16, 8, [3.0, 0.0]);
            let eye = over(f, 64.0, 40.0, -40.0);
            let down = f.vector([0.0, -1.0, 0.0]);
            let mut rush = RushSounds::new();
            let mut out = Vec::new();
            for _ in 0..12 {
                rush.update(eye, down, &hear(&still, eye, &[]), 0.25, &mut out);
            }
            assert!(out.is_empty(), "still water: {out:?}");
            assert!(!rush.playing());
            for _ in 0..40 {
                rush.update(eye, down, &hear(&moving, eye, &[]), 0.25, &mut out);
            }
            let kinds: Vec<SoundKind> = out.iter().map(|e| e.kind).collect();
            assert_eq!(
                kinds.iter().filter(|k| **k == SoundKind::Rush).count(),
                1,
                "{kinds:?}"
            );
            assert_eq!(
                kinds
                    .iter()
                    .filter(|k| **k == SoundKind::FloodBurst)
                    .count(),
                1,
                "one burst at the onset: {kinds:?}"
            );
            assert!(rush.playing());
            let at = out[0].at.unwrap();
            let r = (0..3).map(|k| (at[k] - eye[k]).powi(2)).sum::<f64>().sqrt();
            assert!((r - ambient::RUSH_PAN_M).abs() < 1e-6, "{at:?}");
            assert!(at[2] > eye[2], "placed toward the water: {at:?}");
            out.clear();
            for _ in 0..8 {
                rush.update(eye, down, &hear(&still, eye, &[]), 0.25, &mut out);
            }
            assert!(
                out.iter().any(|e| e.kind == SoundKind::RushEnd),
                "it stops with the flow: {out:?}"
            );
        }
    );

    both_faces!(
        /// Arriving (a teleport) where a river runs is no burst, though its water streams in after
        /// the listener arrives (its land was not loaded before); a dam going later is one.
        fn arriving_at_running_water_is_no_burst(f: FaceFrame) {
            let empty = VoxelWorld::new(empty_generator(), None);
            let moving = channel(f, 128, 16, 8, [3.0, 0.0]);
            let still = channel(f, 128, 16, 8, [0.0, 0.0]);
            let down = f.vector([0.0, -1.0, 0.0]);
            let mut rush = RushSounds::new();
            let mut out = Vec::new();
            let far = over(f, 3000.0, 4.0, 3000.0);
            let eye = over(f, 64.0, 4.0, -10.0);
            for _ in 0..60 {
                rush.update(far, down, &hear(&empty, far, &[]), 0.25, &mut out);
            }
            // Teleported: nothing loaded for a second, then the running river arrives.
            for i in 0..40 {
                let w = if i < 4 { &empty } else { &moving };
                rush.update(eye, down, &hear(w, eye, &[]), 0.25, &mut out);
            }
            assert!(rush.playing());
            assert!(
                !out.iter().any(|e| e.kind == SoundKind::FloodBurst),
                "{out:?}"
            );
            // Long after: the river stops (a dam upstream) and starts again at once.
            for _ in 0..20 {
                rush.update(eye, down, &hear(&still, eye, &[]), 0.25, &mut out);
            }
            for _ in 0..8 {
                rush.update(eye, down, &hear(&moving, eye, &[]), 0.25, &mut out);
            }
            assert_eq!(
                out.iter()
                    .filter(|e| e.kind == SoundKind::FloodBurst)
                    .count(),
                1,
                "{out:?}"
            );
        }
    );

    both_faces!(
        /// Coming up to a river that was already running is no burst: the chunks were moving when
        /// the listener was further off (inside the margin).
        fn walking_up_to_running_water_is_no_burst(f: FaceFrame) {
            let w = channel(f, 128, 16, 8, [3.0, 0.0]);
            let down = f.vector([0.0, -1.0, 0.0]);
            let mut rush = RushSounds::new();
            let mut out = Vec::new();
            for i in 0..80 {
                let eye = over(f, 64.0, 4.0, -140.0 + f64::from(i) * 1.6);
                rush.update(eye, down, &hear(&w, eye, &[]), 0.25, &mut out);
            }
            assert!(rush.playing());
            assert!(
                !out.iter().any(|e| e.kind == SoundKind::FloodBurst),
                "{out:?}"
            );
        }
    );
}
