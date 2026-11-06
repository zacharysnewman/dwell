//! Flecks drifting on moving water (docs/WATER_SURFACE_GRID.md §2c, Phase 53 G3d; the Drifting
//! flecks setting): small specks spawned on the surface of water whose current is at least the
//! liquid's `particle_from_ms` within `particle_radius_m` of the eye, carried by the current table
//! (`VoxelWorld::currents`, the same the swimmers and bodies read), kept on the surface, faded in
//! and out over their `particle_life_s`, per liquid as `shared/look/waterflow.json` says. The
//! renderer draws them (`render::flames::fleck_quads`). Face-local: a column's surface is found
//! toward the face's up, through `FaceFrame`.

use dwell_blocks::{FLUIDS, try_info};
use dwell_look::waterflow::{LIQUID_FLOWS, LiquidFlowLook};
use dwell_world::{CELL_H, FaceFrame, VoxelWorld, m_of_row};

/// A fleck: where it is (world m), how old (s), its liquid's look (index in `LIQUID_FLOWS`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fleck {
    pub at: [f64; 3],
    pub age: f64,
    pub look: usize,
}

impl Fleck {
    /// Its opacity now: faded in over its first half second and out over its last second.
    pub fn alpha(&self) -> f32 {
        let life = LIQUID_FLOWS[self.look].particle_life_s;
        let a = (self.age / 0.5).min(1.0) * ((life - self.age) / 1.0).clamp(0.0, 1.0);
        a as f32
    }
}

/// The flecks near the eye.
pub struct Flecks {
    pub list: Vec<Fleck>,
    credit: f64,
    rng: u64,
}

impl Default for Flecks {
    fn default() -> Self {
        Self {
            list: Vec::new(),
            credit: 0.0,
            rng: 0x9e37_79b9_7f4a_7c15,
        }
    }
}

/// How many rows a column's surface is looked for above and below a point.
const SCAN_UP: i32 = 4;
const SCAN_DOWN: i32 = 12;

/// The drawn surface of the water in column (`x`, `z`) nearest under `near_y` (world m) on face
/// `f`, and its liquid's flow look: (world y, look index). None: no water there, or a liquid with
/// no flow look.
pub fn surface_at(world: &VoxelWorld, x: i32, z: i32, near_y: f64) -> Option<(f64, usize)> {
    let f = FaceFrame::at_height(near_y);
    let local = f.y(near_y);
    let row = (local / CELL_H).floor() as i32;
    let liquid = |r: i32| {
        let m = world.get_voxel_if_loaded(f.cell([x, r, z]));
        try_info(m).filter(|i| i.liquid)
    };
    for r in (row - SCAN_DOWN..=row + SCAN_UP).rev() {
        let Some(info) = liquid(r) else {
            continue;
        };
        if liquid(r + 1).is_some() {
            continue;
        }
        let fluid = &FLUIDS[usize::from(info.fluid?)];
        let look = LIQUID_FLOWS.iter().position(|l| l.fluid == fluid.name)?;
        let fill = if info.fill > 0.0 {
            f64::from(info.fill)
        } else {
            1.0
        };
        let top = m_of_row(r) + CELL_H * fill - fluid.surface_drop * CELL_H;
        return Some((f.y(top), look));
    }
    None
}

impl Flecks {
    fn random(&mut self) -> f64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 11) as f64 / (1u64 << 53) as f64
    }

    /// A step of `dt` seconds around `eye` (world m): the flecks carried by the current and kept on
    /// the surface, the old, stranded or far ones gone, new ones spawned on moving water; none
    /// while `on` is false.
    pub fn update(&mut self, world: &VoxelWorld, eye: [f64; 3], dt: f64, on: bool) {
        if !on {
            self.list.clear();
            self.credit = 0.0;
            return;
        }
        let speed = |c: [f32; 2]| f64::from(c[0].hypot(c[1]));
        self.list.retain_mut(|p| {
            let look: &LiquidFlowLook = &LIQUID_FLOWS[p.look];
            let c = world.currents.at(p.at);
            p.at[0] += f64::from(c[0]) * dt;
            p.at[2] += f64::from(c[1]) * dt;
            p.age += dt;
            let far = (p.at[0] - eye[0]).hypot(p.at[2] - eye[2]) > look.particle_radius_m * 1.5;
            if p.age >= look.particle_life_s || far || speed(c) < 0.5 * look.particle_from_ms {
                return false;
            }
            match surface_at(
                world,
                p.at[0].floor() as i32,
                p.at[2].floor() as i32,
                p.at[1],
            ) {
                Some((y, _)) if (y - p.at[1]).abs() < 1.0 => {
                    p.at[1] = y;
                    true
                }
                _ => false,
            }
        });
        // New ones: each liquid's count over its life a second, a few tries a frame.
        let Some(first) = LIQUID_FLOWS.first() else {
            return;
        };
        self.credit += f64::from(first.particle_count) / first.particle_life_s * dt;
        let mut tries = 0;
        while self.credit >= 1.0 && tries < 16 {
            tries += 1;
            let (a, r) = (self.random() * std::f64::consts::TAU, self.random().sqrt());
            let x = eye[0] + a.cos() * r * first.particle_radius_m;
            let z = eye[2] + a.sin() * r * first.particle_radius_m;
            let Some((y, look)) = surface_at(world, x.floor() as i32, z.floor() as i32, eye[1])
            else {
                continue;
            };
            let l = &LIQUID_FLOWS[look];
            let at = [x, y, z];
            let here = self.list.iter().filter(|p| p.look == look).count();
            if speed(world.currents.at(at)) < l.particle_from_ms
                || here >= l.particle_count as usize
            {
                continue;
            }
            self.credit -= 1.0;
            self.list.push(Fleck { at, age: 0.0, look });
        }
        // Still water all round: no backlog.
        self.credit = self.credit.min(4.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_blocks::require;
    use dwell_world::currents::{CA, current_key};

    /// A pool on face `f`: water rows 0 and 1 over stone, x −20..20, z −20..20, near y 0 local,
    /// its columns given a current `c` (m/s along x, z).
    fn pool(f: FaceFrame, c: [f32; 2]) -> VoxelWorld {
        let mut w = VoxelWorld::new(dwell_world::empty_generator(), None);
        let base = dwell_world::FLAT_TOP_Y + 40;
        for x in -24..24 {
            for z in -24..24 {
                {
                    let [a, b, c] = f.cell([x, base - 1, z]);
                    w.set_voxel(a, b, c, require("dwell:stone"));
                }
                {
                    let [a, b, c] = f.cell([x, base, z]);
                    w.set_voxel(a, b, c, require("dwell:water"));
                }
                {
                    let [a, b, c] = f.cell([x, base + 1, z]);
                    w.set_voxel(a, b, c, require("dwell:water"));
                }
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

    fn eye(f: FaceFrame) -> [f64; 3] {
        let base = dwell_world::FLAT_TOP_Y + 40;
        f.point([0.5, m_of_row(base + 2) + 1.5, 0.5])
    }

    dwell_world::both_faces!(
        fn flecks_spawn_on_moving_water_and_drift_with_it(f: FaceFrame) {
            let w = pool(f, [0.0, 1.0]);
            let mut fl = Flecks::default();
            for _ in 0..240 {
                fl.update(&w, eye(f), 1.0 / 60.0, true);
            }
            assert!(fl.list.len() > 10, "{} flecks", fl.list.len());
            // On the surface: the top cell's drawn level, face-locally.
            let base = dwell_world::FLAT_TOP_Y + 40;
            let drop = FLUIDS[0].surface_drop * CELL_H;
            let surface = f.y(m_of_row(base + 2) - drop);
            for p in &fl.list {
                assert!(
                    (p.at[1] - surface).abs() < 1e-6,
                    "{p:?} off the surface {surface}"
                );
            }
            // Carried downstream (+z) at the current's speed.
            let before: Vec<f64> = fl.list.iter().map(|p| p.at[2]).collect();
            let ids = fl.list.len();
            fl.update(&w, eye(f), 0.5, true);
            let moved = fl
                .list
                .iter()
                .take(ids.min(fl.list.len()))
                .zip(&before)
                .filter(|(p, z)| (p.at[2] - **z - 0.5).abs() < 1e-6)
                .count();
            assert!(moved > 5, "{moved} carried 0.5 m");
        }
    );

    dwell_world::both_faces!(
        fn still_water_and_the_setting_off_have_none(f: FaceFrame) {
            let w = pool(f, [0.0, 0.0]);
            let mut fl = Flecks::default();
            for _ in 0..240 {
                fl.update(&w, eye(f), 1.0 / 60.0, true);
            }
            assert!(fl.list.is_empty(), "still water: {} flecks", fl.list.len());
            let w = pool(f, [0.6, 0.0]);
            for _ in 0..240 {
                fl.update(&w, eye(f), 1.0 / 60.0, true);
            }
            assert!(!fl.list.is_empty());
            fl.update(&w, eye(f), 1.0 / 60.0, false);
            assert!(fl.list.is_empty(), "the setting off clears them");
        }
    );
}
