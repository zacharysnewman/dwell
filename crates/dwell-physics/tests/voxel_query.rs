//! VoxelQuery's exact-shape ray cast against a plain box ray cast (cubes must answer exactly as a
//! box cast does), and the submerged fraction of flooded blocks.

mod common;

use common::*;
use dwell_blocks::{VoxelShape, info, require};
use dwell_physics::query::{ProbeHit, VoxelQuery};
use dwell_physics::{DVec3, Vec3};

const INF: f32 = f32::INFINITY;

/// The box-based Amanatides–Woo cast: a cell's shape is the box [0,1]×[0,h]×[0,1], h = 1 for a
/// cube, ½ for a shaped block.
fn reference_cast(q: &VoxelQuery, origin: DVec3, dir: Vec3, max_distance: f32) -> Option<ProbeHit> {
    let base = [
        origin.x.floor() as i32,
        origin.y.floor() as i32,
        origin.z.floor() as i32,
    ];
    let o = [
        (origin.x - f64::from(base[0])) as f32,
        (origin.y - f64::from(base[1])) as f32,
        (origin.z - f64::from(base[2])) as f32,
    ];
    let d = [dir.x, dir.y, dir.z];
    let (mut cell, mut step, mut t_max, mut t_delta) = ([0i32; 3], [0i32; 3], [INF; 3], [INF; 3]);
    for i in 0..3 {
        cell[i] = o[i].floor() as i32;
        if d[i] > 0.0 {
            step[i] = 1;
            t_delta[i] = 1.0 / d[i];
            t_max[i] = (cell[i] as f32 + 1.0 - o[i]) / d[i];
        } else if d[i] < 0.0 {
            step[i] = -1;
            t_delta[i] = -1.0 / d[i];
            t_max[i] = (cell[i] as f32 - o[i]) / d[i];
        }
    }
    let (mut in_solid, mut t_cell, mut entry_axis) = (true, 0.0f32, -1i32);
    while t_cell <= max_distance {
        let t_next = t_max[0].min(t_max[1]).min(t_max[2]);
        let height =
            match info(q.material(base[0] + cell[0], base[1] + cell[1], base[2] + cell[2])).shape {
                VoxelShape::Full => 1.0,
                VoxelShape::Shaped => 0.5,
                VoxelShape::Empty => 0.0,
            };
        let mut free_after = true;
        if height > 0.0 {
            let (mut e0, mut e1, mut axis0, mut miss) = (-INF, INF, -1i32, false);
            for i in 0..3 {
                let lo = cell[i] as f32;
                let hi = lo + if i == 1 { height } else { 1.0 };
                if d[i] == 0.0 {
                    if o[i] < lo || o[i] > hi {
                        miss = true;
                        break;
                    }
                    continue;
                }
                let (mut a, mut b) = ((lo - o[i]) / d[i], (hi - o[i]) / d[i]);
                if a > b {
                    std::mem::swap(&mut a, &mut b);
                }
                if a > e0 {
                    e0 = a;
                    axis0 = i as i32;
                }
                e1 = e1.min(b);
            }
            e0 = e0.max(t_cell);
            e1 = e1.min(t_next);
            if !miss && e0 <= e1 {
                const EPS: f32 = 1e-6;
                if e0 > t_cell + EPS || !in_solid {
                    if e0 > max_distance {
                        return None;
                    }
                    let axis = if e0 > t_cell + EPS { axis0 } else { entry_axis };
                    let mut n = [0.0f32; 3];
                    if axis >= 0 {
                        n[axis as usize] = if d[axis as usize] > 0.0 { -1.0 } else { 1.0 };
                    }
                    return Some(ProbeHit {
                        distance: e0,
                        point: origin + dir.as_dvec3() * f64::from(e0),
                        normal: Vec3::from(n),
                        ..ProbeHit::default()
                    });
                }
                free_after = e1 < t_next - EPS;
            }
        }
        in_solid = !free_after;
        let mut axis = 0;
        if t_max[1] < t_max[axis] {
            axis = 1;
        }
        if t_max[2] < t_max[axis] {
            axis = 2;
        }
        if t_max[axis] == INF {
            break;
        }
        t_cell = t_max[axis];
        t_max[axis] += t_delta[axis];
        cell[axis] += step[axis];
        entry_axis = axis as i32;
    }
    None
}

/// A small deterministic generator (xorshift64*), so the cases are the same everywhere.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 32) as u32
    }
    fn uniform(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * (self.next() as f32 / u32::MAX as f32)
    }
}

/// Within 1e-3 of a cell boundary (or a half-cell plane) on at least two axes: no one answer.
fn near_edges(at: DVec3) -> i32 {
    at.to_array()
        .iter()
        .map(|&v| {
            let f = v - v.floor();
            i32::from(!(1e-3..=1.0 - 1e-3).contains(&f) || (f - 0.5).abs() < 1e-3)
        })
        .sum()
}

#[test]
fn a_flooded_block_counts_as_water_for_swimming_and_a_dry_one_does_not() {
    let mut w = PlayerTestWorld::new();
    w.set(0, 0, 0, require("dwell:ladder[facing=south,flooded=true]"));
    w.set(4, 0, 0, require("dwell:ladder[facing=south]"));
    w.set(8, 0, 0, require("dwell:water"));
    let q = VoxelQuery::new(&mut w.world, &w.physics);
    let submerged = |x: f32| q.submerged_fraction(to_world(Vec3::new(x + 0.5, 0.9, 0.5)), 0.9);
    assert!(
        approx(submerged(8.0), submerged(0.0), 1e-5),
        "as wet as plain water"
    );
    assert!(submerged(0.0) > 0.4);
    assert_eq!(submerged(4.0), 0.0);
}

#[test]
fn axis_aligned_rays_from_grid_points_answer_like_the_box_cast() {
    // The ray caster itself, in world coordinates: nothing face-specific for face B to prove.
    if face_b() {
        return;
    }
    let mut w = PlayerTestWorld::new();
    let mut rng = Rng(11);
    for z in -4..=4 {
        for y in -2..=2 {
            for x in -4..=4 {
                match rng.next() % 6 {
                    0 => w.set(x, y, z, stone()),
                    1 => w.set(x, y, z, require("dwell:dirt")),
                    _ => {}
                }
            }
        }
    }
    let q = VoxelQuery::new(&mut w.world, &w.physics);
    let dirs: [[f32; 3]; 11] = [
        [1., 0., 0.],
        [-1., 0., 0.],
        [0., 1., 0.],
        [0., -1., 0.],
        [0., 0., 1.],
        [0., 0., -1.],
        [1., 1., 0.],
        [1., -1., 0.],
        [0., 1., 1.],
        [-1., -1., 1.],
        [1., 1., 1.],
    ];
    let (mut compared, mut hits) = (0, 0);
    for zi in -16..=16 {
        for yi in -8..=8 {
            for xi in -16..=16 {
                let origin = to_world(Vec3::new(
                    xi as f32 * 0.25,
                    yi as f32 * 0.25,
                    zi as f32 * 0.25,
                ));
                for raw in dirs {
                    let d = Vec3::from(raw).normalize();
                    // A ray starting on a cell's edge or corner, or running in a cell-face plane,
                    // grazes cells and has no one answer.
                    let edge_start =
                        i32::from(xi % 4 == 0) + i32::from(yi % 4 == 0) + i32::from(zi % 4 == 0)
                            >= 2;
                    let cells = [xi, yi, zi];
                    let in_plane = (0..3).any(|k| raw[k] == 0.0 && cells[k] % 2 == 0);
                    if edge_start || in_plane {
                        continue;
                    }
                    let e = reference_cast(&q, origin, d, 6.0);
                    let a = q.cast_voxels(origin, d, 6.0);
                    compared += 1;
                    // A hit on a cell's edge (the ray grazes it) may go either way.
                    if e.is_some_and(|e| near_edges(e.point) >= 2)
                        || a.is_some_and(|a| near_edges(a.point) >= 2)
                    {
                        continue;
                    }
                    let ctx =
                        format!("origin {xi},{yi},{zi}/4 dir {raw:?}: expected {e:?}, got {a:?}");
                    assert_eq!(a.is_some(), e.is_some(), "{ctx}");
                    if let (Some(a), Some(e)) = (a, e) {
                        hits += 1;
                        assert!(approx(a.distance, e.distance, 1e-4), "{ctx}");
                        assert_eq!(a.normal, e.normal, "{ctx}");
                    }
                }
            }
        }
    }
    assert!(
        compared > 50_000 && hits > 1000,
        "{compared} rays, {hits} hits"
    );
}

#[test]
fn the_exact_shape_ray_cast_answers_like_the_box_cast_on_cubes() {
    if face_b() {
        return; // as above
    }
    let mut w = PlayerTestWorld::new();
    let mut rng = Rng(7);
    for z in -6..=6 {
        for y in -3..=3 {
            for x in -6..=6 {
                match rng.next() % 8 {
                    0 => w.set(x, y, z, stone()),
                    1 => w.set(x, y, z, require("dwell:dirt")),
                    2 => w.set(x, y, z, require("dwell:grass")),
                    _ => {}
                }
            }
        }
    }
    let q = VoxelQuery::new(&mut w.world, &w.physics);
    let (mut compared, mut hits) = (0, 0);
    for i in 0..20000 {
        let origin = to_world(Vec3::new(
            rng.uniform(-5.0, 5.0),
            rng.uniform(-5.0, 5.0) * 0.5,
            rng.uniform(-5.0, 5.0),
        ));
        let d = Vec3::new(
            rng.uniform(-1.0, 1.0),
            rng.uniform(-1.0, 1.0),
            rng.uniform(-1.0, 1.0),
        );
        if d.length_squared() < 1e-4 {
            continue;
        }
        let d = d.normalize();
        let e = reference_cast(&q, origin, d, 12.0);
        let a = q.cast_voxels(origin, d, 12.0);
        compared += 1;
        let near_edge = e.map_or(0, |e| near_edges(e.point));
        let ctx = format!("ray {i} origin {origin} dir {d}: expected {e:?}, got {a:?}");
        if a.is_some() != e.is_some() && near_edge >= 2 {
            continue;
        }
        assert_eq!(a.is_some(), e.is_some(), "{ctx}");
        if let (Some(a), Some(e)) = (a, e) {
            hits += 1;
            assert!(approx(a.distance, e.distance, 1e-4), "{ctx}");
            if near_edge < 2 {
                assert_eq!(a.normal, e.normal, "{ctx}");
            }
        }
    }
    assert!(hits > 1000, "{compared} rays, {hits} hits");
}
