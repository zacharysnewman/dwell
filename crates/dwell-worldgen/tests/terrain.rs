//! The terrain's behaviour beyond the goldens (ported from the C++ worldgen, island and bifacial
//! tests): noise precision, heights, purity, point queries, bounds, the air test, the fill, spawn,
//! and the two faces.

use dwell_blocks::{StateId, info, mirror, require};
use dwell_world::*;
use dwell_worldgen::noise::{fbm2, lattice, perlin2, perlin2f};
use dwell_worldgen::*;

const S: i32 = CHUNK_SIZE;

/// A solid cell (the registry's data): decorations such as tall grass are not.
fn solid(m: StateId) -> bool {
    dwell_blocks::info(m).solid
}

/// What the generator puts over the ground: one of `shared/worldgen/decorations.json`'s blocks.
fn decoration(m: StateId) -> bool {
    dwell_worldgen::decorations::DECORATIONS
        .iter()
        .any(|d| dwell_blocks::require(d.block) == m)
}

/// The block a state is made of: a stored ramp (`dwell:grass_ramp[...]`) is its base material's.
fn material(m: StateId) -> StateId {
    let name = dwell_blocks::block_of(m).name;
    match name.strip_suffix("_ramp") {
        Some(base) => require(base),
        None => m,
    }
}

/// What the generator's structure placements put over the ground (the pines' log and leaves).
fn placed(m: StateId) -> bool {
    m == require("dwell:log") || m == require("dwell:leaves")
}

/// The core's fill at face-local row `y` (from the midplane): the bedrock in its first rows
/// (`shared/worldgen/midplane.json`), stone over it.
fn core_at(y: i32) -> StateId {
    let rows = dwell_worldgen::portals::MIDPLANE.bedrock_rows;
    // face-boundary: a face-local row's height over the midplane (these tests are face A's)
    if y - MIDPLANE_Y < rows {
        require(dwell_worldgen::portals::MIDPLANE.bedrock)
    } else {
        M.stone
    }
}

fn gen_chunk(g: &TerrainGenerator, c: ChunkCoord) -> Chunk {
    let mut chunk = Chunk::default();
    g.generate(c, &mut chunk);
    chunk
}

fn gen_world(g: &BifacialTerrain, c: ChunkCoord) -> Chunk {
    let mut chunk = Chunk::default();
    g.generate(c, &mut chunk);
    chunk
}

/// Where the island's coast crosses the +x axis (the first sea column, 16 m steps).
fn coast_on_axis(g: &TerrainGenerator) -> (i32, i32) {
    let mut x = 0;
    while g.column_at(x, 0).height >= (dwell_world::SEA_LEVEL_M as f32) {
        x += 16;
    }
    (x, 0)
}

#[test]
fn gradient_noise_is_zero_on_the_lattice_bounded_and_continuous() {
    for i in -3..=3 {
        assert_eq!(perlin2f(7, i as f32, (2 * i) as f32), 0.0);
    }
    let (mut lo, mut hi, mut max_step) = (0f32, 0f32, 0f32);
    let mut prev = perlin2f(3, -50.0, 0.7);
    for i in 1..20000 {
        let v = perlin2f(3, -50.0 + i as f32 * 0.005, 0.7);
        lo = lo.min(v);
        hi = hi.max(v);
        max_step = max_step.max((v - prev).abs());
        prev = v;
    }
    assert!(
        lo > -1.1 && hi < 1.1 && hi - lo > 0.8 && max_step < 0.02,
        "{lo} {hi} {max_step}"
    );
    assert_ne!(perlin2f(3, 1.25, -3.75), perlin2f(4, 1.25, -3.75));
}

#[test]
fn noise_is_as_detailed_8000_km_out_as_at_the_origin() {
    const FAR: i64 = 7_999_488;
    for origin in [0, FAR, -FAR] {
        let mut repeats = 0;
        let mut largest = 0f32;
        for i in 0..1024 {
            let a = fbm2(9, origin + i, 17, 28, 3, None);
            let b = fbm2(9, origin + i + 1, 17, 28, 3, None);
            repeats += i32::from(a == b);
            largest = largest.max((b - a).abs());
        }
        assert_eq!(repeats, 0, "{origin}");
        assert!(largest < 0.2, "{origin}: {largest}");
        // The split is exact: the offset of x equals that of x's offset in its cell.
        let l = lattice(origin + 13, 28, 2);
        assert_eq!(l.frac, lattice((origin + 13).rem_euclid(28), 28, 2).frac);
    }
    let _ = perlin2;
}

#[test]
fn fractal_sums_stay_in_range() {
    for i in 0..5000i64 {
        assert!(fbm2(11, i * 37, i * -21, 100, 5, None).abs() <= 1.1);
    }
}

#[test]
fn heights_a_low_shore_land_rising_inland_a_shallow_sea_floor() {
    let g = TerrainGenerator::new(0);
    let (cx, cz) = coast_on_axis(&g);
    let (mut highest, mut lowest) = (f32::MIN, f32::MAX);
    for z in (-30_000..=30_000).step_by(500) {
        for x in (-30_000..=30_000).step_by(500) {
            let h = g.column_at(x, z).height;
            highest = highest.max(h);
            lowest = lowest.min(h);
        }
    }
    // Face-local heights from the midplane: sea level 32, the core's top 10.
    let sea = dwell_world::SEA_LEVEL_M as f32;
    assert!(highest > sea + 300.0 && highest < sea + 650.0, "{highest}");
    assert!(
        lowest >= (dwell_world::CORE_M as f32) && lowest > sea - 30.0,
        "{lowest}"
    );
    assert!((g.column_at(cx, cz).height - sea).abs() < 6.0);
}

#[test]
fn a_chunk_is_a_pure_function_of_seed_and_coordinate() {
    let (a, b, c) = (
        TerrainGenerator::new(123),
        TerrainGenerator::new(123),
        TerrainGenerator::new(124),
    );
    // The chunk at the ground (rows) of column (48, −16), which the seed moves (the hills reach
    // ~500 m).
    let cy = floor_div(a.ground_y(48, -16).expect("inside"), S);
    let coord = ChunkCoord::new(1, cy, -1);
    assert_eq!(gen_chunk(&a, coord).voxels(), gen_chunk(&b, coord).voxels());
    assert_ne!(gen_chunk(&a, coord).voxels(), gen_chunk(&c, coord).voxels());
    let n = ChunkCoord::new(2, cy, -1);
    let mut w1 = VoxelWorld::new(generator_for(GENERATOR_TERRAIN, 123), None);
    let mut w2 = VoxelWorld::new(generator_for(GENERATOR_TERRAIN, 123), None);
    w1.get_or_create(coord);
    w1.get_or_create(n);
    w2.get_or_create(n);
    w2.get_or_create(coord);
    assert_eq!(
        w1.find(coord).map(Chunk::voxels),
        w2.find(coord).map(Chunk::voxels)
    );
    assert_eq!(w1.find(n).map(Chunk::voxels), w2.find(n).map(Chunk::voxels));
}

#[test]
fn point_queries_agree_with_the_chunk_path() {
    let g = TerrainGenerator::new(0);
    let (cx, cz) = coast_on_axis(&g);
    let mut coords = vec![
        ChunkCoord::new(0, 0, 0),
        ChunkCoord::new(0, 2, 0),
        ChunkCoord::new(-1, 1, 3),
        ChunkCoord::new(-1, -3, 3),
    ];
    for (x, z) in [(cx, cz), (cx + 600, cz), (-9000, 4000)] {
        coords.push(chunk_of(x, g.ground_y(x, z).expect("inside"), z));
    }
    for c in coords {
        let chunk = gen_chunk(&g, c);
        for z in (0..S).step_by(3) {
            for y in (0..S).step_by(2) {
                for x in (0..S).step_by(3) {
                    // The point query is the terrain's: placed trees stand over it.
                    if placed(chunk.get(x, y, z)) {
                        continue;
                    }
                    assert_eq!(
                        solid(chunk.get(x, y, z)),
                        g.solid_at(c.x * S + x, c.y * S + y, c.z * S + z),
                        "{c:?} {x},{y},{z}"
                    );
                }
            }
        }
    }
}

#[test]
fn the_core_is_stone_nothing_below_the_face_or_over_the_build_volume() {
    let g = TerrainGenerator::new(5);
    let bottom = MIDPLANE_Y / S;
    // The 10-layer core (ADR 0025 D1) under land, the coast and the open sea.
    for column in [(0, 0), (780, 0), (0, -1400), (1200, 300)] {
        let floor = gen_chunk(&g, ChunkCoord::new(column.0, bottom, column.1));
        for z in 0..S {
            for x in 0..S {
                for y in 0..CORE_LAYERS {
                    assert_eq!(floor.get(x, y, z), core_at(y), "{column:?}");
                }
            }
        }
    }
    for cy in [
        bottom - 1,
        dwell_worldgen::terrain::FACE_MAX_Y / S,
        MAX_CHUNK_Y + 1,
    ] {
        assert!(
            gen_chunk(&g, ChunkCoord::new(0, cy, 0))
                .voxels()
                .iter()
                .all(|&v| v == M.air)
        );
    }
}

#[test]
fn beyond_the_rim_nothing_is_generated() {
    let g = TerrainGenerator::new(5);
    let column = ChunkCoord::new(1104, 0, 1104);
    assert_eq!(chunk_disc_overlap(column.x, column.z), DiscOverlap::Partial);
    let bottom = MIDPLANE_Y / S;
    let (mut inside, mut outside) = (0, 0);
    for cy in [bottom, 1] {
        let chunk = gen_chunk(&g, ChunkCoord::new(column.x, cy, column.z));
        for z in 0..S {
            for x in 0..S {
                let inn =
                    inside_world_disc(i64::from(column.x * S + x), i64::from(column.z * S + z));
                if cy == bottom {
                    if inn { inside += 1 } else { outside += 1 }
                }
                for y in 0..S {
                    let wx = i64::from(column.x * S + x);
                    let wz = i64::from(column.z * S + z);
                    if !inn || !inside_build_volume(wx, i64::from(cy * S + y), wz) {
                        assert_eq!(chunk.get(x, y, z), M.air);
                    } else if cy == bottom && y < CORE_LAYERS {
                        assert_eq!(chunk.get(x, y, z), core_at(y));
                    }
                }
            }
        }
    }
    assert!(inside > 0 && outside > 0);
    let air = air_test_for(GENERATOR_TERRAIN, 5);
    for cy in [bottom, -1, 1, 3] {
        let c = ChunkCoord::new(1563, cy, 0);
        assert_eq!(chunk_disc_overlap(c.x, c.z), DiscOverlap::Outside);
        assert!(gen_chunk(&g, c).voxels().iter().all(|&v| v == M.air));
        assert!(air(c) && g.is_air_chunk(c));
    }
    assert!(g.column_at(1563 * S, 0).outside);
    assert!(!g.solid_at(1563 * S, MIDPLANE_Y, 0));
    assert!(g.solid_at(1560 * S, MIDPLANE_Y, 0));
}

#[test]
fn chunks_the_air_test_calls_air_generate_as_air() {
    for seed in [0u64, 20_260_925] {
        let g = TerrainGenerator::new(seed);
        let air = air_test_for(GENERATOR_TERRAIN, seed);
        let (mut sky, mut checked) = (0, 0);
        for (cx, cz) in [
            (0, 0),
            (5, -3),
            (-14, -36),
            (700, -12),
            (1560, 10),
            (-3, 1550),
        ] {
            let surface = floor_div(
                dwell_world::row_of_m(g.column_at(cx * S, cz * S).height.into()),
                S,
            );
            for cy in (surface - 2).max(0)..=surface + 6 {
                let c = ChunkCoord::new(cx, cy, cz);
                let is_air = air(c);
                assert_eq!(is_air, g.is_air_chunk(c), "{c:?}");
                checked += 1;
                if is_air {
                    sky += 1;
                    assert!(
                        gen_chunk(&g, c).voxels().iter().all(|&v| v == M.air),
                        "{c:?}"
                    );
                }
            }
        }
        assert!(sky > checked / 4);
    }
    let generate = generator_for(GENERATOR_FLAT, 0);
    let air = air_test_for(GENERATOR_FLAT, 0);
    for cy in -3..=2 {
        for cx in -2..=1 {
            let c = ChunkCoord::new(cx, cy, 0);
            if air(c) {
                let mut chunk = Chunk::default();
                generate(c, &mut chunk);
                assert!(chunk.voxels().iter().all(|&v| v == M.air));
            }
        }
    }
    assert!(air(ChunkCoord::new(0, FLAT_TOP_Y / S, 0)));
    assert!(!air(ChunkCoord::new(0, FLAT_TOP_Y / S - 1, 0)));
    assert!(air(ChunkCoord::new(0, -1, 0)));
}

#[test]
fn the_sandbox_is_the_flat_world_in_its_indestructible_floor_block() {
    let floor = require(SANDBOX_FLOOR);
    assert!(info(floor).indestructible && info(floor).solid && !info(floor).placeable);
    let sandbox = generator_for(GENERATOR_SANDBOX, 7);
    let flat = generator_for(GENERATOR_FLAT, 7);
    let air = air_test_for(GENERATOR_SANDBOX, 7);
    for c in [
        (0, 0, 0),
        (0, 1, 0),
        (3, -5, -2),
        (0, MIDPLANE_Y / S - 1, 0),
    ] {
        let c = ChunkCoord::new(c.0, c.1, c.2);
        let (mut a, mut b) = (Chunk::default(), Chunk::default());
        sandbox(c, &mut a);
        flat(c, &mut b);
        // The same shape as the flat world, every solid cell of it the floor.
        for (&s, &f) in a.voxels().iter().zip(b.voxels().iter()) {
            assert_eq!(s, if f == M.air { M.air } else { floor }, "{c:?}");
        }
        if air(c) {
            assert!(a.voxels().iter().all(|&v| v == M.air));
        }
    }
    let mut world = VoxelWorld::new(sandbox, None);
    assert_eq!(world.get_voxel(3, FLAT_TOP_Y - 1, 3), floor);
    assert_eq!(world.get_voxel(3, MIDPLANE_Y, 3), floor);
    assert_eq!(world.get_voxel(3, FLAT_TOP_Y, 3), M.air);
    assert_eq!(world.get_voxel(3, MIDPLANE_Y - 1, 3), M.air);
    // The floor is a full cube (not an adaptive natural block like the flat world's grass, which
    // rests at half height): the spawn stands on top of it, not half a metre inside it.
    assert_eq!(
        spawn_point_for(GENERATOR_SANDBOX, 7),
        [0.5, FLAT_TOP_M, 0.5]
    );
}

#[test]
fn the_flat_world_and_retired_versions_are_unchanged_by_the_sandbox() {
    // The flat world's ground chunk, what every version but terrain and sandbox generates.
    let mut chunk = Chunk::default();
    let ground = ChunkCoord::new(0, FLAT_TOP_Y / S - 1, 0);
    generate_flat_chunk(ground, &mut chunk);
    let flat_hash = chunk_hash(&chunk);
    for version in [GENERATOR_FLAT, 1, 2, 7, 9, 99] {
        let mut c = Chunk::default();
        generator_for(version, 3)(ground, &mut c);
        assert_eq!(chunk_hash(&c), flat_hash, "version {version}");
    }
    assert_eq!(chunk.get(0, 31, 0), M.grass);
    assert_eq!(chunk.get(0, 30, 0), M.dirt);
    assert_eq!(chunk.get(0, 27, 0), M.stone);
}

#[test]
fn the_fill_grass_over_dirt_sand_at_the_shore_sea_to_its_level() {
    let g = TerrainGenerator::new(0);
    let (cx, cz) = coast_on_axis(&g);
    let (mut water, mut grass, mut sand, mut decorated) = (0, 0, 0, 0);
    // Depths and the shore in rows of cells.
    let r = |m: f64| rows_for_m(m);
    for x in (cx - 200..=cx + 200).step_by(40) {
        let h = g.ground_y(x, cz).expect("inside");
        for cy in [floor_div(h, S) - 1, floor_div(h, S), floor_div(h, S) + 1] {
            let c = chunk_of(x, cy * S, cz);
            let chunk = gen_chunk(&g, c);
            for z in 0..S {
                for lx in 0..S {
                    let ground = g.ground_y(c.x * S + lx, c.z * S + z).expect("inside");
                    for y in 0..S {
                        let wy = c.y * S + y;
                        // face-boundary: this test is of face A's generator alone
                        if wy < MIDPLANE_Y {
                            continue; // face B's rows: this is face A's generator
                        }
                        // A stored ramp topping a column is its material, shaped.
                        let m = material(chunk.get(lx, y, z));
                        let depth = ground - wy;
                        if depth == -1 && wy >= SEA_LEVEL && decoration(m) {
                            decorated += 1;
                        } else if depth < 0 && wy >= SEA_LEVEL && placed(m) {
                            // A placed structure's (tests/placements.rs).
                        } else if depth < 0 {
                            assert_eq!(m, if wy < SEA_LEVEL { M.water } else { M.air });
                            water += i32::from(m == M.water);
                        } else if wy < MIDPLANE_Y + CORE_LAYERS {
                            // face-boundary: face A's core
                            assert_eq!(m, core_at(wy)); // the core, on the bedrock
                        } else if ground < SEA_LEVEL + r(2.0) {
                            // Under the sea the top holds it (its flooded state; any block).
                            let m = dwell_blocks::with_property(m, "flooded", "false").unwrap_or(m);
                            assert_eq!(m, if depth < r(3.0) { M.sand } else { M.stone });
                            sand += i32::from(m == M.sand);
                        } else {
                            let want = if depth == 0 {
                                M.grass
                            } else if depth <= r(3.0) {
                                M.dirt
                            } else {
                                M.stone
                            };
                            assert_eq!(m, want);
                            grass += i32::from(m == M.grass);
                        }
                    }
                }
            }
        }
    }
    assert!(
        water > 1000 && grass > 100 && sand > 100 && decorated > 10,
        "{water} {grass} {sand} {decorated}"
    );
}

#[test]
fn players_spawn_on_level_open_ground_near_the_origin() {
    for seed in 0..16u64 {
        let s = spawn_point_for(GENERATOR_TERRAIN, seed);
        let mut world = VoxelWorld::new(generator_for(GENERATOR_TERRAIN, seed), None);
        // The feet stand on the top of a level patch's top cell (row g): no ramp tops it.
        let (x, g, z) = (s[0].floor() as i32, row_of_m(s[1]) - 1, s[2].floor() as i32);
        assert_eq!(s[1], m_of_row(g + 1), "seed {seed}: {s:?}");
        assert!(
            x.abs() < 512 && z.abs() < 512 && g >= SEA_LEVEL,
            "seed {seed}: {s:?}"
        );
        for dz in -4..=4 {
            for dx in -4..=4 {
                assert!(solid(world.get_voxel(x + dx, g, z + dz)), "seed {seed}");
                // Open: air, or a decoration players walk through (tall grass).
                let at = world.get_voxel(x + dx, g + 1, z + dz);
                assert!(at == M.air || (decoration(at) && !solid(at)), "seed {seed}");
                // Headroom: 2 m clear above the plant's cell.
                for row in g + 2..=g + 1 + rows_for_m(2.0) {
                    let above = world.get_voxel(x + dx, row, z + dz);
                    assert!(above == M.air || !solid(above), "seed {seed}");
                }
            }
        }
        assert_eq!(
            world.shape_at(x, g, z).max_y,
            1.0,
            "seed {seed}: a level patch's top is a full cell"
        );
    }
    // The flat world's grass top is its top layer's.
    assert_eq!(spawn_point_for(GENERATOR_FLAT, 9)[1], FLAT_TOP_M);
}

#[test]
fn one_island_at_the_origin_with_sea_all_round() {
    for seed in [0u64, 7, 20_260_925] {
        let g = TerrainGenerator::new(seed);
        assert!(g.coast_at(0, 0) > 0.0, "the origin is land");
        // Sea all round at 40 km, land inside 15 km.
        for a in 0..32 {
            let t = a as f64 * std::f64::consts::TAU / 32.0;
            let (c, s) = (t.cos(), t.sin());
            assert!(
                g.coast_at((40_000.0 * c) as i32, (40_000.0 * s) as i32) < 0.0,
                "seed {seed}"
            );
            assert!(
                g.coast_at((12_000.0 * c) as i32, (12_000.0 * s) as i32) > 0.0,
                "seed {seed}"
            );
        }
    }
}

#[test]
fn the_mirror_maps_voxels_and_chunks_and_is_its_own_inverse() {
    // The midplane at y = 0 (ADR 0025 D9): face B's h = −1 − y.
    assert_eq!(MIDPLANE_Y, 0);
    assert_eq!(
        (mirror_y(0), mirror_y(-1), mirror_y(31), mirror_y(-33)),
        (-1, 0, -32, 32)
    );
    assert_eq!((mirror_chunk_y(0), mirror_chunk_y(-1)), (-1, 0));
    for y in [-50_000, -5000, -33, -1, 0, 32, 49_999] {
        assert_eq!(mirror_y(mirror_y(y)), y);
        let cy = floor_div(y, S);
        let local = mirror_y(y);
        assert!(mirror_chunk_y(cy) * S <= local && local < (mirror_chunk_y(cy) + 1) * S);
    }
    assert_eq!((face_of_y(0), face_of_y(-1)), (Face::A, Face::B));
    let (lo, hi) = BUILD_VOLUME.y_range();
    assert_eq!(i64::from(mirror_y(lo as i32)), hi - 1);
    assert_eq!((MIN_CHUNK_Y, MAX_CHUNK_Y), (-1563, 1562));
    assert_eq!(mirror_chunk_y(MIN_CHUNK_Y), MAX_CHUNK_Y);
    assert_eq!(
        face_local_chunk(ChunkCoord::new(3, -70, -4)),
        ChunkCoord::new(3, 69, -4)
    );
    // The build sphere is symmetric under the mirror: a chunk and its face-local chunk agree.
    for c in [
        (1562, -1, 0),
        (0, -1563, 0),
        (1104, -2, 1104),
        (1560, -89, 0),
    ] {
        let c = ChunkCoord::new(c.0, c.1, c.2);
        assert_eq!(
            BUILD_VOLUME.chunk_overlap(c),
            BUILD_VOLUME.chunk_overlap(face_local_chunk(c))
        );
    }
    for m in 0..dwell_blocks::STATES.len() as StateId {
        assert_eq!(mirror(mirror(m)), m);
    }
}

#[test]
fn a_chunk_of_face_b_is_the_flip_of_its_face_local_chunk() {
    for seed in [0u64, 20_260_925] {
        let g = BifacialTerrain::new(seed);
        for c in [
            (0, -1, 0),
            (4, -2, -7),
            (-3, -1, 5),
            (0, -2, 0),
            (9, -1, 2),
            (780, -1, 0),
        ] {
            let c = ChunkCoord::new(c.0, c.1, c.2);
            let flipped = gen_world(&g, c);
            let local = gen_chunk(g.local(Face::B), face_local_chunk(c));
            let mut solid_count = 0;
            for z in 0..S {
                for y in 0..S {
                    for x in 0..S {
                        let expect = mirror(local.get(x, S - 1 - y, z));
                        assert_eq!(flipped.get(x, y, z), expect, "{c:?} {x},{y},{z}");
                        solid_count += i32::from(solid(expect));
                    }
                }
            }
            assert!(solid_count > 0, "{c:?}");
        }
    }
}

#[test]
fn face_b_is_its_own_landscape() {
    for seed in [0u64, 5] {
        let g = BifacialTerrain::new(seed);
        assert_ne!(
            bifacial::face_seed(seed, Face::A),
            bifacial::face_seed(seed, Face::B)
        );
        let (mut differing, mut columns) = (0, 0);
        for i in -20..=20 {
            for j in -20..=20 {
                let (a, b) = (
                    g.local(Face::A).column_at(i * 1500, j * 1500),
                    g.local(Face::B).column_at(i * 1500, j * 1500),
                );
                let sea = dwell_world::SEA_LEVEL_M as f32;
                if a.outside || b.outside || (a.height < sea && b.height < sea) {
                    continue;
                }
                columns += 1;
                differing += i32::from((a.height - b.height).abs() > 8.0);
            }
        }
        assert!(
            columns > 500 && differing > columns / 2,
            "{columns} {differing}"
        );
    }
}

#[test]
fn the_bedrock_lies_either_side_of_the_midplane_and_no_edit_breaks_it() {
    // both-faces: the chunks checked straddle the midplane, each row on its own face.
    // Owner, 2026-10-21: bedrock back between the faces (midplane.json `bedrock`): its rows on
    // each face are the indestructible bedrock, the core's stone over them.
    let rows = dwell_worldgen::portals::MIDPLANE.bedrock_rows;
    assert!(rows > 0);
    let bedrock = require(dwell_worldgen::portals::MIDPLANE.bedrock);
    assert!(info(bedrock).indestructible && dwell_world::edit::unbreakable(bedrock));
    for seed in [0u64, 3] {
        let g = BifacialTerrain::new(seed);
        for c in [(0, 0, 0), (0, -1, 0), (200, 0, -30), (200, -1, -30)] {
            let chunk = gen_world(&g, ChunkCoord::new(c.0, c.1, c.2));
            for z in 0..S {
                for x in 0..S {
                    for ly in 0..S {
                        let y = c.1 * S + ly;
                        // Face-local row from the midplane, on either face.
                        let h = FaceFrame::at_row(y).row(y) - MIDPLANE_Y;
                        let want = if h < rows {
                            bedrock
                        } else if h < CORE_LAYERS {
                            M.stone
                        } else {
                            continue;
                        };
                        assert_eq!(chunk.get(x, ly, z), want, "{c:?} {x} {ly} {z}");
                    }
                }
            }
        }
        for (x, z) in [
            (0, 0),
            (32 * 200 + 5, -30 * 32 + 3),
            (-32 * 12 + 9, 40 * 32 + 1),
        ] {
            assert!(g.solid_at(x, -1, z) && g.solid_at(x, 0, z));
        }
    }
}

#[test]
fn point_queries_by_face_agree_with_the_chunk_path() {
    let g = BifacialTerrain::new(11);
    for c in [(0, -2, 0), (5, -1, 5), (0, -3, 0), (0, -4, 0), (780, -1, 0)] {
        let c = ChunkCoord::new(c.0, c.1, c.2);
        let chunk = gen_world(&g, c);
        for z in (0..S).step_by(3) {
            for y in 0..S {
                for x in (0..S).step_by(3) {
                    assert_eq!(
                        g.solid_at(c.x * S + x, c.y * S + y, c.z * S + z),
                        solid(chunk.get(x, y, z)),
                        "{c:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn the_air_test_knows_face_bs_sky_and_rows_outside_the_world() {
    let seed = 5;
    let air = air_test_for(GENERATOR_TERRAIN, seed);
    let g = BifacialTerrain::new(seed);
    let (mut sky, mut checked) = (0, 0);
    for cx in -3..=3 {
        for cz in -3..=3 {
            for cy in [1, 0, -1, -2, -3, -4, -6, -100, MIN_CHUNK_Y, MIN_CHUNK_Y - 1] {
                let c = ChunkCoord::new(cx * 300, cy, cz * 313);
                let is_air = air(c);
                checked += 1;
                sky += i32::from(is_air);
                assert_eq!(is_air, g.is_air_chunk(c), "{c:?}");
                if is_air {
                    assert!(
                        gen_world(&g, c).voxels().iter().all(|&m| m == M.air),
                        "{c:?}"
                    );
                }
            }
        }
    }
    assert!(sky > 0 && sky < checked);
    for cy in [-24, -100, MIN_CHUNK_Y] {
        assert!(air(ChunkCoord::new(0, cy, 0)) && air(ChunkCoord::new(300, cy, -300)));
    }
    assert!(
        air(ChunkCoord::new(0, MIN_CHUNK_Y - 1, 0)) && air(ChunkCoord::new(0, MAX_CHUNK_Y + 1, 0))
    );
    let _ = require("dwell:air");
}

#[test]
fn meadows_of_tall_grass_grow_on_grass_by_the_decoration_data() {
    // shared/worldgen/decorations.json: each plant stands in the air cell over a grass block,
    // within the rule's slope and height; a cell's answer is its own (no other chunk is read), so
    // chunk borders agree and chunks generate in any order.
    let g = TerrainGenerator::new(7);
    let rule = &dwell_worldgen::decorations::DECORATIONS[0];
    let tall = require(rule.block);
    let (mut plants, mut grass_tops) = (0, 0);
    for (cx, cz) in [(0, 0), (1, 0), (0, 1), (-3, 5), (12, -7)] {
        let h = g.ground_y(cx * S + 16, cz * S + 16).expect("inside");
        for cy in [floor_div(h, S), floor_div(h, S) + 1] {
            let c = ChunkCoord::new(cx, cy, cz);
            let chunk = gen_chunk(&g, c);
            assert_eq!(chunk.voxels(), gen_chunk(&g, c).voxels());
            for z in 0..S {
                for x in 0..S {
                    let (wx, wz) = (c.x * S + x, c.z * S + z);
                    let top = g.ground_y(wx, wz).expect("inside");
                    if (c.y * S..c.y * S + S).contains(&top)
                        && chunk.get(x, top - c.y * S, z) == M.grass
                    {
                        grass_tops += 1;
                    }
                    for y in 0..S {
                        if chunk.get(x, y, z) != tall {
                            continue;
                        }
                        plants += 1;
                        assert_eq!(c.y * S + y, top + 1, "over the ground");
                        // The rule is in metres, the grid in rows.
                        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                            let n = g.ground_y(wx + dx, wz + dz).expect("inside");
                            assert!((n - top).abs() <= rows_for_m(f64::from(rule.max_slope)));
                        }
                        // The rule's heights are metres above sea level.
                        let (lo, hi) = (rule.min_height, rule.max_height);
                        let band = rows_for_m(lo.into())..=rows_for_m(hi.into());
                        assert!(band.contains(&(top - SEA_LEVEL)));
                    }
                }
            }
        }
    }
    // Meadows, not a lawn: some of the grass, not all of it.
    assert!(
        plants > 20 && plants < grass_tops,
        "{plants} of {grass_tops}"
    );
}

#[test]
fn generated_ramps_follow_the_rule_across_chunk_borders_and_hold_the_sea_at_the_shore() {
    // HALF_HEIGHT_GRID.md §4: each natural top is the rule's answer over its 9 columns' heights,
    // whichever chunk asks (the strip crosses chunk borders every 32 m), and a ramp whose cell is
    // below sea level is flooded (WATER_RENDERING.md §9: regression, owner 2026-10-20 — a ramp in
    // the sea's top row was dry, with the water piled beside it). A world generates the chunks;
    // the heights come from the point query.
    use dwell_blocks::state_property;
    use dwell_worldgen::ramps::{Around, Top, top_of};
    let seed = 0;
    let g = TerrainGenerator::new(seed);
    let mut world = VoxelWorld::new(generator_for(GENERATOR_TERRAIN, seed), None);
    let (cx, cz) = coast_on_axis(&g);
    let (mut gentle, mut steep, mut flooded, mut on_borders) = (0, 0, 0, 0);
    // From 3 km inland (hills) out past the coast, across the chunk border at z = 0.
    for z in cz - 8..cz + 8 {
        for x in cx - 3000..cx + 40 {
            let top = g.ground_y(x, z).expect("inside");
            let around: Around = std::array::from_fn(|r| {
                std::array::from_fn(|c| {
                    g.ground_y(x + c as i32 - 1, z + r as i32 - 1)
                        .expect("inside")
                        + 1
                })
            });
            let m = world.get_voxel(x, top, z);
            let shape = state_property(m, "shape");
            let facing = state_property(m, "facing");
            match top_of(&around, rows_for_m(1.0)) {
                Top::Flat => assert_eq!(shape, None, "({x}, {z})"),
                Top::Gentle(f) => {
                    assert_eq!(
                        (shape, facing),
                        (Some("ramp"), Some(f.name())),
                        "({x}, {z})"
                    );
                    gentle += 1;
                }
                Top::Steep(f) => {
                    let below = world.get_voxel(x, top - 1, z);
                    assert_eq!((shape, facing), (Some("steep_upper"), Some(f.name())));
                    assert_eq!(state_property(below, "shape"), Some("steep_lower"));
                    steep += 1;
                }
            }
            if shape.is_some() {
                let wet = top < SEA_LEVEL;
                assert_eq!(
                    state_property(m, "flooded"),
                    Some(if wet { "true" } else { "false" })
                );
                flooded += i32::from(wet);
                on_borders += i32::from(x.rem_euclid(S) == 0 || x.rem_euclid(S) == S - 1);
            }
        }
    }
    // The steep hills (a column 1 m above its downhill neighbour) make 1 m ramps too.
    assert!(
        gentle > 100 && steep > 100 && flooded > 10 && on_borders > 10,
        "{gentle} {steep} {flooded} {on_borders}"
    );
}

#[test]
fn a_decorations_height_band_is_metres_above_sea_level() {
    // decorations.json gives `height` in metres above sea level, but the generator compared it
    // with the face-local top (sea level 32 since Phase 16), so meadow grass grew from 29 m below
    // its band's foot and stopped 32 m short of its top. Here: the grass at the shore's top edge
    // (2 m above sea level, below the band's 3 m) along the coast of three seeds gets none.
    let rule = &dwell_worldgen::decorations::DECORATIONS[0];
    let tall = require(rule.block);
    let (mut low, mut planted) = (0, 0);
    for seed in [1u64, 7, 42] {
        let g = TerrainGenerator::new(seed);
        for ray in 0..12 {
            let a = f64::from(ray * 30).to_radians();
            // The first sea column out from 15 km, in 16 m steps.
            let mut r = 15_000.0;
            let at = |r: f64| ((r * a.cos()) as i32, (r * a.sin()) as i32);
            while f64::from(g.column_at(at(r).0, at(r).1).height) >= SEA_LEVEL_M && r < 40_000.0 {
                r += 16.0;
            }
            let (x, z) = at(r);
            for dz in -2..=2 {
                for dx in -2..=2 {
                    let (cx, cz) = (floor_div(x, S) + dx, floor_div(z, S) + dz);
                    let cy = floor_div(SEA_LEVEL + 3, S);
                    let chunk = gen_chunk(&g, ChunkCoord::new(cx, cy, cz));
                    for lz in 0..S {
                        for lx in 0..S {
                            let top = g.ground_y(cx * S + lx, cz * S + lz).expect("inside");
                            if top - SEA_LEVEL >= rows_for_m(rule.min_height.into())
                                || g.state_at(cx * S + lx, top, cz * S + lz) != M.grass
                                || !(cy * S..cy * S + S).contains(&(top + 1))
                            {
                                continue;
                            }
                            low += 1;
                            planted += usize::from(chunk.get(lx, top + 1 - cy * S, lz) == tall);
                        }
                    }
                }
            }
        }
    }
    assert!(low > 200, "{low} low grass cells checked");
    assert_eq!(
        planted, 0,
        "{planted} of {low} grass cells below the band have a plant"
    );
}

#[test]
fn the_island_keeps_its_phase_15_size_in_the_50_km_world() {
    // Owner, 2026-10-15: shrinking the world to 50 km must not shrink the island. Its mean coast
    // lay 24.8–25.7 km out over seeds before the resize (`--example island`): within ±10% of that,
    // and well clear of the rim.
    for seed in [1u64, 42, 12345] {
        let g = TerrainGenerator::new(seed);
        let mut sum = 0.0;
        for ray in 0..72 {
            let a = f64::from(ray * 5).to_radians();
            let mut last_land = 0.0;
            let mut r = 0.0;
            while r < 45_000.0 {
                if g.coast_at((r * a.cos()) as i32, (r * a.sin()) as i32) > 0.0 {
                    last_land = r;
                }
                r += 200.0;
            }
            assert!(last_land < f64::from(WORLD_RADIUS) - 8_000.0, "seed {seed}");
            sum += last_land;
        }
        let mean = sum / 72.0;
        assert!((mean - 25_000.0).abs() < 2_500.0, "seed {seed}: {mean}");
    }
}

#[test]
fn the_hills_are_steep_enough_to_slide_and_reach_about_500_m() {
    // Owner, 2026-10-19: "I want terrain steep enough to slide on" — about 15 % of the land's
    // surface steeper than the slide's 44° (on the half-height branch, the land columns topped by
    // a 45° ramp, HALF_HEIGHT_GRID.md §4), everywhere, and peaks up to about 500 m above sea level
    // (generator version 16, `shared/worldgen/terrain.json`). Land columns on a 499 m grid over
    // three seeds, both faces; each column's shaped top block sampled at four points, as
    // `--example relief` does. Version 15 measured ~0.01 % sliding, a highest column ~190 m and a
    // mean slope of ~5.9°; version 16 ~15 %, ~500 m and ~22°, with the island under water
    // (hollows below sea level) still under 1.5 %. Half-height generator 1019 (option A: no 1 m
    // step snap, the Sobel facing) slides on ~10.6 % (A) where 1018's steps slid on ~12.9 %: the
    // band's floor is 9 % on this branch. Generator 1020 (V2, the owner's choice: the old ramp rule, no 1 m snap) slides on ~3.2 % (A),
    // ~3.0 % (B): the old rule makes few 45° ramps, so the band is 2–5 % on this branch.
    use dwell_worldgen::ramps::{Around, Top, top_of};
    for face in [Face::A, Face::B] {
        let (mut highest, mut slope, mut land) = (0.0f64, 0.0f64, 0usize);
        let (mut samples, mut slides, mut island, mut flooded) = (0usize, 0usize, 0usize, 0usize);
        for seed in [1u64, 42, 12345] {
            let terrain = BifacialTerrain::new(seed);
            let g = terrain.local(face);
            for z in (-30_000..=30_000).step_by(499) {
                for x in (-30_000..=30_000).step_by(499) {
                    let col = g.column_at(x, z);
                    let h = f64::from(col.height) - SEA_LEVEL_M;
                    if col.coast > 0.0 {
                        island += 1;
                        flooded += usize::from(h <= 0.0);
                    }
                    if col.outside || h <= 0.0 {
                        continue;
                    }
                    land += 1;
                    highest = highest.max(h);
                    let dx = g.column_at(x + 1, z).height - g.column_at(x - 1, z).height;
                    let dz = g.column_at(x, z + 1).height - g.column_at(x, z - 1).height;
                    slope += f64::from((dx * 0.5).hypot(dz * 0.5)).atan().to_degrees();
                    // The top's stored piece, by the generation rule over its 3 × 3 columns
                    // (HALF_HEIGHT_GRID.md §4): only the 45° pieces slide.
                    let around: Around = std::array::from_fn(|j| {
                        std::array::from_fn(|i| {
                            g.ground_y(x + i as i32 - 1, z + j as i32 - 1)
                                .expect("inside")
                                + 1
                        })
                    });
                    samples += 1;
                    slides +=
                        usize::from(matches!(top_of(&around, rows_for_m(1.0)), Top::Steep(_)));
                }
            }
        }
        let sliding = slides as f64 / samples as f64;
        let slope = slope / land as f64;
        let wet = flooded as f64 / island as f64;
        let at = format!(
            "{face:?}: sliding {:.2} %, highest {highest:.1} m, slope {slope:.2}°, wet {:.2} %",
            100.0 * sliding,
            100.0 * wet
        );
        println!("{at}");
        assert!((0.02..0.05).contains(&sliding), "{at}");
        assert!((430.0..580.0).contains(&highest), "{at}");
        assert!((15.0..30.0).contains(&slope), "{at}");
        assert!(wet < 0.015, "{at}");
    }
}

/// The generated world (face A) and where the island's coast crosses the four axes.
fn shore_world(seed: u64) -> (VoxelWorld, Vec<(i32, i32)>) {
    let g = TerrainGenerator::new(seed);
    let mut coasts = Vec::new();
    for (dx, dz) in [(1, 0), (0, 1), (-1, 0), (0, -1)] {
        let mut d = 0;
        while g
            .ground_y(d * dx, d * dz)
            .is_some_and(|t| t >= SEA_LEVEL - 2)
        {
            d += 8;
        }
        coasts.push((d * dx, d * dz));
    }
    let world = VoxelWorld::new(
        generator_for(GENERATOR_TERRAIN, seed),
        Some(air_test_for(GENERATOR_TERRAIN, seed)),
    );
    (world, coasts)
}

/// The water's drawn top in the sea's top row (cells): sea level less the surface drop.
fn drawn_sea_top() -> f64 {
    let drop = dwell_blocks::require_fluid("dwell:water").surface_drop;
    f64::from(SEA_LEVEL) - drop
}

/// A solid cell's shape that leaves part of the cell open: a slope, a ramp or a slab — derived
/// (main's adaptive tops) or stored (the half-height grid's ramps).
fn open_shape(s: &dwell_blocks::ShapeInfo) -> bool {
    s.face_count > 0 && (s.fan || s.corners != [2; 4])
}

#[test]
fn the_sea_floods_the_open_part_of_every_shaped_cell_below_sea_level() {
    //! Regression (owner, 2026-10-20: water piled up against the beach): the generator filled
    //! only empty cells below sea level, so a shaped top in the sea's top row (a ramp, a slope)
    //! was dry, its open part air under the water line, with the water beside it standing above
    //! it. Every solid cell below sea level whose shape leaves part of it open holds the sea there
    //! now (WATER_RENDERING.md §9).
    for seed in [0, 7] {
        let (mut w, coasts) = shore_world(seed);
        let (mut checked, mut dry) = (0, Vec::new());
        for &(cx, cz) in &coasts {
            for z in cz - 24..cz + 24 {
                for x in cx - 24..cx + 24 {
                    for y in SEA_LEVEL - 4..SEA_LEVEL {
                        let m = w.get_voxel(x, y, z);
                        if !info(m).solid || !open_shape(w.shape_at(x, y, z)) {
                            continue;
                        }
                        checked += 1;
                        if !w.flooded_at(x, y, z) {
                            dry.push((x, y, z));
                        }
                    }
                }
            }
        }
        assert!(checked > 200, "seed {seed}: {checked}");
        assert!(
            dry.is_empty(),
            "seed {seed}: {} of {checked} shaped cells below sea level are dry, e.g. {:?}",
            dry.len(),
            &dry[..dry.len().min(5)]
        );
    }
}

#[test]
fn no_shore_column_has_drawn_water_higher_than_the_dry_land_beside_it() {
    //! The rule of WATER_RENDERING.md §9: wherever drawn water (a water cell's or a flooded
    //! cell's, in the sea's top row) is beside a dry column, that column's surface on the side
    //! they share is at or above the water's.
    let top = drawn_sea_top();
    for seed in [0, 7] {
        let (mut w, coasts) = shore_world(seed);
        let water = require("dwell:water");
        let (mut pairs, mut piled) = (0, Vec::new());
        for &(cx, cz) in &coasts {
            for z in cz - 24..cz + 24 {
                for x in cx - 24..cx + 24 {
                    let y = SEA_LEVEL - 1;
                    let m = w.get_voxel(x, y, z);
                    let wet = m == water || (m != 0 && w.flooded_at(x, y, z));
                    if !wet {
                        continue;
                    }
                    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                        let (nx, nz) = (x + dx, z + dz);
                        // The neighbour column's top cell, in this row or above.
                        let mut ny = y;
                        while info(w.get_voxel(nx, ny + 1, nz)).solid {
                            ny += 1;
                        }
                        let n = w.get_voxel(nx, ny, nz);
                        if !info(n).solid || w.flooded_at(nx, ny, nz) {
                            continue;
                        }
                        pairs += 1;
                        let shape = w.shape_at(nx, ny, nz);
                        // Its side toward the water (profiles +X −X +Z −Z, in quarters).
                        let side = match (dx, dz) {
                            (1, 0) => 1,
                            (-1, 0) => 0,
                            (0, 1) => 3,
                            _ => 2,
                        };
                        let low = shape.sides[side].iter().copied().min().unwrap_or(0);
                        let land = f64::from(ny) + f64::from(low) / 4.0;
                        if land < top - 1e-6 {
                            piled.push(((x, z), (nx, ny, nz), land));
                        }
                    }
                }
            }
        }
        assert!(pairs > 100, "seed {seed}: {pairs}");
        assert!(
            piled.is_empty(),
            "seed {seed}: {} of {pairs} shore pairs have the water at {top} above the land, e.g. {:?}",
            piled.len(),
            &piled[..piled.len().min(5)]
        );
    }
}
