//! Resurrection shrines (`shrines.rs`, `shared/worldgen/shrines.json`; owner 2026-11-12: "You respawn
//! at the nearest shrine when you die", ~2 km apart): sited from the seed per face, about a grid
//! cell apart within the jitter, never in water or on steep ground nor by a portal; the structure
//! standing at each site whichever order its chunks are generated in, turned to face downhill (the
//! seed's turn on flat ground); the nearest one found from a death; and every spawn at one.

use std::collections::HashMap;

use dwell_blocks::{StateId, require};
use dwell_world::*;
use dwell_worldgen::BifacialTerrain;
use dwell_worldgen::shrines::{SHRINES, Shrine, ShrineRules, Shrines};

const S: i32 = CHUNK_SIZE;

/// The world's voxels, generated a chunk at a time on demand.
struct Voxels<'a> {
    g: &'a BifacialTerrain,
    chunks: HashMap<ChunkCoord, Chunk>,
}

impl<'a> Voxels<'a> {
    fn new(g: &'a BifacialTerrain) -> Self {
        Self {
            g,
            chunks: HashMap::new(),
        }
    }
    fn at(&mut self, x: i32, y: i32, z: i32) -> StateId {
        let c = chunk_of(x, y, z);
        let g = self.g;
        let chunk = self.chunks.entry(c).or_insert_with(|| {
            let mut chunk = Chunk::default();
            g.generate(c, &mut chunk);
            chunk
        });
        chunk.get(x - c.x * S, y - c.y * S, z - c.z * S)
    }
}

/// The face's shrines in the cells within 6 of the origin's (a 26 km square: the island).
fn shrines_of(g: &BifacialTerrain, face: Face) -> Vec<Shrine> {
    let mut v = g.shrines_round(face, 0, 0, 6);
    v.sort();
    v.dedup();
    v
}

dwell_world::both_faces!(
    /// On face `f`: many shrines, each the same from a fresh generator, about a cell apart (no two
    /// nearer than the jitter allows), each on dry ground in the height band over its whole
    /// footprint, no steeper than the rule, inland and clear of the portals.
    fn shrines_are_sited_from_the_seed_a_grid_apart_on_dry_level_ground(f: FaceFrame) {
        // Seeds 1, 7 and 42 (owner's check for the 16 m shrine, 2026-11-13) and 0.
        for seed in [0u64, 1, 7, 42] {
            let g = BifacialTerrain::new(seed);
            let sites = shrines_of(&g, f.face);
            assert!(sites.len() >= 20, "seed {seed}: {} shrines", sites.len());
            assert_eq!(
                shrines_of(&BifacialTerrain::new(seed), f.face),
                sites,
                "deterministic"
            );
            let t = g.local(f.face);
            let sh = t.shrines().expect("the terrain has shrines");
            let r = sh.rules;
            let least = r.spacing * (1.0 - r.jitter) - 2.0;
            for (i, a) in sites.iter().enumerate() {
                for b in &sites[i + 1..] {
                    let d = f64::from(a.x - b.x).hypot(f64::from(a.z - b.z));
                    assert!(d >= f64::from(least), "{a:?} {b:?} {d:.0} m apart");
                }
                // Within the jitter of its cell's centre.
                let (ci, ck) = sh.cell_of(a.x, a.z);
                let centre =
                    |c: i32| f64::from(c) * f64::from(r.spacing) + f64::from(r.spacing) / 2.0;
                let half = f64::from(r.spacing) * 0.5 * f64::from(r.jitter) + 1.0;
                assert!((f64::from(a.x) - centre(ci)).abs() <= half);
                assert!((f64::from(a.z) - centre(ck)).abs() <= half);
                let (mut lo, mut hi) = (f32::MAX, f32::MIN);
                for (x, z) in sh.feet(*a) {
                    let c = t.column_at(x, z);
                    assert!(
                        !c.outside && c.water == dwell_worldgen::terrain::Water::None,
                        "{a:?} dry"
                    );
                    let above = c.height - SEA_LEVEL_M as f32;
                    assert!(
                        (r.min_height..=r.max_height).contains(&above),
                        "{a:?} {above}"
                    );
                    lo = lo.min(c.height);
                    hi = hi.max(c.height);
                }
                let width = (2 * sh.reach() + 1) as f32;
                assert!(
                    hi - lo <= width * r.max_slope_deg.to_radians().tan() + 1e-3,
                    "{a:?} steep"
                );
                assert!(t.coast_at(a.x, a.z) >= r.inland);
                for p in g.portal_sites() {
                    let d = f64::from(p.x - a.x).hypot(f64::from(p.z - a.z));
                    assert!(d >= f64::from(r.portal_clear), "{a:?} by portal {p:?}");
                }
            }
        }
    }
);

/// Every column of shrine `s`'s structure as generated on face `f`, by generating its chunks.
fn generated(g: &BifacialTerrain, f: FaceFrame, s: Shrine, v: &mut Voxels) -> Vec<StateId> {
    let t = g.local(f.face);
    let sh = t.shrines().expect("shrines");
    let placed = t.shrine(sh, s);
    placed
        .cells()
        .map(|(p, _)| v.at(p[0], f.row(p[1]), p[2]))
        .collect()
}

dwell_world::both_faces!(
    /// On face `f`: the shrine's structure stands at a site, its torches lit at its centre, the same
    /// whichever order its chunks are generated in (its own chunk first, or its neighbours' first).
    fn the_shrine_structure_generates_at_a_site_in_any_chunk_order(f: FaceFrame) {
        let seed = 0;
        let g = BifacialTerrain::new(seed);
        let s = shrines_of(&g, f.face)[3];
        let t = g.local(f.face);
        let sh = t.shrines().expect("shrines");
        let placed = t.shrine(sh, s);
        // Its own chunks first.
        let mut v = Voxels::new(&g);
        let first = generated(&g, f, s, &mut v);
        // Its neighbours' first, on a fresh generator (fresh caches too).
        let g2 = BifacialTerrain::new(seed);
        let mut v2 = Voxels::new(&g2);
        for dz in [-40, 40] {
            for dx in [-40, 40] {
                v2.at(s.x + dx, f.row(placed.min[1]), s.z + dz);
            }
        }
        assert_eq!(generated(&g2, f, s, &mut v2), first);
        // Each cell is the structure's (the ground under a higher column may hold it instead).
        let torch = require("dwell:torch");
        let mut torches = 0;
        for ((p, state), got) in placed.cells().zip(&first) {
            if *got == f.state(state) {
                torches += usize::from(state == torch);
            } else {
                assert!(
                    p[1] <= t.ground_y(p[0], p[2]).expect("ground"),
                    "{p:?} missing"
                );
            }
        }
        assert!(torches >= 1, "its light stands at the centre");
        // Nothing grows on it: no log or leaves over its footprint.
        let (log, leaves) = (require("dwell:log"), require("dwell:leaves"));
        for (x, z) in sh.feet(s) {
            for y in placed.min[1]..placed.max[1] + 8 {
                let m = v.at(x, f.row(y), z);
                assert!(
                    m != f.state(log) && m != f.state(leaves),
                    "a tree at ({x}, {y}, {z})"
                );
            }
        }
    }
);

dwell_world::both_faces!(
    /// The mirror world's face B has face A's shrines, built the same, mirrored (face parity).
    fn the_mirror_worlds_faces_build_the_same_shrines(f: FaceFrame) {
        let g = BifacialTerrain::mirrored(3);
        let a = shrines_of(&g, Face::A);
        assert_eq!(shrines_of(&g, f.face), a);
        let s = a[1];
        let mut va = Voxels::new(&g);
        let mut vf = Voxels::new(&g);
        let ga = generated(&g, FaceFrame::A, s, &mut va);
        let gf = generated(&g, f, s, &mut vf);
        let mirrored: Vec<StateId> = ga.iter().map(|&m| f.state(m)).collect();
        assert_eq!(gf, mirrored);
    }
);

dwell_world::both_faces!(
    /// On face `f`, the shrine as placed at its sites (each turned its own way): 16 × 16 m (owner,
    /// 2026-11-13: "Double the shrine radius, make it 3 times as tall."), pillars 7.5 to 10.5 m, its
    /// pillars standing apart with a full cell of air between neighbours at every height (owner,
    /// 2026-11-13: "Yes, shrine pillars have gaps."), air under the lintel, and the lintel on the
    /// tallest half only, joining their tops over a quarter circle into one arch.
    fn the_shrines_pillars_stand_apart_under_a_lintel_on_the_tallest_half(f: FaceFrame) {
        let g = BifacialTerrain::new(0);
        let t = g.local(f.face);
        let sh = t.shrines().expect("shrines");
        let (stone, torch) = (require("dwell:stone"), require("dwell:torch"));
        let mut turns = std::collections::HashSet::new();
        for s in shrines_of(&g, f.face).into_iter().take(8) {
            turns.insert(s.turn);
            let placed = t.shrine(sh, s);
            let cells: HashMap<[i32; 3], StateId> = placed.cells().collect();
            let [x0, _, z0] = placed.min;
            assert_eq!(placed.max[0] - x0 + 1, 16, "{s:?} 16 m across x");
            assert_eq!(placed.max[2] - z0 + 1, 16, "{s:?} 16 m across z");
            let is_stone = |x: i32, y: i32, z: i32| cells.get(&[x, y, z]) == Some(&stone);
            // Above the dais's top (the torches' row) only the pillars and the lintel stand.
            let dais = cells
                .iter()
                .filter(|(_, st)| **st == torch)
                .map(|(p, _)| p[1])
                .min()
                .expect("torches");
            // The pillars: the 8-connected groups of stone columns at the dais's top.
            let mut cols: Vec<[i32; 2]> = Vec::new();
            for z in z0..z0 + 16 {
                for x in x0..x0 + 16 {
                    if is_stone(x, dais, z) {
                        cols.push([x, z]);
                    }
                }
            }
            let touch =
                |a: [i32; 2], b: [i32; 2]| (a[0] - b[0]).abs().max((a[1] - b[1]).abs()) <= 1;
            let mut pillars: Vec<Vec<[i32; 2]>> = Vec::new();
            for c in cols {
                let near: Vec<usize> = (0..pillars.len())
                    .filter(|&i| pillars[i].iter().any(|&q| touch(q, c)))
                    .collect();
                let mut merged = vec![c];
                for &i in near.iter().rev() {
                    merged.extend(pillars.remove(i));
                }
                pillars.push(merged);
            }
            assert!(pillars.len() >= 3, "{s:?}: {} pillars", pillars.len());
            let top = |p: &Vec<[i32; 2]>| {
                let [x, z] = p[0];
                let mut y = dais;
                while is_stone(x, y, z) {
                    y += 1;
                }
                y
            };
            let pillar_of = |x: i32, z: i32| pillars.iter().position(|p| p.contains(&[x, z]));
            // The tallest half (by top), and the lintel: the stone above the dais off the pillars.
            let mut by_top: Vec<usize> = (0..pillars.len()).collect();
            by_top.sort_by_key(|&i| std::cmp::Reverse(top(&pillars[i])));
            let tall: Vec<usize> = by_top[..pillars.len().div_ceil(2)].to_vec();
            let lintel: Vec<[i32; 3]> = cells
                .iter()
                .filter(|(p, st)| **st == stone && p[1] >= dais && pillar_of(p[0], p[2]).is_none())
                .map(|(p, _)| *p)
                .collect();
            assert!(!lintel.is_empty(), "{s:?} has a lintel");
            // Three times as tall: each pillar 7.5, 9 or 10.5 m over the base it stands on (its
            // column's stone runs from the bottom row to its top), the tallest 10.5 m.
            let base = placed.min[1] + 2;
            let mut heights: Vec<f32> = pillars
                .iter()
                .map(|p| (top(p) - base) as f32 * CELL_H as f32)
                .collect();
            heights.sort_by(f32::total_cmp);
            heights.dedup();
            assert_eq!(heights, vec![7.5, 9.0, 10.5], "{s:?} pillar heights");
            // At least 6 m of air under the lintel, from the floor of its column.
            for &[x, y, z] in &lintel {
                let mut floor = placed.min[1];
                while is_stone(x, floor, z) {
                    floor += 1;
                }
                let under = lintel
                    .iter()
                    .filter(|q| q[0] == x && q[2] == z)
                    .map(|q| q[1])
                    .min()
                    .unwrap_or(y);
                assert!(
                    (under - floor) as f32 * CELL_H as f32 >= 6.0,
                    "{s:?}: {} m under the lintel at {x},{z}",
                    (under - floor) as f32 * CELL_H as f32
                );
            }
            for &[x, y, z] in &lintel {
                // Air under it, down to the dais's top.
                for r in dais..y {
                    assert!(
                        !cells.contains_key(&[x, r, z]) || lintel.contains(&[x, r, z]),
                        "{s:?}: {x},{r},{z} under the lintel"
                    );
                }
                assert!(
                    !is_stone(x, dais, z),
                    "{s:?}: air under the lintel at {x},{z}"
                );
                // Beside the tallest half's pillars only.
                for (i, p) in pillars.iter().enumerate() {
                    if p.iter().any(|&q| touch(q, [x, z])) {
                        assert!(
                            tall.contains(&i),
                            "{s:?}: the lintel at {x},{y},{z} by a lower pillar"
                        );
                    }
                }
            }
            // One arch: lintel and pillar-top cells connected join every tall pillar, and no other.
            let mut seen = std::collections::HashSet::new();
            let p0 = pillars[tall[0]][0];
            let mut stack = vec![[p0[0], top(&pillars[tall[0]]) - 1, p0[1]]];
            while let Some(c) = stack.pop() {
                if !seen.insert(c) {
                    continue;
                }
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        for dx in -1..=1 {
                            let n = [c[0] + dx, c[1] + dy, c[2] + dz];
                            let arch = lintel.contains(&n)
                                || pillar_of(n[0], n[2]).is_some_and(|i| {
                                    is_stone(n[0], n[1], n[2]) && n[1] + 1 >= top(&pillars[i])
                                });
                            if arch && !seen.contains(&n) {
                                stack.push(n);
                            }
                        }
                    }
                }
            }
            for (i, p) in pillars.iter().enumerate() {
                let joined = seen.contains(&[p[0][0], top(p) - 1, p[0][1]]);
                assert_eq!(joined, tall.contains(&i), "{s:?}: pillar {i} joined");
            }
            // The joined tops span a quarter circle about the footprint's centre.
            let (cx, cz) = (f64::from(x0) + 8.0, f64::from(z0) + 8.0);
            let angle = |p: &Vec<[i32; 2]>| {
                let n = p.len() as f64;
                let x = p.iter().map(|q| f64::from(q[0]) + 0.5).sum::<f64>() / n - cx;
                let z = p.iter().map(|q| f64::from(q[1]) + 0.5).sum::<f64>() / n - cz;
                z.atan2(x).to_degrees()
            };
            let mut span: f64 = 0.0;
            for &a in &tall {
                for &b in &tall {
                    let d = (angle(&pillars[a]) - angle(&pillars[b])).rem_euclid(360.0);
                    span = span.max(d.min(360.0 - d));
                }
            }
            assert!(
                (80.0..=100.0).contains(&span),
                "{s:?}: the arch spans {span}°"
            );
        }
        assert!(
            turns.len() >= 2,
            "sites turned more than one way: {turns:?}"
        );
    }
);

dwell_world::both_faces!(
    /// A shrine on a hillside faces out of it: its open half the quarter turn nearest downhill;
    /// on ground flatter than the rule, the seed's turn (owner, 2026-11-12: "rotate it so it faces
    /// out from hillsides").
    fn a_shrine_faces_downhill_and_a_flat_one_takes_the_seeds_turn(f: FaceFrame) {
        let g = BifacialTerrain::new(7);
        let t = g.local(f.face);
        let portals = std::sync::Arc::new(g.portal_sites().to_vec());
        let seed = t.seed();
        let steep = Shrines::new(
            seed,
            ShrineRules {
                flat_below_deg: 0.0,
                ..SHRINES
            },
            portals.clone(),
        );
        let flat = Shrines::new(
            seed,
            ShrineRules {
                flat_below_deg: 45.0,
                ..SHRINES
            },
            portals,
        );
        let h = |x: i32, z: i32| f64::from(t.column_at(x, z).height);
        let mut sloped = 0;
        for s in shrines_of(&g, f.face) {
            let r = steep.reach();
            let (gx, gz) = (
                h(s.x + r, s.z) - h(s.x - r, s.z),
                h(s.x, s.z + r) - h(s.x, s.z - r),
            );
            if gx.hypot(gz) < 0.2 {
                continue;
            }
            sloped += 1;
            let turn = steep.turn_at(t, s.x, s.z);
            let [fx, fz] = Shrine { turn, ..s }.facing();
            let dot = -(f64::from(fx) * gx + f64::from(fz) * gz);
            // Downhill: no other quarter faces further down.
            for q in 0..4 {
                let [ox, oz] = Shrine { turn: q, ..s }.facing();
                assert!(
                    dot >= -(f64::from(ox) * gx + f64::from(oz) * gz),
                    "{s:?} turn {turn}"
                );
            }
            assert!(dot > 0.0, "{s:?} faces downhill");
            // Flat (by the rule): the seed's turn, the same for the same column whatever its slope.
            assert_eq!(flat.turn_at(t, s.x, s.z), flat.turn_at(t, s.x, s.z));
        }
        assert!(sloped >= 5, "{sloped} sloped sites");
        // The seed's turns vary over sites.
        let turns: std::collections::HashSet<u8> = shrines_of(&g, f.face)
            .iter()
            .map(|s| flat.turn_at(t, s.x, s.z))
            .collect();
        assert!(turns.len() >= 3, "{turns:?}");
    }
);

dwell_world::both_faces!(
    /// The shrine nearest a point on face `f` is the nearest of every shrine there, and stands a
    /// player on its floor (its stand column, open air over the feet).
    fn the_nearest_shrine_to_a_death_on_its_face(f: FaceFrame) {
        let g = BifacialTerrain::new(0);
        let all = shrines_of(&g, f.face);
        let mut v = Voxels::new(&g);
        for (x, z) in [(0.0, 0.0), (3_100.0, -1_700.0), (-4_250.5, 2_900.0)] {
            let (s, feet) = g.nearest_shrine(f.face, x, z).expect("a shrine near");
            let d = |s: &Shrine| (f64::from(s.x) + 0.5 - x).hypot(f64::from(s.z) + 0.5 - z);
            let best = all.iter().map(d).fold(f64::MAX, f64::min);
            assert!((d(&s) - best).abs() < 1e-9, "({x}, {z}): {s:?}");
            // Feet in the world on face f: the cell under them solid, the two over them air.
            let local = f.point(feet);
            let row = (local[1] / CELL_H).round() as i32;
            let (cx, cz) = (local[0].floor() as i32, local[2].floor() as i32);
            assert!(info(v.at(cx, f.row(row - 1), cz)).solid, "{s:?} floor");
            for h in row..row + 4 {
                assert_eq!(v.at(cx, f.row(h), cz), M.air, "{s:?} headroom {h}");
            }
        }
    }
);

use dwell_blocks::info;

dwell_world::both_faces!(
    /// Every spawn is at a shrine (owner, 2026-11-12): face `f`'s spawn stands at the shrine nearest
    /// the spawn search's point, which lies within `spawnWithin` of it.
    fn the_spawn_stands_at_the_shrine_nearest_the_spawn_search(f: FaceFrame) {
        for seed in [0u64, 7, 3] {
            let g = BifacialTerrain::new(seed);
            let t = g.local(f.face);
            let sh = t.shrines().expect("shrines");
            let [px, _, pz] = t.spawn_search_point();
            let s = sh.spawn_shrine(t);
            let d = (f64::from(s.x) + 0.5 - px).hypot(f64::from(s.z) + 0.5 - pz);
            assert!(
                d <= f64::from(sh.rules.spawn_within) + 1.0,
                "seed {seed}: {d:.0} m"
            );
            let (near, feet) = g.nearest_shrine(f.face, px, pz).expect("the spawn's");
            assert_eq!(near, s, "seed {seed}");
            let spawn = match f.face {
                Face::A => g.spawn_point(),
                Face::B => g.spawn_point_b(),
            };
            assert_eq!(spawn, feet, "seed {seed}");
        }
    }
);

/// With no grid shrine within reach of the spawn search's point (a rule that sites none), a shrine
/// is built at the point itself, so the spawn always has one.
#[test]
fn a_spawn_without_a_shrine_near_gets_its_own() {
    // face-a-only: the rule is a face's; both faces run the same code (the test above).
    let g = BifacialTerrain::new(0);
    let t = g.local(Face::A);
    let none = Shrines::new(
        t.seed(),
        ShrineRules {
            min_height: 5_000.0,
            max_height: 5_001.0,
            ..SHRINES
        },
        std::sync::Arc::new(Vec::new()),
    );
    let [px, _, pz] = t.spawn_search_point();
    let s = none.spawn_shrine(t);
    assert_eq!((s.x, s.z), (px.floor() as i32, pz.floor() as i32));
    assert_eq!(
        none.nearest(t, px + 300.0, pz),
        Some(s),
        "and a death near it finds it"
    );
    // The chunks there build it: near() lists it.
    assert_eq!(none.near(t, (s.x, s.z), (s.x, s.z)), vec![s]);
}
