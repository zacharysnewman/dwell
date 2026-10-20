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

/// What the generator's structure placements put over the ground (the pines' log and leaves).
fn placed(m: StateId) -> bool {
    m == require("dwell:log") || m == require("dwell:leaves")
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
    while g.column_at(x, 0).height >= SEA_LEVEL as f32 {
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
    let sea = SEA_LEVEL as f32;
    assert!(highest > sea + 300.0 && highest < sea + 650.0, "{highest}");
    assert!(
        lowest >= CORE_LAYERS as f32 && lowest > sea - 30.0,
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
    // The row holding seed 123's ground there (the hills reach ~500 m), and its neighbour.
    let cy = floor_div(a.column_at(48, -16).height as i32, S);
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
        coords.push(chunk_of(x, g.column_at(x, z).height as i32, z));
    }
    for c in coords {
        let chunk = gen_chunk(&g, c);
        for z in (0..S).step_by(3) {
            for y in (0..S).step_by(2) {
                for x in (0..S).step_by(3) {
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
                    assert_eq!(floor.get(x, y, z), M.stone, "{column:?}");
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
                        assert_eq!(chunk.get(x, y, z), M.stone);
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
            let surface = floor_div(g.column_at(cx * S, cz * S).height as i32, S);
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
        [0.5, f64::from(FLAT_TOP_Y), 0.5]
    );
}

#[test]
fn the_flat_world_and_retired_versions_are_unchanged_by_the_sandbox() {
    // The flat world's ground chunk, what every version but terrain and sandbox generates.
    let mut chunk = Chunk::default();
    generate_flat_chunk(ChunkCoord::new(0, 0, 0), &mut chunk);
    let flat_hash = chunk_hash(&chunk);
    for version in [GENERATOR_FLAT, 1, 2, 7, 9, 99] {
        let mut c = Chunk::default();
        generator_for(version, 3)(ChunkCoord::new(0, 0, 0), &mut c);
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
    for x in (cx - 200..=cx + 200).step_by(40) {
        let h = g.column_at(x, cz).height as i32;
        for cy in [floor_div(h, S) - 1, floor_div(h, S), floor_div(h, S) + 1] {
            let c = chunk_of(x, cy * S, cz);
            let chunk = gen_chunk(&g, c);
            for z in 0..S {
                for lx in 0..S {
                    let ground = g.ground_y(c.x * S + lx, c.z * S + z).expect("inside");
                    for y in 0..S {
                        let wy = c.y * S + y;
                        if wy < MIDPLANE_Y {
                            continue; // face B's rows: this is face A's generator
                        }
                        let m = chunk.get(lx, y, z);
                        let depth = ground - wy;
                        if depth == -1 && wy >= SEA_LEVEL && decoration(m) {
                            decorated += 1;
                        } else if depth < 0 && wy >= SEA_LEVEL && placed(m) {
                            // A placed structure's (tests/placements.rs).
                        } else if depth < 0 {
                            assert_eq!(m, if wy < SEA_LEVEL { M.water } else { M.air });
                            water += i32::from(m == M.water);
                        } else if wy < MIDPLANE_Y + CORE_LAYERS {
                            assert_eq!(m, M.stone); // the core
                        } else if ground < SEA_LEVEL + 2 {
                            // The top voxel under the sea holds it (its flooded state).
                            let sand_here = if depth == 0 && wy < SEA_LEVEL {
                                require("dwell:sand[flooded=true]")
                            } else {
                                M.sand
                            };
                            assert_eq!(m, if depth < 3 { sand_here } else { M.stone });
                            sand += i32::from(m == sand_here);
                        } else {
                            let want = if depth == 0 {
                                M.grass
                            } else if depth <= 3 {
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
        // The feet stand on the shaped surface: a level patch's top blocks rest at half height
        // (ADAPTIVE_BLOCKS.md), half a block above the top voxel's floor.
        let (x, g, z) = (
            s[0].floor() as i32,
            s[1].floor() as i32,
            s[2].floor() as i32,
        );
        assert_eq!(s[1], f64::from(g) + 0.5, "seed {seed}: {s:?}");
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
                assert_eq!(world.get_voxel(x + dx, g + 2, z + dz), M.air);
            }
        }
        assert_eq!(
            world.shape_at(x, g, z).max_y,
            0.5,
            "seed {seed}: the generator's point query agrees with the world's derived shape"
        );
    }
    // The flat world's grass top rests at half height.
    assert_eq!(
        spawn_point_for(GENERATOR_FLAT, 9)[1],
        f64::from(FLAT_TOP_Y) - 0.5
    );
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
                let sea = SEA_LEVEL as f32;
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
fn no_bedrock_and_the_core_is_rock_across_the_midplane() {
    for seed in [0u64, 3] {
        let g = BifacialTerrain::new(seed);
        for c in [(0, 0, 0), (0, -1, 0), (200, 0, -30), (200, -1, -30)] {
            let chunk = gen_world(&g, ChunkCoord::new(c.0, c.1, c.2));
            assert!(chunk.voxels().iter().all(|&m| !info(m).indestructible));
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
        let h = g.column_at(cx * S + 16, cz * S + 16).height as i32;
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
                        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                            let n = g.ground_y(wx + dx, wz + dz).expect("inside");
                            assert!((n - top).abs() <= rule.max_slope);
                        }
                        // The rule's heights are metres above sea level.
                        assert!((rule.min_height..=rule.max_height).contains(&(top - SEA_LEVEL)));
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
            while g.column_at(at(r).0, at(r).1).height >= SEA_LEVEL as f32 && r < 40_000.0 {
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
                            if top - SEA_LEVEL >= rule.min_height
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
    // surface steeper than the slide's 44° (the adaptive blocks' 45° ramps, ridge flanks and
    // saddles, ADAPTIVE_BLOCKS.md), everywhere, and peaks up to about 500 m above sea level
    // (generator version 16, `shared/worldgen/terrain.json`). Land columns on a 499 m grid over
    // three seeds, both faces; each column's shaped top block sampled at four points, as
    // `--example relief` does. Version 15 measured ~0.01 % sliding, a highest column ~190 m and a
    // mean slope of ~5.9°; version 16 ~15 %, ~500 m and ~22°, with the island under water
    // (hollows below sea level) still under 1.5 %.
    let slide_tan = 44f32.to_radians().tan();
    for face in [Face::A, Face::B] {
        let (mut highest, mut slope, mut land) = (0.0f64, 0.0f64, 0usize);
        let (mut samples, mut slides, mut island, mut flooded) = (0usize, 0usize, 0usize, 0usize);
        for seed in [1u64, 42, 12345] {
            let terrain = BifacialTerrain::new(seed);
            let g = terrain.local(face);
            for z in (-30_000..=30_000).step_by(499) {
                for x in (-30_000..=30_000).step_by(499) {
                    let col = g.column_at(x, z);
                    let h = f64::from(col.height - SEA_LEVEL as f32);
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
                    // The top block's shape, from the tops of its 3 × 3 columns.
                    let top = g.ground_y(x, z).expect("inside");
                    let mut tops = [[0; 3]; 3];
                    for (j, row) in tops.iter_mut().enumerate() {
                        for (i, t) in row.iter_mut().enumerate() {
                            *t = g
                                .ground_y(x + i as i32 - 1, z + j as i32 - 1)
                                .expect("inside");
                        }
                    }
                    let get = |ox: i32, oy: i32, oz: i32| {
                        let y = top + oy;
                        if y <= tops[(oz + 1) as usize][(ox + 1) as usize] {
                            M.stone
                        } else if y < SEA_LEVEL {
                            M.water
                        } else {
                            M.air
                        }
                    };
                    let shaped = dwell_blocks::adaptive::shape_at(get, 1);
                    let hi =
                        |fx: f32, fz: f32| dwell_blocks::shape::solid_span_at(shaped, fx, fz).hi;
                    for (fx, fz) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                        let gx = (hi(fx + 0.05, fz) - hi(fx - 0.05, fz)) / 0.1;
                        let gz = (hi(fx, fz + 0.05) - hi(fx, fz - 0.05)) / 0.1;
                        samples += 1;
                        slides += usize::from(gx.hypot(gz) > slide_tan);
                    }
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
        assert!((0.12..0.18).contains(&sliding), "{at}");
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
        while g.column_at(d * dx, d * dz).height >= SEA_LEVEL as f32 - 1.0 {
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

/// The water's drawn top in the sea's top row: sea level less the surface drop.
fn drawn_sea_top() -> f64 {
    let drop = dwell_blocks::require_fluid("dwell:water").surface_drop;
    f64::from(SEA_LEVEL) - drop
}

#[test]
fn the_sea_floods_the_open_part_of_every_natural_top_block_below_sea_level() {
    //! Regression (owner, 2026-10-20: water piled up against the beach): the generator filled
    //! only empty cells below sea level, so a beach top block in the sea's top row rested at
    //! half height, ½ m under sea level and 0.375 m under the drawn water, dry, with the water
    //! beside it standing above it. Every adaptive top block below sea level is flooded now
    //! (WATER_RENDERING.md §9).
    for seed in [0, 7] {
        let (mut w, coasts) = shore_world(seed);
        let (mut checked, mut dry) = (0, Vec::new());
        for &(cx, cz) in &coasts {
            for z in cz - 24..cz + 24 {
                for x in cx - 24..cx + 24 {
                    for y in SEA_LEVEL - 3..SEA_LEVEL {
                        let m = w.get_voxel(x, y, z);
                        let top = dwell_blocks::adaptive::fills(m)
                            && !dwell_blocks::adaptive::fills(w.get_voxel(x, y + 1, z));
                        // A one-block pit derives a full cube: no open part to flood.
                        let s = w.shape_at(x, y, z);
                        if !top || (!s.fan && s.corners == [2; 4]) {
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
        assert!(checked > 500, "seed {seed}: {checked}");
        assert!(
            dry.is_empty(),
            "seed {seed}: {} of {checked} top blocks below sea level are dry, e.g. {:?}",
            dry.len(),
            &dry[..dry.len().min(5)]
        );
    }
}

#[test]
fn no_shore_column_has_drawn_water_higher_than_the_dry_land_beside_it() {
    //! The rule of WATER_RENDERING.md §9: wherever drawn water (a water cell's or a flooded
    //! bank's) is beside a dry top block, the land's surface on the side they share is at or
    //! above the water's.
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
                        // The neighbour column's top block, in this row or above.
                        let mut ny = y;
                        while dwell_blocks::adaptive::fills(w.get_voxel(nx, ny + 1, nz)) {
                            ny += 1;
                        }
                        let n = w.get_voxel(nx, ny, nz);
                        if !dwell_blocks::adaptive::fills(n) || w.flooded_at(nx, ny, nz) {
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
