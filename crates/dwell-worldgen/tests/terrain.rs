//! The terrain's behaviour beyond the goldens (ported from the C++ worldgen, island and bifacial
//! tests): noise precision, heights, purity, point queries, bounds, the air test, the fill, spawn,
//! and the two faces.

use dwell_blocks::{StateId, info, mirror, require};
use dwell_world::*;
use dwell_worldgen::noise::{fbm2, lattice, perlin2, perlin2f};
use dwell_worldgen::*;

const S: i32 = CHUNK_SIZE;

fn solid(m: StateId) -> bool {
    m != M.air && m != M.water
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
    while g.column_at(x, 0).height >= 0.0 {
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
    assert!(highest > 60.0 && highest < 200.0, "{highest}");
    assert!(lowest > -30.0, "{lowest}");
    assert!(g.column_at(cx, cz).height.abs() < 6.0);
}

#[test]
fn a_chunk_is_a_pure_function_of_seed_and_coordinate() {
    let (a, b, c) = (
        TerrainGenerator::new(123),
        TerrainGenerator::new(123),
        TerrainGenerator::new(124),
    );
    let coord = ChunkCoord::new(1, 2, -1);
    assert_eq!(gen_chunk(&a, coord).voxels(), gen_chunk(&b, coord).voxels());
    assert_ne!(gen_chunk(&a, coord).voxels(), gen_chunk(&c, coord).voxels());
    let n = ChunkCoord::new(2, 2, -1);
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
fn world_bounds_stone_floor_nothing_below_or_at_the_top() {
    let g = TerrainGenerator::new(5);
    let bottom = MIDPLANE_Y / S;
    let floor = gen_chunk(&g, ChunkCoord::new(0, bottom, 0));
    for z in 0..S {
        for x in 0..S {
            for y in 0..4 {
                assert_eq!(floor.get(x, y, z), M.stone);
            }
        }
    }
    for cy in [bottom - 1, WORLD_MAX_Y / S] {
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
    let column = ChunkCoord::new(181_019, 0, 181_019);
    assert_eq!(chunk_disc_overlap(column.x, column.z), DiscOverlap::Partial);
    let bottom = MIDPLANE_Y / S;
    let (mut inside, mut outside) = (0, 0);
    for cy in [bottom, -1] {
        let chunk = gen_chunk(&g, ChunkCoord::new(column.x, cy, column.z));
        for z in 0..S {
            for x in 0..S {
                let inn =
                    inside_world_disc(i64::from(column.x * S + x), i64::from(column.z * S + z));
                if cy == bottom {
                    if inn { inside += 1 } else { outside += 1 }
                }
                for y in 0..S {
                    if !inn {
                        assert_eq!(chunk.get(x, y, z), M.air);
                    } else if cy == bottom && y < 4 {
                        assert_eq!(chunk.get(x, y, z), M.stone);
                    }
                }
            }
        }
    }
    assert!(inside > 0 && outside > 0);
    let air = air_test_for(GENERATOR_TERRAIN, 5);
    for cy in [bottom, -1, 0, 3] {
        let c = ChunkCoord::new(256_001, cy, 0);
        assert_eq!(chunk_disc_overlap(c.x, c.z), DiscOverlap::Outside);
        assert!(gen_chunk(&g, c).voxels().iter().all(|&v| v == M.air));
        assert!(air(c) && g.is_air_chunk(c));
    }
    assert!(g.column_at(256_001 * S, 0).outside);
    assert!(!g.solid_at(256_001 * S, MIDPLANE_Y, 0));
    assert!(g.solid_at(255_990 * S, MIDPLANE_Y, 0));
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
            (249_990, 10),
            (-3, 249_000),
        ] {
            let surface = floor_div(g.column_at(cx * S, cz * S).height as i32, S);
            for cy in surface - 2..=surface + 6 {
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
    assert!(air(ChunkCoord::new(0, 1, 0)));
    assert!(!air(ChunkCoord::new(0, -1, 0)));
}

#[test]
fn the_sandbox_is_the_flat_world_in_its_indestructible_floor_block() {
    let floor = require(SANDBOX_FLOOR);
    assert!(info(floor).indestructible && info(floor).solid && !info(floor).placeable);
    let sandbox = generator_for(GENERATOR_SANDBOX, 7);
    let flat = generator_for(GENERATOR_FLAT, 7);
    let air = air_test_for(GENERATOR_SANDBOX, 7);
    for c in [(0, -1, 0), (0, 0, 0), (3, -5, -2), (0, MIDPLANE_Y / S, 0)] {
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
    assert_eq!(world.get_voxel(3, -1, 3), floor);
    assert_eq!(world.get_voxel(3, 0, 3), M.air);
    assert_eq!(
        spawn_point_for(GENERATOR_SANDBOX, 7),
        spawn_point_for(GENERATOR_FLAT, 7)
    );
}

#[test]
fn the_flat_world_and_retired_versions_are_unchanged_by_the_sandbox() {
    // The flat world's ground chunk, what every version but terrain and sandbox generates.
    let mut chunk = Chunk::default();
    generate_flat_chunk(ChunkCoord::new(0, -1, 0), &mut chunk);
    let flat_hash = chunk_hash(&chunk);
    for version in [GENERATOR_FLAT, 1, 2, 7, 9, 99] {
        let mut c = Chunk::default();
        generator_for(version, 3)(ChunkCoord::new(0, -1, 0), &mut c);
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
    let (mut water, mut grass, mut sand) = (0, 0, 0);
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
                        let m = chunk.get(lx, y, z);
                        let depth = ground - wy;
                        if depth < 0 {
                            assert_eq!(m, if wy < SEA_LEVEL { M.water } else { M.air });
                            water += i32::from(m == M.water);
                        } else if ground < SEA_LEVEL + 2 {
                            assert_eq!(m, if depth < 3 { M.sand } else { M.stone });
                            sand += i32::from(m == M.sand);
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
        water > 1000 && grass > 100 && sand > 100,
        "{water} {grass} {sand}"
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
                assert_eq!(world.get_voxel(x + dx, g + 1, z + dz), M.air);
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
    assert_eq!(spawn_point_for(GENERATOR_FLAT, 9)[1], -0.5);
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
    assert_eq!(MIDPLANE_Y, -2048);
    assert_eq!(
        (
            mirror_y(-2048),
            mirror_y(-2049),
            mirror_y(-4096),
            mirror_y(-4097)
        ),
        (-2049, -2048, -1, 0)
    );
    assert_eq!((mirror_chunk_y(-64), mirror_chunk_y(-65)), (-65, -64));
    for y in [-10240, -5000, -2049, -2048, -1, 0, 6143] {
        assert_eq!(mirror_y(mirror_y(y)), y);
        let cy = floor_div(y, S);
        let local = mirror_y(y);
        assert!(mirror_chunk_y(cy) * S <= local && local < (mirror_chunk_y(cy) + 1) * S);
    }
    assert_eq!((face_of_y(-2048), face_of_y(-2049)), (Face::A, Face::B));
    assert_eq!(mirror_y(WORLD_BOTTOM_Y), WORLD_MAX_Y - 1);
    assert_eq!(MIN_CHUNK_Y, -320);
    assert_eq!(mirror_chunk_y(MIN_CHUNK_Y), MAX_CHUNK_Y);
    assert_eq!(
        face_local_chunk(ChunkCoord::new(3, -70, -4)),
        ChunkCoord::new(3, -59, -4)
    );
    for m in 0..dwell_blocks::STATES.len() as StateId {
        assert_eq!(mirror(mirror(m)), m);
    }
}

#[test]
fn a_chunk_of_face_b_is_the_flip_of_its_face_local_chunk() {
    for seed in [0u64, 20_260_925] {
        let g = BifacialTerrain::new(seed);
        for c in [
            (0, -65, 0),
            (4, -66, -7),
            (-3, -100, 5),
            (0, -128, 0),
            (0, -129, 0),
            (0, -130, 0),
            (9, -129, 2),
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
                if a.outside || b.outside || (a.height < 0.0 && b.height < 0.0) {
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
        for c in [(0, -64, 0), (0, -65, 0), (200, -64, -30), (200, -65, -30)] {
            let chunk = gen_world(&g, ChunkCoord::new(c.0, c.1, c.2));
            assert!(chunk.voxels().iter().all(|&m| !info(m).indestructible));
        }
        for (x, z) in [
            (0, 0),
            (32 * 200 + 5, -30 * 32 + 3),
            (-32 * 12 + 9, 40 * 32 + 1),
        ] {
            assert!(g.solid_at(x, -2049, z) && g.solid_at(x, -2048, z));
        }
    }
}

#[test]
fn point_queries_by_face_agree_with_the_chunk_path() {
    let g = BifacialTerrain::new(11);
    for c in [(0, -66, 0), (5, -129, 5), (0, -130, 0), (0, -131, 0)] {
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
            for cy in [-66, -65, -130, -129, -128, -127, -200, -320, -321] {
                let c = ChunkCoord::new(cx * 997, cy, cz * 1013);
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
    for cy in [-229, -279, MIN_CHUNK_Y] {
        assert!(air(ChunkCoord::new(0, cy, 0)) && air(ChunkCoord::new(1000, cy, -1000)));
    }
    assert!(
        air(ChunkCoord::new(0, MIN_CHUNK_Y - 1, 0)) && air(ChunkCoord::new(0, MAX_CHUNK_Y + 1, 0))
    );
    let _ = require("dwell:air");
}
