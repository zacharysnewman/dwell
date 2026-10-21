//! The hydrology prototype (docs/proposals/TERRAIN_HYDROLOGY_AND_VOLUMES.md): its invariants over
//! real maps on both faces (water reaches the sea or a lake, lakes are flat to their spill level
//! with one outflow, no river flows uphill), synthetic fixtures for the lake rules, the erosion's
//! effect, the profiles' data, and the determinism digest that CI checks natively and in WASM.

use dwell_world::{FaceFrame, both_faces};
use dwell_worldgen::BifacialTerrain;
use dwell_worldgen::hydrology::{
    self, End, Grid, HydrologyMap, HydrologyParams, NONE, SmallPits, from_heights,
};

/// The default profile on a coarse grid (400 m cells: 252 × 252), so the tests stay quick.
fn coarse() -> HydrologyParams {
    HydrologyParams {
        cell: 400,
        ..*hydrology::default_profile()
    }
}

/// Follows the receivers from every land cell: each path ends at the sea, within the grid's size.
fn assert_water_reaches_the_sea(m: &HydrologyMap) {
    let len = m.grid.len();
    // 0 unknown, 1 reaches the sea.
    let mut known: Vec<u8> = m.sea.iter().map(|&s| u8::from(s)).collect();
    for start in 0..len {
        let mut path = Vec::new();
        let mut c = start;
        while known[c] == 0 {
            path.push(c);
            assert!(path.len() <= len, "a cycle through cell {start}");
            let r = m.receiver[c];
            assert_ne!(r, NONE, "land cell {c} drains nowhere");
            c = r as usize;
        }
        for p in path {
            known[p] = 1;
        }
    }
}

fn assert_lakes_are_flat_with_one_outflow(m: &HydrologyMap, p: &HydrologyParams) {
    for (id, lake) in m.lakes.iter().enumerate() {
        let id = id as u32;
        let cells: Vec<usize> = (0..m.grid.len()).filter(|&i| m.lake_of[i] == id).collect();
        assert_eq!(cells.len() as u32, lake.cells);
        assert!(
            lake.area >= p.lake_min_area && lake.max_depth >= p.lake_min_depth,
            "lake {id} is too small to keep"
        );
        let mut outflows = Vec::new();
        for &c in &cells {
            // Flat: one water surface, over ground below it.
            assert_eq!(
                f64::from(m.surface[c]),
                lake.level as f32 as f64,
                "lake {id} cell {c}"
            );
            assert!(
                m.height[c] <= lake.level as f32,
                "lake {id} cell {c} is dry"
            );
            let r = m.receiver[c] as usize;
            if m.lake_of[r] != id {
                outflows.push((c, r));
            }
            // Filled to its spill level: no dry neighbour lies below the water.
            for (j, _) in m.grid.neighbours(c) {
                if m.lake_of[j] != id && !m.sea[j] {
                    assert!(
                        f64::from(m.height[j]) >= lake.level as f32 as f64,
                        "lake {id} leaks into cell {j}"
                    );
                }
            }
        }
        assert_eq!(
            outflows,
            vec![(lake.entry as usize, lake.outlet as usize)],
            "lake {id}"
        );
        let o = lake.outlet as usize;
        if m.sea[o] {
            assert_eq!(lake.level, 0.0);
        } else {
            assert_eq!(
                m.height[o], lake.level as f32,
                "lake {id}: its outlet is its spill point"
            );
        }
    }
}

fn assert_no_river_flows_uphill(m: &HydrologyMap) {
    for i in 0..m.grid.len() {
        if !m.sea[i] {
            let r = m.receiver[i] as usize;
            assert!(
                m.surface[r] <= m.surface[i],
                "cell {i} drains uphill to {r}"
            );
        }
    }
    for (k, r) in m.rivers.iter().enumerate() {
        assert!(r.path.len() >= 2, "river {k} is a single point");
        for w in r.path.windows(2) {
            assert!(w[1].surface <= w[0].surface, "river {k} rises");
            assert!(w[1].discharge >= w[0].discharge, "river {k} loses water");
        }
        match r.end {
            End::Sea => assert_eq!(r.path.last().unwrap().surface, 0.0),
            End::Lake(id) => assert!((id as usize) < m.lakes.len()),
            End::Joins => {}
        }
    }
}

both_faces!(
    /// A real map's invariants, on each face's own terrain.
    fn water_reaches_the_sea_lakes_are_flat_and_rivers_run_downhill(f: FaceFrame) {
        let p = coarse();
        for seed in [7, 42] {
            let m = hydrology::generate(seed, f, &p);
            assert!(
                m.sea.iter().filter(|s| !**s).count() > 5_000,
                "the island is there"
            );
            assert!(!m.rivers.is_empty(), "seed {seed}: rivers");
            assert_water_reaches_the_sea(&m);
            assert_lakes_are_flat_with_one_outflow(&m, &p);
            assert_no_river_flows_uphill(&m);
        }
    }
);

both_faces!(
    /// Face parity (BIFACIAL_WORLD.md §11): in the mirror world, face B's map is face A's — the
    /// hydrology is face-local, one code path for both faces.
    fn the_mirror_worlds_face_b_map_is_face_as(f: FaceFrame) {
        let g = BifacialTerrain::mirrored(7);
        let a = hydrology::generate_for(&g, FaceFrame::A, &coarse());
        let b = hydrology::generate_for(&g, f, &coarse());
        assert_eq!(a.digest(), b.digest());
    }
);

both_faces!(
    /// Determinism (ADR 0010): the digest of a seed's map is pinned, and CI runs this natively and
    /// under WASM (wasm32-wasip1), so the two builds produce bit-identical maps. A change to the
    /// algorithm or to hydrology.json's default profile changes it: check the images, then update.
    fn a_seeds_map_is_bit_identical_everywhere(f: FaceFrame) {
        let m = hydrology::generate(7, f, &coarse());
        assert_eq!(m.digest(), hydrology::generate(7, f, &coarse()).digest());
        let pinned = if f.is_b() { DIGEST_7_B } else { DIGEST_7_A };
        assert_eq!(m.digest(), pinned, "{:#018x}", m.digest());
    }
);

const DIGEST_7_A: u64 = 0x6088_db94_661b_86c8;
const DIGEST_7_B: u64 = 0x90f6_3376_cc6c_663a;

/// A synthetic island on a `n` × `n` grid of 100 m cells: a cone rising to `peak` m from the coast
/// at `radius` cells, plus `extra(x, z)`.
fn island(n: usize, radius: f64, peak: f64, extra: impl Fn(f64, f64) -> f64) -> (Grid, Vec<f64>) {
    let c = (n as f64 - 1.0) * 0.5;
    let h = (0..n * n)
        .map(|i| {
            let (x, z) = ((i % n) as f64 - c, (i / n) as f64 - c);
            let r = (x * x + z * z).sqrt();
            peak * (1.0 - r / radius) + extra(x, z)
        })
        .collect();
    (Grid { n }, h)
}

fn no_erosion() -> HydrologyParams {
    HydrologyParams {
        cell: 100,
        iterations: 0,
        ..*hydrology::default_profile()
    }
}

#[test]
fn a_crater_is_a_lake_at_its_rims_lowest_point_draining_through_it() {
    // A cone with a bowl 60 m deep and 2 km across on its flank: a lake.
    let (grid, h) = island(101, 45.0, 100.0, |x, z| {
        let d = ((x - 12.0).powi(2) + z * z).sqrt();
        -60.0 * (1.0 - d / 10.0).max(0.0)
    });
    // Rivers from small catchments (the island is small).
    let p = HydrologyParams {
        river_threshold: 0.005,
        ..no_erosion()
    };
    let m = from_heights(grid, 100.0, -5_000.0, &h, 0, &p);
    assert_eq!(m.lakes.len(), 1, "{:?}", m.lakes);
    let lake = &m.lakes[0];
    assert!(lake.max_depth > 10.0 && lake.area > 1e6, "{lake:?}");
    assert_lakes_are_flat_with_one_outflow(&m, &p);
    assert_water_reaches_the_sea(&m);
    assert_no_river_flows_uphill(&m);
    // The outflow is a river from the lake, which reaches the sea.
    let out = m
        .rivers
        .iter()
        .find(|r| r.from_lake == Some(0))
        .expect("the lake's outflow");
    let mut r = out;
    while r.end == End::Joins {
        let last = *r.cells.last().unwrap();
        let next = m.receiver[last as usize];
        r = m.rivers.iter().find(|q| q.cells.contains(&next)).unwrap();
    }
    assert_eq!(r.end, End::Sea);
    // Rivers flow into the lake from upslope.
    assert!(m.rivers.iter().any(|r| r.end == End::Lake(0)));
}

#[test]
fn small_pits_are_filled_or_breached_and_not_kept() {
    // On a gentle cone (0.55 m a cell), a small shallow pit (a few metres, ~0.2 km²) and a tiny
    // deep one (one cell, 30 m).
    let extra = |x: f64, z: f64| {
        let a = ((x - 10.0).powi(2) + z * z).sqrt();
        let b = ((x + 10.0).powi(2) + z * z).sqrt();
        -4.0 * (1.0 - a / 3.0).max(0.0) + if b < 0.5 { -30.0 } else { 0.0 }
    };
    let (grid, h) = island(101, 45.0, 25.0, extra);
    for pits in [SmallPits::Fill, SmallPits::Breach] {
        let p = HydrologyParams {
            small_pits: pits,
            lake_min_area: 5e6,
            ..no_erosion()
        };
        let m = from_heights(grid, 100.0, -5_000.0, &h, 0, &p);
        assert!(m.lakes.is_empty(), "{pits:?}: {:?}", m.lakes);
        assert_water_reaches_the_sea(&m);
        assert_no_river_flows_uphill(&m);
        // Filling raises ground; breaching only lowers it.
        let (mut up, mut down) = (0, 0);
        for (i, &before) in h.iter().enumerate() {
            if m.sea[i] {
                continue;
            }
            let (before, after) = (before as f32, m.height[i]);
            up += usize::from(after > before);
            down += usize::from(after < before);
        }
        match pits {
            SmallPits::Fill => assert!(up > 0 && down == 0, "fill: {up} up, {down} down"),
            // The 30 m pit is deeper than max_breach_depth, so it is filled: one cell up.
            SmallPits::Breach => assert!(down > 0 && up == 1, "breach: {up} up, {down} down"),
        }
    }
}

#[test]
fn erosion_cuts_valleys_along_the_rivers_more_than_elsewhere() {
    // A rough cone: ridges and hollows for the water to find.
    let (grid, h) = island(161, 70.0, 400.0, |x, z| {
        20.0 * ((x * 0.37).sin() * (z * 0.29).cos() + (x * 0.11 + z * 0.13).sin())
    });
    let p = HydrologyParams {
        cell: 100,
        river_threshold: 0.05,
        ..*hydrology::default_profile()
    };
    let m = from_heights(grid, 100.0, -8_000.0, &h, 0, &p);
    let river: std::collections::HashSet<u32> = m
        .rivers
        .iter()
        .flat_map(|r| r.cells.iter().copied())
        .collect();
    assert!(river.len() > 50);
    let (mut on, mut off, mut n_on, mut n_off) = (0.0, 0.0, 0.0, 0.0);
    for i in 0..grid.len() {
        if m.sea[i] || m.lake_of[i] != NONE {
            continue;
        }
        let cut = f64::from(m.original[i]) - f64::from(m.height[i]);
        if river.contains(&(i as u32)) {
            (on, n_on) = (on + cut, n_on + 1.0);
        } else {
            (off, n_off) = (off + cut, n_off + 1.0);
        }
    }
    let (on, off) = (on / n_on, off / n_off);
    assert!(
        on > off + 5.0,
        "rivers cut {on:.1} m on average, elsewhere {off:.1} m"
    );
}

#[test]
fn the_profiles_are_data_and_the_default_is_one_of_them() {
    let names: Vec<&str> = hydrology::PROFILES.iter().map(|p| p.name).collect();
    for want in ["gentle", "medium", "strong"] {
        assert!(names.contains(&want), "{names:?}");
    }
    assert!(hydrology::profile(hydrology::DEFAULT).is_some());
    assert!(hydrology::profile("nope").is_none());
    for p in &hydrology::PROFILES {
        assert!(p.cell > 0 && p.cell % 2 == 0, "{}", p.name);
        assert!(!p.river_geometry.is_empty());
        // The full-disc grid is about a thousand cells across at 100 m.
        let (g, x0) = hydrology::grid_for(p.cell);
        assert!(x0 < -f64::from(dwell_world::WORLD_RADIUS));
        assert!(g.n as i64 * i64::from(p.cell) > 2 * i64::from(dwell_world::WORLD_RADIUS));
    }
}

#[test]
fn the_width_curve_interpolates_and_holds_beyond_its_ends() {
    let curve = hydrology::default_profile().river_geometry;
    let (w0, d0) = hydrology::rivers::geometry(curve, 0.0);
    assert_eq!((w0, d0), (curve[0].width, curve[0].depth));
    let last = curve[curve.len() - 1];
    assert_eq!(
        hydrology::rivers::geometry(curve, last.discharge * 10.0),
        (last.width, last.depth)
    );
    let mid = 0.5 * (curve[0].discharge + curve[1].discharge);
    let (w, _) = hydrology::rivers::geometry(curve, mid);
    assert!((w - 0.5 * (curve[0].width + curve[1].width)).abs() < 1e-9);
}

#[test]
fn the_general_power_path_is_bit_identical_everywhere() {
    // face-a-only: pins the exponents' general path (math.rs pow), which both faces share.
    let p = HydrologyParams {
        area_exponent: 0.45,
        slope_exponent: 1.2,
        iterations: 6,
        ..coarse()
    };
    let m = hydrology::generate(1234, FaceFrame::A, &p);
    assert_eq!(m.digest(), DIGEST_1234_GENERAL, "{:#018x}", m.digest());
}

const DIGEST_1234_GENERAL: u64 = 0x79cf_62ae_ac15_4a31;
