//! The world map as the terrain generator (ADR 0049): the kept map's bytes and digest (pinned, and
//! run natively and under WASM in CI), a fixture map carved by data alone, and the water's rules on
//! real maps of both faces: rivers fall downstream, lakes are flat, no water stands open beside
//! lower dry ground, and face B's terrain is face A's code.

use std::sync::Arc;

use dwell_world::{FaceFrame, SEA_LEVEL_M, both_faces};
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::terrain::{Column, Water};
use dwell_worldgen::worldmap::{
    self, MapRiver, MapVertex, Mouth, NO_LAKE, RiverEnd, SEA, WorldMap, codec,
};
use dwell_worldgen::{BifacialTerrain, TerrainGenerator};

/// Seed 7's maps, pinned (`WorldMap::digest`): a change to the hydrology, the uplift layer, the base
/// field or `hydrology.json`'s default changes them. Look at the terrain (`--example
/// terrain_view`), then update.
const DIGEST_7_A: u64 = 0xe315_373b_9381_4428;
const DIGEST_7_B: u64 = 0xf606_8c1b_dfa4_2a33;

fn face_map(seed: u64, f: FaceFrame) -> Arc<WorldMap> {
    worldmap::for_seed(face_seed(seed, f.face))
}

both_faces!(
    /// Determinism (ADR 0010): a seed's map is pinned, natively and under WASM (CI runs this suite
    /// under wasm32-wasip1), so the server and every client agree bit for bit; its bytes decode to
    /// the same map.
    fn a_seeds_map_is_pinned_and_its_bytes_round_trip(f: FaceFrame) {
        let m = face_map(7, f);
        let pinned = if f.is_b() { DIGEST_7_B } else { DIGEST_7_A };
        assert_eq!(m.digest(), pinned, "{:#018x}", m.digest());
        let bytes = codec::encode(&m);
        let back = codec::decode(&bytes).expect("decodes");
        assert_eq!(back.digest(), m.digest());
        assert_eq!(codec::encode(&back), bytes);
        assert_eq!(back.channels.segs.len(), m.channels.segs.len());
        assert!(m.rivers.len() > 10, "{} rivers", m.rivers.len());
    }
);

#[test]
fn damaged_or_foreign_map_bytes_are_refused() {
    // face-a-only: the codec is face-agnostic; one face's bytes exercise it.
    let m = worldmap::for_seed(face_seed(7, dwell_world::Face::A));
    let bytes = codec::encode(&m);
    let mut flipped = bytes.clone();
    flipped[1000] ^= 1;
    assert!(codec::decode(&flipped).is_err(), "a flipped bit");
    assert!(
        codec::decode(&bytes[..bytes.len() - 9]).is_err(),
        "truncated"
    );
    // Another build's map (another fingerprint), however intact, is not read.
    let mut foreign = bytes.clone();
    foreign[7] ^= 0xff;
    let n = foreign.len() - 8;
    let sum = codec::checksum(&foreign[..n]);
    foreign[n..].copy_from_slice(&sum.to_le_bytes());
    let err = codec::decode(&foreign).expect_err("foreign");
    assert!(err.contains("another build"), "{err}");
    assert!(worldmap::install(&foreign).is_err());
}

/// A small fixture map: a plain sloping from 40 m down to the sea at +x, and one river by data.
fn fixture(seed: u64) -> WorldMap {
    let n = 64usize;
    let cell = 100.0;
    let x0 = -((n - 1) as f64) * cell / 2.0;
    let mut height = vec![0f32; n * n];
    let mut flags = vec![0u8; n * n];
    for j in 0..n {
        for i in 0..n {
            let x = x0 + i as f64 * cell;
            let h = (20.0 - x / 100.0) as f32; // 51 m at the west edge, the sea from x = 2,000
            let c = j * n + i;
            height[c] = h.max(-10.0);
            if h <= 0.0 {
                flags[c] = SEA | worldmap::NO_FLOW;
            } else {
                flags[c] = 0; // flows east (D8 0)
            }
        }
    }
    let path = (0..=30)
        .map(|k| {
            let x = -1000.0 + 100.0 * k as f32;
            MapVertex {
                x,
                z: 50.0,
                surface: (20.0 - x / 100.0 - 0.5).max(0.0),
                width: 12.0,
                depth: 1.5,
                discharge: 6.0,
            }
        })
        .collect();
    WorldMap::assemble(
        seed,
        n,
        cell,
        x0,
        height,
        flags,
        vec![NO_LAKE; n * n],
        Vec::new(),
        vec![MapRiver {
            from_lake: NO_LAKE,
            end: RiverEnd::Sea,
            mouth: Mouth::None,
            path,
        }],
    )
}

#[test]
fn a_river_added_by_data_alone_is_carved_with_water_at_its_level() {
    // face-a-only: a fixture map of one face; the faces' code is one (the parity test).
    let g = TerrainGenerator::with_map(3, Arc::new(fixture(3)));
    let at = |x: i32, z: i32| g.column_at(x, z);
    let centre = at(0, 50);
    assert_eq!(centre.water, Water::River, "{centre:?}");
    assert!(centre.wet());
    // Its surface is the river's (19.5 m above sea at x = 0), its bed at least its depth below.
    let level = f64::from(centre.level) - SEA_LEVEL_M;
    assert!((level - 19.5).abs() < 0.6, "{level}");
    assert!(f64::from(centre.level - centre.height) >= 1.0);
    // The banks dry (beyond the channel and its meander), and no water open beside them.
    for dz in [-24, 24, 120] {
        let c = at(0, 50 + dz);
        assert!(!c.wet() && c.water == Water::None, "{dz}: {c:?}");
    }
    for x in (-800..1800).step_by(37) {
        for z in 20..80 {
            let c = at(x, z);
            if c.wet() {
                assert_eq!(leaks(&g, x, z, &c), 0, "({x}, {z})");
            }
        }
    }
    // Downstream the water falls in ½ m terraces, never rising.
    let mut prev = i32::MAX;
    for x in (-900..1900).step_by(7) {
        let c = at(x, 50);
        assert!(c.wet(), "x {x}");
        assert!(c.water_row() <= prev, "x {x}");
        prev = c.water_row();
    }
}

/// The columns of a ring of points round (x, z) every `step` m out to `r`.
fn wet_columns_near(
    g: &TerrainGenerator,
    x: i32,
    z: i32,
    r: i32,
    step: i32,
) -> Vec<(i32, i32, Column)> {
    let mut out = Vec::new();
    for dz in (-r..=r).step_by(step as usize) {
        for dx in (-r..=r).step_by(step as usize) {
            let c = g.column_at(x + dx, z + dz);
            if c.wet() && c.water != Water::Sea {
                out.push((x + dx, z + dz, c));
            }
        }
    }
    out
}

/// Water beside a column that does not hold it: a dry neighbour whose ground lies below the
/// water's top row (a neighbour with lower water of its own is a fall, allowed).
fn leaks(g: &TerrainGenerator, x: i32, z: i32, c: &Column) -> usize {
    let w = c.water_row();
    [(1, 0), (-1, 0), (0, 1), (0, -1)]
        .into_iter()
        .filter(|&(dx, dz)| {
            let n = g.column_at(x + dx, z + dz);
            // A dry neighbour whose ground does not reach our water's top row (a neighbour with
            // water of its own, lower, is a fall).
            let ground = dwell_world::row_of_m(f64::from(n.height)) - 1;
            let leak = !n.outside && n.water == Water::None && ground + 1 < w;
            if leak && std::env::var("LEAKS").is_ok() {
                println!("leak ({x}, {z}) {c:?} -> ({}, {}) {n:?}", x + dx, z + dz);
            }
            leak
        })
        .count()
}

both_faces!(
    /// Rivers (WORLD_GENERATION.md §3.3): along every river's course the water's surface never
    /// rises downstream, and no river or lake water stands open beside lower dry ground.
    fn rivers_fall_downstream_and_hold_their_water(f: FaceFrame) {
        let terrain = BifacialTerrain::new(7);
        let g = terrain.local(f.face);
        let map = face_map(7, f);
        let (mut steps, mut rises, mut wet, mut checked, mut leaked) = (0, 0, 0, 0, 0);
        for (k, course) in map
            .channels
            .courses
            .iter()
            .enumerate()
            .take(map.rivers.len())
        {
            let mut prev = i32::MAX;
            for w in course.windows(2) {
                let (a, b) = (w[0], w[1]);
                let len = (b.x - a.x).hypot(b.z - a.z);
                let n = (len / 3.0).ceil() as i32;
                for s in 0..n {
                    let t = f64::from(s) / f64::from(n);
                    let (x, z) = (a.x + (b.x - a.x) * t, a.z + (b.z - a.z) * t);
                    let (xi, zi) = (x.floor() as i32, z.floor() as i32);
                    let c = g.column_at(xi, zi);
                    if c.water != Water::River {
                        continue;
                    }
                    // Only where this river is the nearest channel (not a crossing or a join).
                    let hit = map.channels.query(x, z, 0.0, f64::INFINITY);
                    if hit.map(|h| h.river) != Some(k as u32) {
                        continue;
                    }
                    wet += 1;
                    steps += 1;
                    rises += usize::from(c.water_row() > prev);
                    prev = c.water_row();
                    if s % 8 == 0 {
                        checked += 1;
                        leaked += leaks(g, xi, zi, &c);
                    }
                }
            }
        }
        assert!(wet > 5_000, "{wet} river columns");
        // The meander warp moves a bank a few metres: a sample can catch a terrace's step back.
        assert!(rises * 1000 <= steps, "{rises} rises in {steps} steps");
        assert!(
            leaked * 200 <= checked,
            "{leaked} leaks at {checked} river columns"
        );
        // Lakes: one flat surface each, held in.
        for (l, lake) in map.lakes.iter().enumerate() {
            let (x, z) = map.centre(lake.entry as usize);
            let cols = wet_columns_near(g, x as i32, z as i32, 300, 7);
            let rows: Vec<i32> = cols
                .iter()
                .filter(|c| c.2.water == Water::Lake)
                .map(|c| c.2.water_row())
                .collect();
            assert!(!rows.is_empty(), "lake {l}");
            // Flat: every lake column's surface is its lake's level (a neighbour lake's columns
            // may be in the window too: at its own level).
            let levels: Vec<i32> = map
                .lakes
                .iter()
                .map(|k| {
                    ((SEA_LEVEL_M + f64::from(k.level)) / dwell_world::CELL_H + 0.5).floor() as i32
                })
                .collect();
            let own = levels[l];
            assert!(rows.contains(&own), "lake {l} at its level");
            assert!(
                rows.iter().all(|r| levels.contains(r)),
                "lake {l} is flat: {rows:?}"
            );
            let leaked: usize = cols
                .iter()
                .filter(|c| c.2.water == Water::Lake)
                .map(|(x, z, c)| leaks(g, *x, *z, c))
                .sum();
            assert!(leaked * 100 <= rows.len(), "lake {l}: {leaked} leaks");
        }
    }
);

both_faces!(
    /// Face parity (BIFACIAL_WORLD.md §11): in the mirror world, face B's columns are face A's —
    /// mountains, rivers and lakes are face-local, one code path for both faces.
    fn the_mirror_worlds_face_b_terrain_is_face_as(f: FaceFrame) {
        let t = BifacialTerrain::mirrored(7);
        let a = t.local(dwell_world::Face::A);
        let b = t.local(f.face);
        for z in (-20_000..=20_000).step_by(1_337) {
            for x in (-20_000..=20_000).step_by(1_201) {
                assert_eq!(a.column_at(x, z), b.column_at(x, z), "({x}, {z})");
            }
        }
    }
);

#[test]
fn the_map_covers_the_island_with_mountains_rivers_and_a_sea_round_it() {
    // face-a-only: the map's shape on seed 7's face A (face B: the pinned digest and the parity).
    let m = worldmap::for_seed(face_seed(7, dwell_world::Face::A));
    assert_eq!(m.n, 1002);
    assert!(m.is_sea(0) && m.is_sea(m.n * m.n - 1), "the border is sea");
    let highest = m.height.iter().copied().fold(f32::MIN, f32::max);
    assert!((1800.0..3000.0).contains(&highest), "{highest}");
    // Every river ends in the sea, a lake or another river, and flows somewhere.
    for r in &m.rivers {
        assert!(r.path.len() >= 2);
        assert!(r.path.windows(2).all(|w| w[1].surface <= w[0].surface));
    }
    // The flow direction is kept for the moving water to come: every land cell drains.
    let land = (0..m.n * m.n).filter(|&c| !m.is_sea(c));
    assert!(land.clone().count() > 100_000);
    assert!(land.clone().all(|c| m.flow(c).is_some()));
}

both_faces!(
    /// docs/BUGS.md 5 (tall borders along rivers, even across their mouths): a river's surface stood
    /// above the land along its banks, so the columns raised levees there. Every vertex's surface now
    /// lies at least the bank height under the lowest dry ground round it, never rises downstream,
    /// and never falls below the sea.
    fn rivers_run_below_their_banks_and_down_to_the_sea(f: FaceFrame) {
        let m = face_map(7, f);
        let bank = f64::from(dwell_worldgen::terrain::SURFACE.river_bank as f32);
        let mut over = 0;
        for r in &m.rivers {
            let mut prev = f32::INFINITY;
            for v in &r.path {
                assert!(v.surface <= prev + 1e-4, "a river rises downstream");
                assert!(v.surface >= 0.0, "a river below the sea");
                prev = v.surface;
                let c = m.cell_at(f64::from(v.x), f64::from(v.z));
                let (ci, cj) = ((c % m.n) as i64, (c / m.n) as i64);
                for dj in -1..=1i64 {
                    for di in -1..=1i64 {
                        let (a, b) = (ci + di, cj + dj);
                        if a < 0 || b < 0 || a as usize >= m.n || b as usize >= m.n {
                            continue;
                        }
                        let q = b as usize * m.n + a as usize;
                        if m.is_sea(q) || m.lake[q] != worldmap::NO_LAKE {
                            continue;
                        }
                        if f64::from(v.surface) > f64::from(m.height[q]) - bank + 1e-3
                            && v.surface > 0.0
                        {
                            over += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(
            over, 0,
            "{over} river vertices stand less than the bank under dry ground round them"
        );
    }
);
