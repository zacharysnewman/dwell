//! The listener's surroundings for the ambience beds (docs/AUDIO.md §2c, ADR 0077): a coarse look
//! round the camera — how high it is above the sea and the ground, how open the sky is, how near
//! and how wide the water is and how fast it runs, how many trees stand round it — handed to the
//! TS shell, whose data (shared/sounds `ambience`) turns them into each bed's level. A side
//! channel, like the sound events: nothing here feeds the simulation. Face-local throughout: the
//! columns are read along the face's up through [`FaceFrame`].

use dwell_blocks::{StateId, info};
use dwell_world::FaceFrame;

use crate::sound::ambient;

/// The surroundings the beds read, in the order the shell decodes them (`ambience.ts` `INPUTS`).
pub const INPUTS: [&str; 9] = [
    "altitude",
    "height",
    "open",
    "waterShare",
    "waterM",
    "flow",
    "trees",
    "day",
    "fireM",
];

/// A distance with nothing found (m).
pub const FAR_M: f64 = 99.0;
/// Columns read each side of the listener (a (2N+1)² grid over the probe's radius).
const SIDE: i32 = 4;
/// How far above the eye a column is read (a roof, a canopy) and how far below it (m).
const ABOVE_M: f64 = 8.0;
const BELOW_M: f64 = 40.0;

/// The listener's surroundings.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Listening {
    /// Metres above the sea (face-local).
    pub altitude: f64,
    /// Metres above the ground under the listener (`BELOW_M` with none).
    pub height: f64,
    /// Share of the columns open to the sky above the eye (0–1).
    pub open: f64,
    /// Share of the columns whose top is water (0–1): wide water is the sea or a lake.
    pub water_share: f64,
    /// Metres to the nearest water column (`FAR_M` with none).
    pub water_m: f64,
    /// The fastest current over the water near the listener (m/s).
    pub flow: f64,
    /// Share of the columns whose top is a tree's block (0–1).
    pub trees: f64,
    /// Daylight on the listener's face (0 night, 1 day).
    pub day: f64,
    /// Metres to the nearest fire (`FAR_M` with none).
    pub fire_m: f64,
}

impl Listening {
    /// In [`INPUTS`] order.
    pub fn to_array(&self) -> [f64; 9] {
        [
            self.altitude,
            self.height,
            self.open,
            self.water_share,
            self.water_m,
            self.flow,
            self.trees,
            self.day,
            self.fire_m,
        ]
    }
}

/// The blocks that count as trees (shared/sounds `ambience.probe.trees`), as block indices of this
/// build's registry; names it does not know are skipped.
pub fn tree_blocks() -> Vec<usize> {
    ambient::AMBIENCE_TREES
        .iter()
        .filter_map(|n| dwell_blocks::find_block(n))
        .map(|b| info(b.first_state).block)
        .collect()
}

/// Reads the surroundings round `eye` (world) on `face`: a grid of columns over `radius` m, each
/// read from `ABOVE_M` over the eye down to its first block. `voxel` is a world cell's state,
/// `is_tree` whether a state is a tree's, `current` the water's speed at a world point (m/s).
/// `day` and `fire_m` come from elsewhere (the clock, the fires) and are left 0 and `FAR_M`.
pub fn probe(
    eye: [f64; 3],
    face: FaceFrame,
    radius: f64,
    mut voxel: impl FnMut([i32; 3]) -> StateId,
    is_tree: impl Fn(StateId) -> bool,
    mut current: impl FnMut([f64; 3]) -> f64,
) -> Listening {
    let step = radius / f64::from(SIDE);
    let ey = face.y(eye[1]);
    let cell_h = dwell_world::CELL_H;
    let rows = ((ABOVE_M + BELOW_M) / cell_h) as i32;
    let (mut open, mut water, mut trees, mut n) = (0u32, 0u32, 0u32, 0u32);
    let (mut water_m, mut flow, mut height) = (FAR_M, 0.0f64, BELOW_M);
    for i in -SIDE..=SIDE {
        for j in -SIDE..=SIDE {
            let (dx, dz) = (f64::from(i) * step, f64::from(j) * step);
            let (x, z) = (eye[0] + dx, eye[2] + dz);
            n += 1;
            let mut top = None;
            for r in 0..rows {
                let h = ey + ABOVE_M - f64::from(r) * cell_h;
                let id = voxel(dwell_world::cell_of_pos([x, face.y(h), z]));
                if id != 0 {
                    top = Some((h, id));
                    break;
                }
            }
            let Some((h, id)) = top else {
                open += 1;
                continue;
            };
            if h <= ey {
                open += 1;
            }
            if i == 0 && j == 0 && h <= ey {
                height = ey - h;
            }
            let wet = info(id).liquid || info(id).flooded;
            if wet {
                water += 1;
                water_m = water_m.min(dx.hypot(dz).max((ey - h).max(0.0)));
                flow = flow.max(current([x, face.y(h), z]));
            } else if is_tree(id) {
                trees += 1;
            }
        }
    }
    let share = |k: u32| f64::from(k) / f64::from(n.max(1));
    Listening {
        altitude: ey - dwell_world::SEA_LEVEL_M,
        height,
        open: share(open),
        water_share: share(water),
        water_m,
        flow,
        trees: share(trees),
        day: 0.0,
        fire_m: FAR_M,
    }
}

/// Daylight on a face from the sun's direction (world, unit) and the face's up: 0 with the sun
/// below the horizon, 1 with it 15° up or more.
pub fn daylight(sun: [f64; 3], face: FaceFrame) -> f64 {
    let up = face.up();
    let s = sun[0] * up[0] + sun[1] * up[1] + sun[2] * up[2];
    (s / 0.26).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_blocks::parse_state;
    use dwell_world::both_faces;

    fn st(n: &str) -> StateId {
        parse_state(n).unwrap()
    }

    /// A world from a face-local column rule: `top(x, z)` is the face-local height of the ground's
    /// top and its block; air above, stone below.
    fn world(
        f: FaceFrame,
        top: impl Fn(f64, f64) -> (f64, StateId),
    ) -> impl FnMut([i32; 3]) -> StateId {
        let stone = st("dwell:stone");
        move |c| {
            let lo = dwell_world::pos_of_cell(c);
            let (x, z) = (lo[0] + 0.5, lo[2] + 0.5);
            let y = f.y(lo[1] + 0.5 * dwell_world::CELL_H);
            let (h, b) = top(x, z);
            if y > h {
                0
            } else if y > h - 1.0 {
                b
            } else {
                stone
            }
        }
    }

    fn listen(f: FaceFrame, eye_y: f64, top: impl Fn(f64, f64) -> (f64, StateId)) -> Listening {
        let trees = tree_blocks();
        probe(
            [0.5, f.y(eye_y), 0.5],
            f,
            16.0,
            world(f, top),
            |id| trees.contains(&info(id).block),
            |_| 1.5,
        )
    }

    both_faces!(
        fn a_mountain_top_is_high_and_open_and_a_forest_floor_is_treed_and_closed(f: FaceFrame) {
            let sea = dwell_world::SEA_LEVEL_M;
            let grass = st("dwell:grass");
            let high = listen(f, sea + 81.6, |_, _| (sea + 80.0, grass));
            assert!(high.altitude > 80.0, "{high:?}");
            assert!((high.height - 1.5).abs() < 0.6, "{high:?}");
            assert_eq!(high.open, 1.0);
            assert_eq!(high.water_share, 0.0);
            assert_eq!(high.water_m, FAR_M);
            let leaves = st("dwell:leaves");
            // A canopy 4 m up over every other column.
            let forest = listen(f, sea + 11.6, |x, z| {
                if (x.floor() as i32 + z.floor() as i32) % 8 == 0 {
                    (sea + 14.0, leaves)
                } else {
                    (sea + 10.0, grass)
                }
            });
            assert!(forest.trees > 0.1, "{forest:?}");
            assert!(forest.open < 1.0, "{forest:?}");
            assert!(forest.altitude < high.altitude);
        }
    );

    both_faces!(
        fn the_coast_reads_wide_water_near_and_a_stream_narrow_water_running(f: FaceFrame) {
            let sea = dwell_world::SEA_LEVEL_M;
            let (water, sand) = (st("dwell:water"), st("dwell:sand"));
            // The sea from x > 2.
            let coast = listen(f, sea + 1.6, |x, _| {
                if x > 2.0 {
                    (sea, water)
                } else {
                    (sea + 0.5, sand)
                }
            });
            assert!(coast.water_share > 0.3, "{coast:?}");
            assert!(coast.water_m < 6.0, "{coast:?}");
            assert_eq!(coast.flow, 1.5);
            // A 1 m stream along z at x = 4.
            let grass = st("dwell:grass");
            let stream = listen(f, sea + 11.6, |x, _| {
                if (4.0..5.0).contains(&x) {
                    (sea + 9.5, water)
                } else {
                    (sea + 10.0, grass)
                }
            });
            assert!(stream.water_share < 0.15, "{stream:?}");
            assert!(stream.water_m < 6.0, "{stream:?}");
        }
    );

    both_faces!(
        fn daylight_follows_the_sun_over_the_face(f: FaceFrame) {
            let up = f.up();
            assert_eq!(daylight(up, f), 1.0);
            assert_eq!(daylight(up.map(|v| -v), f), 0.0);
        }
    );

    #[test]
    fn inputs_are_the_shells_order() {
        // face-a-only: a fixed list.
        let l = Listening {
            altitude: 1.0,
            height: 2.0,
            open: 3.0,
            water_share: 4.0,
            water_m: 5.0,
            flow: 6.0,
            trees: 7.0,
            day: 8.0,
            fire_m: 9.0,
        };
        assert_eq!(l.to_array(), [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]);
        assert_eq!(INPUTS.len(), 9);
    }
}
