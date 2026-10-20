//! The reference marcher on crafted layouts (RAY_MARCHED_RENDERER.md §5 "Correctness under both"):
//! first hits against a brute-force DDA, borders, precision far from the origin, both faces,
//! skipping, cone LOD and a state added by data alone.

use std::collections::HashMap;

use dwell_blocks::{STATES, StateId, StateInfo, VoxelShape, mirror, require};
use dwell_march::layout::GRID_OFFSET;
use dwell_march::reference::brute_march;
use dwell_march::{
    Beam, MarchParams, NodeBuild, Outcome, Ray, Scene, StateTable, StepHistogram, build_chunk,
    build_node, build_section, face_up, level_for,
};
use dwell_world::{CHUNK_VOLUME, Face, MIRROR_SUM, local_index, lod};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        // splitmix64
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
    fn range(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    /// A direction with no zero component.
    fn dir(&mut self) -> [f32; 3] {
        loop {
            let d = [
                self.unit() * 2.0 - 1.0,
                self.unit() * 2.0 - 1.0,
                self.unit() * 2.0 - 1.0,
            ];
            let len2 = d.iter().map(|v| v * v).sum::<f32>();
            if len2 > 0.01 && len2 <= 1.0 && d.iter().all(|v| v.abs() > 1e-3) {
                return d;
            }
        }
    }
}

/// Level-0 voxels per node (grid node coordinates), `local_index` order.
type Voxels = HashMap<[i32; 3], Vec<StateId>>;

fn scene_of(voxels: &Voxels, table: &StateTable) -> Scene {
    let mut s = Scene::new(table.clone());
    let mut keys: Vec<_> = voxels.keys().copied().collect();
    keys.sort();
    for n in keys {
        s.put(0, n, &build_chunk(&voxels[&n], table));
    }
    s
}

fn occupied_in<'a>(
    voxels: &'a Voxels,
    table: &'a StateTable,
) -> impl Fn([i32; 3]) -> Option<StateId> + 'a {
    move |c: [i32; 3]| {
        let v = voxels.get(&c.map(|x| x >> 5))?;
        let s = v[local_index(c[0] & 31, c[1] & 31, c[2] & 31)];
        table.occupied(s).then_some(s)
    }
}

const BASE: [i32; 3] = [1 << 18, 1 << 18, 1 << 18];

/// 3×3×3 nodes of mixed content: empty, solid, terrain columns, sparse noise, water.
fn random_scene(seed: u64) -> Voxels {
    let mut r = Rng(seed);
    let ids = [
        require("dwell:stone"),
        require("dwell:dirt"),
        require("dwell:grass"),
        require("dwell:sand"),
        require("dwell:ladder"),
    ];
    let water = require("dwell:water");
    let mut out = Voxels::new();
    for z in 0..3 {
        for y in 0..3 {
            for x in 0..3 {
                let n = [BASE[0] + x, BASE[1] + y, BASE[2] + z];
                let kind = r.range(7);
                let mut v = vec![0u16; CHUNK_VOLUME];
                let density = 2 + r.range(40);
                let lonely = [r.range(32) as i32, r.range(32) as i32, r.range(32) as i32];
                let base_h = r.range(32) as i32;
                for lz in 0..32 {
                    for ly in 0..32 {
                        for lx in 0..32 {
                            let s = match kind {
                                0 => 0,
                                1 => ids[0],
                                2 => {
                                    let h = base_h + ((lx / 5 + lz / 7) % 6) - 3;
                                    if ly < h {
                                        ids[(lx + lz) as usize % 4]
                                    } else if ly < 10 {
                                        water
                                    } else {
                                        0
                                    }
                                }
                                3 => {
                                    if r.range(density) == 0 {
                                        ids[r.range(5) as usize]
                                    } else {
                                        0
                                    }
                                }
                                4 => {
                                    if r.range(3) == 0 {
                                        water
                                    } else {
                                        0
                                    }
                                }
                                5 => {
                                    // One voxel: empty bricks far from anything.
                                    if [lx, ly, lz] == lonely { ids[2] } else { 0 }
                                }
                                _ => {
                                    // Big blocks with air gaps: uniform and empty bricks together.
                                    if (lx / 8 + ly / 8 + lz / 8) % 3 == 0 {
                                        ids[1]
                                    } else {
                                        0
                                    }
                                }
                            };
                            v[local_index(lx, ly, lz)] = s;
                        }
                    }
                }
                out.insert(n, v);
            }
        }
    }
    out
}

/// A ray from a random point of the scene's box (or above it).
fn random_ray(r: &mut Rng) -> Ray {
    let lo = BASE.map(|b| b << 5);
    let cell = [
        lo[0] + r.range(96) as i32,
        lo[1] + r.range(110) as i32,
        lo[2] + r.range(96) as i32,
    ];
    Ray::new(cell, [r.unit(), r.unit(), r.unit()], r.dir())
}

fn params() -> MarchParams {
    MarchParams {
        max_t: 400.0,
        step_cap: 1 << 20,
        ..MarchParams::default()
    }
}

#[test]
fn first_hits_match_a_brute_force_dda_on_random_scenes() {
    let table = StateTable::registry();
    let mut compared = 0;
    let mut hits = 0;
    for seed in 0..6 {
        let voxels = random_scene(seed);
        let scene = scene_of(&voxels, &table);
        let occ = occupied_in(&voxels, &table);
        let mut r = Rng(seed + 100);
        for _ in 0..3000 {
            let ray = random_ray(&mut r);
            let want = brute_march(&ray, 0, &occ, 400.0, 1 << 20).outcome;
            for p in [params(), params().without_skipping()] {
                let got = scene.march(&ray, &p).outcome;
                match (got, want) {
                    (Outcome::Hit(g), Outcome::Hit(w)) => {
                        assert_eq!(g, w, "{ray:?} {p:?}");
                        hits += 1;
                    }
                    (Outcome::Miss, Outcome::Miss) => {}
                    _ => panic!("{ray:?}: {got:?} against {want:?} ({p:?})"),
                }
                compared += 1;
            }
        }
    }
    assert!(hits > compared / 3, "{hits} of {compared} hit");
}

#[test]
fn skipping_reduces_steps_without_changing_hits() {
    let table = StateTable::registry();
    for seed in [7, 8] {
        let voxels = random_scene(seed);
        let scene = scene_of(&voxels, &table);
        let mut r = Rng(seed);
        let (mut with, mut without) = (StepHistogram::default(), StepHistogram::default());
        for _ in 0..4000 {
            let ray = random_ray(&mut r);
            let a = scene.march(&ray, &params());
            let b = scene.march(&ray, &params().without_skipping());
            assert_eq!(a.outcome, b.outcome, "{ray:?}");
            with.add(&a);
            without.add(&b);
        }
        let (w, wo) = (with.summary(), without.summary());
        assert!(w.mean < wo.mean, "{w:?} vs {wo:?}");
    }
}

#[test]
fn sections_with_palette_bricks_and_columns_match_brute_force_at_their_level() {
    let table = StateTable::registry();
    let level = 3u32;
    let mut r = Rng(42);
    let mut scene = Scene::new(table.clone());
    let mut cells_of: HashMap<[i32; 3], Vec<StateId>> = HashMap::new();
    let stone = require("dwell:stone");
    let grass = require("dwell:grass");
    let sand = require("dwell:sand");
    let water = require("dwell:water");
    let base = [1 << 15, 1 << 15, 1 << 15];
    for k in 0..3 {
        for j in 0..2 {
            for i in 0..3 {
                let n = [base[0] + i, base[1] + j, base[2] + k];
                let mut cells = vec![0u16; lod::VOLUME];
                for z in 0..32 {
                    for x in 0..32 {
                        let gx = i * 32 + x;
                        let gz = k * 32 + z;
                        let h = 24
                            + ((gx as f32 * 0.21).sin() * 9.0 + (gz as f32 * 0.13).cos() * 11.0)
                                as i32;
                        for y in 0..32 {
                            let gy = j * 32 + y;
                            let s = if gy < h - 3 {
                                stone
                            } else if gy < h {
                                if r.range(4) == 0 { sand } else { grass }
                            } else if gy < 28 {
                                water
                            } else {
                                0
                            };
                            cells[lod::cell_index(x, y, z)] = s;
                        }
                    }
                }
                let c = lod::LodCoord::new(level as i32, n[0], n[1], n[2]);
                scene.put_section(c, &cells);
                cells_of.insert(n, cells);
            }
        }
    }
    let occ = |c: [i32; 3]| {
        let cells = cells_of.get(&c.map(|x| x >> 5))?;
        let s = cells[lod::cell_index(c[0] & 31, c[1] & 31, c[2] & 31)];
        table.occupied(s).then_some(s)
    };
    let lo = base.map(|b| b << (5 + level));
    let mut hits = 0;
    let mut steps = (0u64, 0u64);
    for _ in 0..4000 {
        let cell = [
            lo[0] + r.range(96 << level) as i32,
            lo[1] + (40 << level) + r.range(40 << level) as i32,
            lo[2] + r.range(96 << level) as i32,
        ];
        let mut d = r.dir();
        d[1] = -d[1].abs() * 0.3; // mostly grazing, toward the ground
        let ray = Ray::new(cell, [r.unit(), r.unit(), r.unit()], d);
        let want = brute_march(&ray, level, occ, 1.0e5, 1 << 20).outcome;
        let a = scene.march(
            &ray,
            &MarchParams {
                max_t: 1.0e5,
                ..params()
            },
        );
        let b = scene.march(
            &ray,
            &MarchParams {
                max_t: 1.0e5,
                ..params()
            }
            .without_skipping(),
        );
        assert_eq!(a.outcome, want, "{ray:?}");
        assert_eq!(b.outcome, want, "{ray:?}");
        if matches!(want, Outcome::Hit(_)) {
            hits += 1;
        }
        steps.0 += u64::from(a.steps);
        steps.1 += u64::from(b.steps);
    }
    assert!(hits > 500, "{hits}");
    assert!(steps.0 * 10 < steps.1 * 9, "columns skip: {steps:?}");
}

#[test]
fn rays_across_chunk_and_brick_borders_have_no_cracks() {
    let table = StateTable::registry();
    let stone = require("dwell:stone");
    // A floor whose top is a node border (y = 32 of the bottom row), 8×8 nodes wide, and a wall
    // whose face is a node border in x.
    let mut voxels = Voxels::new();
    for z in 0..8 {
        for x in 0..8 {
            voxels.insert(
                [BASE[0] + x, BASE[1], BASE[2] + z],
                vec![stone; CHUNK_VOLUME],
            );
            let mut v = vec![0u16; CHUNK_VOLUME];
            if x == 3 {
                // x = 0 of the node: a wall face at a node border.
                for s in v.iter_mut().step_by(32) {
                    *s = stone;
                }
            }
            voxels.insert([BASE[0] + x, BASE[1] + 1, BASE[2] + z], v);
        }
    }
    let scene = scene_of(&voxels, &table);
    let floor = (BASE[1] + 1) << 5;
    let wall = (BASE[0] + 3) << 5;
    let mut r = Rng(5);
    for i in 0..20000 {
        // Origins on brick and node borders (fraction 0) and just off them.
        let cell = [
            (BASE[0] << 5) + 8 * r.range(8) as i32,
            floor + 1 + r.range(20) as i32,
            (BASE[2] << 5) + 8 * r.range(8) as i32,
        ];
        let f = if i % 2 == 0 {
            [0.0, r.unit(), 0.0]
        } else {
            [r.unit() * 1e-3, r.unit(), 1.0 - r.unit() * 1e-3]
        };
        let mut d = r.dir();
        d[0] = d[0].abs();
        d[1] = -d[1].abs().max(0.15);
        d[2] = d[2].abs();
        let ray = Ray::new(cell, f, d);
        let m = scene.march(&ray, &params());
        let h = m
            .hit()
            .unwrap_or_else(|| panic!("{ray:?} fell through: {m:?}"));
        let want = brute_march(&ray, 0, occupied_in(&voxels, &table), 400.0, 1 << 20)
            .hit()
            .expect("brute hits");
        assert_eq!(h, want, "{ray:?}");
        // On the floor's top or the wall's face, at that plane exactly.
        let p = h.point(&ray);
        match h.face {
            Some(2) => assert_eq!(p[1], (floor - cell[1]) as f32 - f[1]),
            Some(1) => assert_eq!(p[0], (wall - cell[0]) as f32 - f[0]),
            other => panic!("face {other:?} {ray:?}"),
        }
    }
}

#[test]
fn far_from_the_origin_hits_are_bit_identical() {
    let table = StateTable::registry();
    let voxels = random_scene(3);
    // 4,000 km east: 125,000 nodes of 32 m.
    let shift = 4_000_000 / 32;
    let far: Voxels = voxels
        .iter()
        .map(|(n, v)| ([n[0] + shift, n[1], n[2]], v.clone()))
        .collect();
    let near_scene = scene_of(&voxels, &table);
    let far_scene = scene_of(&far, &table);
    let mut r = Rng(9);
    let mut hits = 0;
    for _ in 0..3000 {
        let ray = random_ray(&mut r);
        let mut moved = ray;
        moved.cell[0] += shift << 5;
        let a = near_scene.march(&ray, &params());
        let b = far_scene.march(&moved, &params());
        assert_eq!(a.steps, b.steps);
        match (a.outcome, b.outcome) {
            (Outcome::Hit(a), Outcome::Hit(mut b)) => {
                assert_eq!(a.point(&ray), b.point(&moved));
                b.cell[0] -= shift << 5;
                assert_eq!(a, b);
                hits += 1;
            }
            (x, y) => assert_eq!(x, y),
        }
    }
    assert!(hits > 500);
    // What camera-relative integer state avoids: f32 world positions there resolve 0.5 m.
    let x = 4_000_000.0f32 + GRID_OFFSET as f32;
    assert_ne!((x + 0.3) - x, 0.3);
}

#[test]
fn face_b_is_face_a_mirrored() {
    let table = StateTable::registry();
    // Terrain-like content just above the midplane, then the same mirrored onto face B.
    let mid_node = ((dwell_world::MIDPLANE_Y as i64 + GRID_OFFSET) >> 5) as i32;
    let voxels: Voxels = random_scene(11)
        .into_iter()
        .map(|(n, v)| ([n[0], mid_node + (n[1] - BASE[1]), n[2]], v))
        .collect();
    // Grid y ↔ M − y mirrors voxels about the midplane.
    let m = MIRROR_SUM + 2 * GRID_OFFSET as i32;
    let mirrored: Voxels = voxels
        .iter()
        .map(|(n, v)| {
            let mut out = vec![0u16; CHUNK_VOLUME];
            for z in 0..32 {
                for y in 0..32 {
                    for x in 0..32 {
                        out[local_index(x, 31 - y, z)] = mirror(v[local_index(x, y, z)]);
                    }
                }
            }
            // Node row ny holds grid rows 32 ny..32 ny + 31 → M − those.
            ([n[0], ((m - (n[1] << 5)) >> 5), n[2]], out)
        })
        .collect();
    let a_scene = scene_of(&voxels, &table);
    let b_scene = scene_of(&mirrored, &table);
    let mut r = Rng(12);
    let mut hits = 0;
    for _ in 0..3000 {
        let mut ray = random_ray(&mut r);
        ray.cell[1] += (mid_node - BASE[1]) << 5;
        ray.frac[1] = ray.frac[1].clamp(0.01, 0.99);
        let mut flipped = ray;
        flipped.cell[1] = m - ray.cell[1];
        flipped.frac[1] = 1.0 - ray.frac[1];
        flipped.dir[1] = -ray.dir[1];
        let a = a_scene.march(&ray, &params());
        let b = b_scene.march(&flipped, &params());
        match (a.outcome, b.outcome) {
            (Outcome::Hit(a), Outcome::Hit(b)) => {
                assert_eq!(b.cell, [a.cell[0], m - a.cell[1], a.cell[2]]);
                assert_eq!(b.state, mirror(a.state));
                assert_eq!(b.face.map(|f| face_up(Face::B, f)), a.face);
                assert!((a.t - b.t).abs() <= 1e-4 * a.t.max(1.0), "{} {}", a.t, b.t);
                hits += 1;
            }
            (x, y) => assert_eq!(x, y, "{ray:?}"),
        }
    }
    assert!(hits > 500);
}

#[test]
fn a_state_added_by_data_alone_is_hit() {
    // A fixture registry: this build's states plus a new solid one and a new liquid one, as a
    // shared/blocks entry would generate them.
    let mut states: Vec<StateInfo> = STATES.to_vec();
    let solid = StateInfo {
        name: "test:basalt",
        color: 0x303030,
        ..STATES[usize::from(require("dwell:stone"))]
    };
    let liquid = StateInfo {
        name: "test:oil",
        ..STATES[usize::from(require("dwell:water"))]
    };
    assert_eq!(solid.shape, VoxelShape::Full);
    states.push(solid);
    states.push(liquid);
    let (basalt, oil) = (STATES.len() as StateId, STATES.len() as StateId + 1);
    let table = StateTable::from_states(&states);
    let mut v = vec![0u16; CHUNK_VOLUME];
    for z in 0..32 {
        for x in 0..32 {
            v[local_index(x, 2, z)] = basalt;
            for y in 3..10 {
                v[local_index(x, y, z)] = oil;
            }
        }
    }
    let NodeBuild::Node(_) = build_chunk(&v, &table) else {
        panic!("a node");
    };
    let mut scene = Scene::new(table);
    scene.put(0, BASE, &build_chunk(&v, scene.table()));
    let ray = Ray::new(
        [(BASE[0] << 5) + 5, (BASE[1] << 5) + 20, (BASE[2] << 5) + 5],
        [0.5; 3],
        [0.1, -1.0, 0.05],
    );
    let h = scene.march(&ray, &params()).hit().expect("hits");
    assert_eq!(h.state, basalt, "through the new liquid onto the new solid");
    assert_eq!(h.face, Some(2));
    assert_eq!(h.cell[1], (BASE[1] << 5) + 2);
}

#[test]
fn water_is_transparent_to_primary_rays() {
    let table = StateTable::registry();
    let water = require("dwell:water");
    let sand = require("dwell:sand");
    let v: Vec<StateId> = (0..CHUNK_VOLUME)
        .map(|i| match (i >> 5) & 31 {
            0..=3 => sand,
            4..=20 => water,
            _ => 0,
        })
        .collect();
    let mut scene = Scene::new(table.clone());
    scene.put(0, BASE, &build_chunk(&v, &table));
    let ray = Ray::new(
        [(BASE[0] << 5) + 3, (BASE[1] << 5) + 25, (BASE[2] << 5) + 3],
        [0.5; 3],
        [0.3, -1.0, 0.2],
    );
    let h = scene.march(&ray, &params()).hit().expect("hits");
    assert_eq!(
        (h.state, h.face, h.cell[1]),
        (sand, Some(2), (BASE[1] << 5) + 3)
    );
}

#[test]
fn cone_lod_reaches_coarser_levels_with_distance() {
    let p = MarchParams {
        pixel_angle: 1.0e-3,
        lod_pixels: 4.0,
        ..MarchParams::default()
    };
    assert_eq!(level_for(100.0, &p), 0);
    assert_eq!(level_for(500.0, &p), 1);
    assert_eq!(level_for(1000.0, &p), 2);
    assert_eq!(level_for(1.0e9, &p), dwell_march::layout::MAX_LEVEL);
    assert_eq!(level_for(1.0e9, &MarchParams::default()), 0);

    let table = StateTable::registry();
    let stone = require("dwell:stone");
    let mut scene = Scene::new(table.clone());
    // Level 0: 12 air nodes along +x. Level 2: a wall 2 cells thick at x = 120..128 m.
    for x in 0..12 {
        scene.put(0, [BASE[0] + x, BASE[1], BASE[2]], &NodeBuild::Air);
    }
    let b2 = BASE.map(|b| b >> 2);
    let wall = build_node(
        |x, _, _| if (30..32).contains(&x) { stone } else { 0 },
        &table,
        dwell_march::BuildOptions::SECTION,
    );
    scene.put(2, b2, &wall);
    let ray = Ray::new(
        [(BASE[0] << 5) + 1, (BASE[1] << 5) + 16, (BASE[2] << 5) + 16],
        [0.5; 3],
        [1.0, 0.0001, 0.0002],
    );
    // Fine: level 0 is air through the wall's span; past it level 2 holds nothing more.
    let fine = scene.march(&ray, &MarchParams::default());
    assert_eq!(fine.outcome, Outcome::Miss);
    // Coarse pixels: level 2 is wanted from 50 m (8 cm pixels).
    let coarse = scene.march(
        &ray,
        &MarchParams {
            pixel_angle: 2.0e-2,
            ..MarchParams::default()
        },
    );
    let h = coarse.hit().expect("the level-2 wall");
    assert_eq!((h.level, h.face, h.state), (2, Some(1), stone));
    assert_eq!(h.cell[0], (b2[0] << 5) + 30);
    assert!((h.t - (120.0 - 1.5)).abs() < 1e-3, "{}", h.t);
}

#[test]
fn rays_from_above_the_data_leave_at_once() {
    let table = StateTable::registry();
    let scene = scene_of(&random_scene(1), &table);
    let above = (BASE[1] + 4) << 5;
    let up = Ray::new(
        [BASE[0] << 5, above, BASE[2] << 5],
        [0.5; 3],
        [0.2, 1.0, 0.1],
    );
    assert_eq!(scene.march(&up, &params()).steps, 0);
    let down = Ray::new(
        [(BASE[0] << 5) + 40, above, (BASE[2] << 5) + 40],
        [0.5; 3],
        [0.2, -1.0, 0.1],
    );
    let m = scene.march(&down, &params());
    let want = brute_march(
        &down,
        0,
        occupied_in(&random_scene(1), &table),
        400.0,
        1 << 20,
    );
    assert_eq!(m.outcome, want.outcome);
}

#[test]
fn the_step_cap_ends_rays() {
    let table = StateTable::registry();
    let scene = scene_of(&random_scene(0), &table);
    let mut r = Rng(1);
    let ray = random_ray(&mut r);
    let m = scene.march(
        &ray,
        &MarchParams {
            step_cap: 1,
            ..params()
        }
        .without_skipping(),
    );
    assert!(matches!(m.outcome, Outcome::Cap | Outcome::Hit(_)));
    assert!(m.steps <= 2);
}

#[test]
fn section_builds_place_at_their_lod_coordinates() {
    let table = StateTable::registry();
    let c = lod::LodCoord::new(4, 100, 200, 300);
    let mut cells = vec![0u16; lod::VOLUME];
    cells[lod::cell_index(0, 0, 0)] = require("dwell:stone");
    let mut scene = Scene::new(table.clone());
    let b = scene.put_section(c, &cells);
    assert_eq!(b, build_section(&cells, &table));
    assert!(matches!(
        scene.slot(4, [100, 200, 300]),
        dwell_march::layout::Slot::Node(_)
    ));
    // Its grid box is the section's world box, shifted by the LOD origin.
    let o = lod::section_origin(c);
    let (lo, _) = scene.bounds().expect("data");
    assert_eq!(i64::from(lo[0]), o.x + GRID_OFFSET);
    assert_eq!(i64::from(lo[1]), o.y + GRID_OFFSET);
}

#[test]
fn exact_ties_and_axis_aligned_rays_agree_with_brute_force() {
    // Rational directions from cell corners and centres cross several planes at exactly the same
    // distance; both take the lower axis first.
    let table = StateTable::registry();
    let dirs: [[f32; 3]; 9] = [
        [1.0, -1.0, 1.0],
        [-1.0, -1.0, 0.0],
        [2.0, -1.0, 0.0],
        [0.0, -1.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 0.0, -1.0],
        [1.0, -2.0, 2.0],
        [-3.0, 1.0, -3.0],
        [0.0, 1.0, 1.0],
    ];
    for seed in 20..23 {
        let voxels = random_scene(seed);
        let scene = scene_of(&voxels, &table);
        let occ = occupied_in(&voxels, &table);
        let mut r = Rng(seed);
        for i in 0..3000 {
            let ray = random_ray(&mut r);
            let frac = if i % 2 == 0 { [0.0; 3] } else { [0.5; 3] };
            let ray = Ray::new(ray.cell, frac, dirs[i % dirs.len()]);
            let want = brute_march(&ray, 0, &occ, 400.0, 1 << 20).outcome;
            for p in [params(), params().without_skipping()] {
                assert_eq!(scene.march(&ray, &p).outcome, want, "{ray:?} {p:?}");
            }
        }
    }
}

/// Flat ground seen from 20 m up along a radial line, rays from 30° below the horizon to 1°:
/// level 0 holds the ground with its top at `fine_top` (cells above the node base) out to 512 m,
/// levels 1 and 2 hold the same ground voxelised at their cell size, rounded up (a coarse cell is
/// solid when any of it is below the surface, as the LOD's cells are), out to 2 km. The cone LOD
/// moves from level 0 to 1 at 100 m and to 2 at 200 m. Returns each ray's hit (face, the hit
/// point's height above the node base, t), nearest pitch first.
/// Flat ground at `fine_top` m on levels 0–2, each rounding it up to its cells: 16 chunks along
/// +x from `BASE`, and the sections over the same span.
fn flat_ground_levels(fine_top: i32) -> Scene {
    // Rounded up, in each level's cells.
    flat_ground_tops(std::array::from_fn(|l| (fine_top + (1 << l) - 1) >> l))
}

/// Flat ground with its top at `tops[L]` cells of level L.
fn flat_ground_tops(tops: [i32; 3]) -> Scene {
    let table = StateTable::registry();
    let grass = require("dwell:grass");
    let mut scene = Scene::new(table.clone());
    let n = BASE;
    for level in 0..=2u32 {
        let top = tops[level as usize];
        let ground = build_node(
            |_, y, _| if y < top { grass } else { 0 },
            &table,
            if level == 0 {
                dwell_march::BuildOptions::CHUNK
            } else {
                dwell_march::BuildOptions::SECTION
            },
        );
        let nodes = if level == 0 { 16 } else { 64 >> level };
        for x in 0..nodes {
            for z in -1..=1 {
                let at = [(n[0] >> level) + x, n[1] >> level, (n[2] >> level) + z];
                scene.put(level, at, &ground);
            }
        }
    }
    scene
}

fn radial_line_over_flat_ground(fine_top: i32) -> Vec<(Option<usize>, f32, f32)> {
    let scene = flat_ground_levels(fine_top);
    let n = BASE;
    let p = MarchParams {
        pixel_angle: 0.005,
        lod_pixels: 4.0,
        ..MarchParams::default()
    };
    assert_eq!(level_for(99.0, &p), 0);
    assert_eq!(level_for(101.0, &p), 1);
    let base_y = n[1] << 5;
    let eye = [(n[0] << 5) + 2, base_y + 20, (n[2] << 5) + 16];
    let mut out = Vec::new();
    for i in 0..600 {
        let pitch = (-30.0f32 + 29.0 * i as f32 / 599.0).to_radians();
        let ray = Ray::new(eye, [0.5; 3], [pitch.cos(), pitch.sin(), 0.0137]);
        let m = scene.march(&ray, &p);
        let h = m.hit().expect("flat ground is hit");
        let y = h.point(&ray)[1] + (eye[1] - base_y) as f32 + 0.5;
        out.push((h.face, y, h.t));
    }
    out
}

#[test]
fn cone_lod_switches_leave_no_seam_on_flat_ground() {
    // Tops on every level's grid: the levels agree, so the line is one smooth plane.
    let aligned = radial_line_over_flat_ground(8);
    for (i, &(face, y, t)) in aligned.iter().enumerate() {
        assert_eq!(face, Some(2), "ray {i}: the top face (t {t})");
        assert!((y - 8.0).abs() < 1e-3, "ray {i}: hit at height {y}");
    }
    // Depth along the line grows smoothly: no jump beyond the plane's own spacing.
    for w in aligned.windows(2) {
        assert!(w[1].2 >= w[0].2 && w[1].2 - w[0].2 < 0.05 * w[0].2, "{w:?}");
    }
    // Tops off the coarse grids (5 m: level 1 rounds to 6, level 2 to 8). Where the cone LOD
    // moves up a level the ray must not hit the coarser cell on the plane it last crossed (a side
    // face, a dark arc around the eye): every hit is a top face, at one of the levels' heights.
    let off = radial_line_over_flat_ground(5);
    let sides: Vec<_> = off
        .iter()
        .enumerate()
        .filter(|(_, h)| h.0 != Some(2))
        .collect();
    assert!(sides.is_empty(), "side faces at the switch: {sides:?}");
    for (i, &(_, y, _)) in off.iter().enumerate() {
        assert!(
            [5.0, 6.0, 8.0].iter().any(|h| (y - h).abs() < 1e-3),
            "ray {i}: hit at height {y}"
        );
    }
}

// --- the beam prepass (Phase 25 A4b) ------------------------------------------------------------

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    v.map(|x| x / l)
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// A tile of `BEAM_TILE`² pixel rays `pixel_angle` apart around `forward`, and its beam.
fn tile(cell: [i32; 3], frac: [f32; 3], forward: [f32; 3], pixel_angle: f32) -> (Beam, Vec<Ray>) {
    let f = normalize(forward);
    let hint = if f[1].abs() > 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let right = normalize(cross(f, hint));
    let up = cross(right, f);
    let n = dwell_march::BEAM_TILE as i32;
    let at = |u: f32, v: f32| -> [f32; 3] {
        normalize(std::array::from_fn(|k| {
            f[k] + u * pixel_angle * right[k] + v * pixel_angle * up[k]
        }))
    };
    let half = n as f32 / 2.0;
    let mut rays = Vec::new();
    for j in 0..n {
        for i in 0..n {
            let d = at(i as f32 + 0.5 - half, j as f32 + 0.5 - half);
            rays.push(Ray { cell, frac, dir: d });
        }
    }
    let c = half - 0.5;
    let corners = [at(-c, -c), at(c, -c), at(-c, c), at(c, c)];
    (Beam::new(cell, frac, at(0.0, 0.0), &corners), rays)
}

/// Checks a tile: no pixel ray hits before the beam's start, and marching from the start finds
/// the hit a march from the eye finds. Returns (start, steps from the eye, steps from the start).
fn check_tile(scene: &Scene, beam: &Beam, rays: &[Ray], p: &MarchParams) -> (f32, u64, u64) {
    let s = scene.beam_start(beam, p);
    let (mut a, mut b) = (0u64, 0u64);
    for ray in rays {
        let full = scene.march(ray, p);
        let from = scene.march_from(ray, p, s);
        if let Outcome::Hit(h) = full.outcome {
            assert!(
                s <= h.t,
                "the start {s} passes the hit at {} ({ray:?}, {beam:?})",
                h.t
            );
        }
        assert_eq!(from.outcome, full.outcome, "start {s}: {ray:?} {beam:?}");
        a += u64::from(full.steps);
        b += u64::from(from.steps);
    }
    (s, a, b)
}

#[test]
fn beam_starts_never_pass_a_hit_on_random_scenes() {
    let table = StateTable::registry();
    let (mut started, mut tiles, mut steps) = (0, 0, (0u64, 0u64));
    for seed in 0..4 {
        let voxels = random_scene(seed);
        let scene = scene_of(&voxels, &table);
        let mut r = Rng(seed + 900);
        for i in 0..120 {
            let ray = random_ray(&mut r);
            let pa = [0.002, 0.01, 0.04][i % 3];
            let p = MarchParams {
                pixel_angle: pa,
                ..params()
            };
            let (beam, rays) = tile(ray.cell, ray.frac, ray.dir, pa);
            let (s, a, b) = check_tile(&scene, &beam, &rays, &p);
            tiles += 1;
            if s > 0.0 {
                started += 1;
            }
            steps.0 += a;
            steps.1 += b;
        }
    }
    // It does start rays late, and saves steps.
    assert!(started * 2 > tiles, "{started} of {tiles} tiles start late");
    assert!(steps.1 < steps.0, "{steps:?}");
}

fn beam_ray(b: &Beam) -> Ray {
    Ray {
        cell: b.cell,
        frac: b.frac,
        dir: b.dir,
    }
}

#[test]
fn a_beam_stops_before_a_voxel_only_some_pixel_rays_hit() {
    let table = StateTable::registry();
    let stone = require("dwell:stone");
    let n = BASE;
    let pa = 0.01;
    let eye = [(n[0] << 5) + 2, (n[1] << 5) + 16, (n[2] << 5) + 16];
    let frac = [0.5; 3];
    let (beam, rays) = tile(eye, frac, [1.0, 0.0, 0.0], pa);
    // The voxel one pixel ray (not the centre's, not a corner's) passes through at 60 m.
    let target = rays[2 * 8 + 5];
    let t = 60.0;
    let hit: [i32; 3] =
        std::array::from_fn(|k| eye[k] + (frac[k] + t * target.dir[k]).floor() as i32);
    let mut voxels = Voxels::new();
    for x in 0..4 {
        voxels.insert([n[0] + x, n[1], n[2]], vec![0u16; CHUNK_VOLUME]);
    }
    let node = hit.map(|v| v >> 5);
    voxels.get_mut(&node).expect("in the scene")
        [local_index(hit[0] & 31, hit[1] & 31, hit[2] & 31)] = stone;
    let scene = scene_of(&voxels, &table);
    let p = MarchParams {
        pixel_angle: pa,
        ..params()
    };
    let hits = rays
        .iter()
        .filter(|r| scene.march(r, &p).hit().is_some())
        .count();
    assert!((1..64).contains(&hits), "{hits} rays hit the voxel");
    assert!(
        scene.march(&beam_ray(&beam), &p).hit().is_none(),
        "the centre ray misses it"
    );
    let (s, _, _) = check_tile(&scene, &beam, &rays, &p);
    assert!(s > 20.0 && s < 60.0, "start {s}");
}

#[test]
fn beam_starts_never_pass_a_hit_across_lod_levels() {
    // Levels 0–2 disagree on the ground's height (5, 6 and 8 m; then 9, 6 and 4 m, a finer level
    // above a coarser one, as an edit not yet in the LOD leaves it): a ray can stop at any of them
    // near a switch, so the beam must clear every level it could ask for.
    for scene in [flat_ground_levels(5), flat_ground_tops([9, 3, 1])] {
        beam_over_flat_ground(&scene);
    }
}

fn beam_over_flat_ground(scene: &Scene) {
    let n = BASE;
    let eye = [(n[0] << 5) + 2, (n[1] << 5) + 20, (n[2] << 5) + 16];
    let mut r = Rng(5);
    let mut started = 0;
    let mut tiles = 0;
    for i in 0..200 {
        let pitch = (-30.0f32 + 29.0 * i as f32 / 199.0).to_radians();
        let yaw = (r.unit() - 0.5) * 0.4;
        let dir = [
            pitch.cos() * yaw.cos(),
            pitch.sin(),
            pitch.cos() * yaw.sin(),
        ];
        let pa = [0.005, 0.003, 0.01][i % 3];
        let p = MarchParams {
            pixel_angle: pa,
            lod_pixels: 4.0,
            ..MarchParams::default()
        };
        let (beam, rays) = tile(eye, [0.5; 3], dir, pa);
        let s = scene.beam_start(&beam, &p);
        for ray in &rays {
            if let Some(h) = scene.march(ray, &p).hit() {
                assert!(s <= h.t, "start {s} passes the hit at {} ({ray:?})", h.t);
            }
        }
        tiles += 1;
        if s > 10.0 {
            started += 1;
        }
    }
    assert!(started * 2 > tiles, "{started} of {tiles}");
}
