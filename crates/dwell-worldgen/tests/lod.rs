//! Level of detail (§6.6, ADR 0012): the section grid, the downsample rule, and generated sections
//! against the downsample of generated chunks (ported from the C++ LOD tests).

use dwell_blocks::StateId;
use dwell_protocol::{decode_lod_cells, encode_lod_cells};
use dwell_world::lod::*;
use dwell_world::*;
use dwell_worldgen::*;

const N: i32 = SECTION_CELLS;

/// A section built bottom-up from generated chunks (what the server's propagation computes).
fn downsampled_from_chunks(generate: &ChunkGenerator, c: LodCoord) -> LodCells {
    let mut out = vec![M.air; VOLUME];
    for o in 0..8 {
        let ch = child(c, o);
        if ch.level == 0 {
            let mut chunk = Chunk::default();
            generate(chunk_of_lod(ch), &mut chunk);
            downsample_chunk_octant(&chunk, c, o, &mut out);
        } else {
            downsample_section_octant(&downsampled_from_chunks(generate, ch), c, o, &mut out);
        }
    }
    out
}

/// The section (at `level`) holding a world point.
fn section_at(level: i32, x: i64, y: i64, z: i64) -> LodCoord {
    let s = section_size(level);
    let div = |v: i64, o: i64| (v - o).div_euclid(s) as i32;
    LodCoord::new(level, div(x, ORIGIN_X), div(y, ORIGIN_Y), div(z, ORIGIN_Z))
}

fn coast_on_axis(g: &TerrainGenerator) -> (i32, i32) {
    let mut x = 0;
    while g.column_at(x, 0).height >= SEA_LEVEL as f32 {
        x += 16;
    }
    (x, 0)
}

fn sites(g: &TerrainGenerator) -> Vec<(&'static str, i32, i32)> {
    let (cx, cz) = coast_on_axis(g);
    vec![
        ("spawn", 0, 0),
        ("coast", cx, cz),
        ("inland", -9000, 4000),
        ("sea", 45_000, -20_000),
    ]
}

fn class_of(m: StateId) -> i32 {
    if m == M.air {
        0
    } else if lod_solid(m) {
        2
    } else {
        1
    }
}

/// Share of interior cells of the same class, share of columns whose top non-air cell is within
/// one cell, and the mean difference of that cell. On face B the surface is the lowest cell.
fn compare(a: &LodCells, b: &LodCells, face: Face) -> (f64, f64, f64) {
    let (mut same, mut within, mut total) = (0, 0, 0.0);
    for z in 0..N {
        for x in 0..N {
            let (mut ta, mut tb) = (-1, -1);
            for yy in 0..N {
                let y = if face == Face::A { yy } else { N - 1 - yy };
                let i = cell_index(x, y, z);
                same += i32::from(class_of(a[i]) == class_of(b[i]));
                if a[i] != M.air {
                    ta = yy;
                }
                if b[i] != M.air {
                    tb = yy;
                }
            }
            within += i32::from((ta - tb).abs() <= 1);
            total += f64::from((ta - tb).abs());
        }
    }
    let n = f64::from(N);
    (
        f64::from(same) / (n * n * n),
        f64::from(within) / (n * n),
        total / (n * n),
    )
}

#[test]
fn level_0_is_the_chunk_grid_and_the_root_holds_the_disc() {
    for c in [
        ChunkCoord::new(0, 0, 0),
        ChunkCoord::new(-1, -2, 5),
        ChunkCoord::new(1561, 1562, -1563),
    ] {
        let l = lod_of_chunk(c);
        assert_eq!(chunk_of_lod(l), c);
        let o = section_origin(l);
        assert_eq!(
            (o.x, o.y, o.z),
            (
                i64::from(c.x) * 32,
                i64::from(c.y) * 32,
                i64::from(c.z) * 32
            )
        );
    }
    let root = LodCoord::new(MAX_LEVEL, 0, 0, 0);
    assert!(in_world(root));
    assert_eq!(section_size(MAX_LEVEL), 1 << 17);
    assert_eq!(section_origin(root).x, -(1 << 16));
    assert!(!in_world(LodCoord::new(MAX_LEVEL, 1, 0, 0)));
    assert_eq!(
        ancestor(
            lod_of_chunk(ChunkCoord::new(-1563, MIN_CHUNK_Y, 1562)),
            MAX_LEVEL
        ),
        root
    );
}

#[test]
fn rows_cover_the_build_sphere_one_row_at_the_root() {
    assert_eq!(last_row(0) - first_row(0) + 1, 3126);
    assert_eq!(last_row(6) - first_row(6) + 1, 50);
    assert_eq!(last_row(8) - first_row(8) + 1, 14);
    assert_eq!((first_row(12), last_row(12)), (0, 0));
    assert_eq!(
        lod_of_chunk(ChunkCoord::new(0, MIN_CHUNK_Y, 0)).j,
        first_row(0)
    );
    assert_eq!(
        lod_of_chunk(ChunkCoord::new(0, MAX_CHUNK_Y, 0)).j,
        last_row(0)
    );
    assert!(!in_world(lod_of_chunk(ChunkCoord::new(
        0,
        MIN_CHUNK_Y - 1,
        0
    ))));
    assert!(!in_world(lod_of_chunk(ChunkCoord::new(
        0,
        MAX_CHUNK_Y + 1,
        0
    ))));
    let j8 = first_row(8);
    assert!(in_world(LodCoord::new(8, 8, j8, 8)) && in_world(LodCoord::new(8, 8, j8 + 13, 8)));
    assert!(
        !in_world(LodCoord::new(8, 8, j8 - 1, 8)) && !in_world(LodCoord::new(8, 8, j8 + 14, 8))
    );
    for level in 0..=11 {
        let s = section_at(level, 0, i64::from(MIDPLANE_Y), 0);
        assert_eq!(section_origin(s).y, i64::from(MIDPLANE_Y));
        assert_eq!((row_face(s, -1), row_face(s, 0)), (Face::B, Face::A));
        let below = section_at(level, 0, i64::from(MIDPLANE_Y) - 1, 0);
        assert_eq!(
            (row_face(below, 31), row_face(below, 32)),
            (Face::B, Face::A)
        );
    }
    let straddle = section_at(12, 0, i64::from(MIDPLANE_Y), 0);
    assert_eq!(
        (row_face(straddle, 0), row_face(straddle, 31)),
        (Face::B, Face::A)
    );
}

#[test]
fn children_nest_in_their_parent() {
    let p = LodCoord::new(5, 100, 3, 101);
    let po = section_origin(p);
    for o in 0..8 {
        let c = child(p, o);
        assert_eq!(parent(c), p);
        let co = section_origin(c);
        let h = section_size(4);
        assert_eq!(
            (co.x, co.y, co.z),
            (
                po.x + i64::from(o & 1) * h,
                po.y + i64::from((o >> 1) & 1) * h,
                po.z + i64::from((o >> 2) & 1) * h
            )
        );
    }
}

#[test]
fn sections_beyond_the_rim_are_outside() {
    assert!(in_world(section_at(4, 49_000, 0, 0)));
    assert!(!in_world(section_at(4, 50_200, 0, 0)));
    assert!(!in_world(section_at(4, 35_900, 0, 35_900)));
    // Over the rim the build sphere is low: a section above it is outside (ADR 0025).
    assert!(in_world(section_at(4, 49_000, 9_000, 0)));
    assert!(!in_world(section_at(4, 49_000, 12_000, 0)));
}

fn block(f: impl Fn(usize, usize, usize) -> StateId) -> StateId {
    downsample_block(&std::array::from_fn(|i| {
        f(i & 1, (i >> 1) & 1, (i >> 2) & 1)
    }))
}

#[test]
fn the_downsample_rule() {
    let (air, stone, dirt, grass, sand, water) = (M.air, M.stone, M.dirt, M.grass, M.sand, M.water);
    // A one-voxel floor and wall survive; a thinner pillar does not.
    assert_eq!(block(|_, y, _| if y == 0 { stone } else { air }), stone);
    assert_eq!(block(|_, y, _| if y == 1 { stone } else { air }), stone);
    assert_eq!(block(|x, _, _| if x == 1 { dirt } else { air }), dirt);
    assert_eq!(
        block(|x, _, z| if x == 0 && z == 0 { dirt } else { air }),
        air
    );
    assert_eq!(
        block(
            |x, y, z| if x + y + z <= 1 && !(x == 0 && y == 0 && z == 0) {
                stone
            } else {
                air
            }
        ),
        air
    );
    // Surface materials win, ties to the upper cells.
    assert_eq!(block(|_, y, _| if y == 1 { grass } else { dirt }), grass);
    assert_eq!(
        block(|x, y, z| if y == 1 && x == 0 && z == 0 {
            sand
        } else if y == 1 {
            air
        } else {
            stone
        }),
        stone
    );
    assert_eq!(
        block(|x, y, _| if y == 1 {
            air
        } else if x == 0 {
            sand
        } else {
            dirt
        }),
        sand
    );
    // Liquids count as filled.
    assert_eq!(block(|_, y, _| if y == 1 { water } else { sand }), water);
    assert_eq!(
        block(|x, y, z| if y == 0 && x == 0 && z == 0 {
            sand
        } else {
            water
        }),
        water
    );
    assert_ne!(
        block(|x, y, _| if y == 1 {
            air
        } else if x == 0 {
            water
        } else {
            sand
        }),
        air
    );
    assert_eq!(
        block(|x, y, _| if y == 0 && x == 0 { water } else { air }),
        air
    );
}

#[test]
fn a_chunk_downsamples_into_its_octant() {
    let mut chunk = Chunk::default();
    for y in 0..32 {
        for z in 0..32 {
            chunk.set(7, y, z, M.stone);
        }
        chunk.set(20, y, 20, M.dirt);
    }
    let mut section = vec![M.air; VOLUME];
    downsample_chunk_octant(&chunk, LodCoord::new(1, 0, 0, 0), 5, &mut section);
    assert_eq!(section[cell_index(19, 0, 20)], M.stone);
    assert_eq!(section[cell_index(19, 15, 31)], M.stone);
    assert_eq!(section[cell_index(26, 5, 26)], M.air);
    assert_eq!(section[cell_index(3, 0, 4)], M.air);
}

#[test]
fn the_flat_worlds_lod_equals_the_downsample_of_its_chunks() {
    let flat = flat_generator();
    for level in 1..=3 {
        let c = section_at(level, 5, i64::from(FLAT_TOP_Y) - 1, -7);
        let mut generated = LodCells::new();
        assert_eq!(generate_flat_lod(c, &mut generated), LodKind::Content);
        let down = downsampled_from_chunks(&flat, c);
        for y in 0..N {
            for z in 0..N {
                for x in 0..N {
                    assert_eq!(
                        generated[cell_index(x, y, z)],
                        down[cell_index(x, y, z)],
                        "level {level}"
                    );
                }
            }
        }
    }
    let mut cells = LodCells::new();
    assert_eq!(
        generate_flat_lod(section_at(2, 0, 200, 0), &mut cells),
        LodKind::Empty
    );
    assert_eq!(
        generate_flat_lod(section_at(2, 0, -1000, 0), &mut cells),
        LodKind::Buried
    );
    assert_eq!(
        generate_flat_lod(LodCoord::new(1, 0, 0, 0), &mut cells),
        LodKind::Empty
    );
}

#[test]
fn the_sandbox_lod_is_its_floor_and_equals_the_downsample_of_its_chunks() {
    let chunks = generator_for(GENERATOR_SANDBOX, 0);
    let lod = lod_generator_for(GENERATOR_SANDBOX, 0);
    let floor = dwell_blocks::require(SANDBOX_FLOOR);
    for level in 1..=3 {
        let c = section_at(level, 5, i64::from(FLAT_TOP_Y) - 1, -7);
        let mut generated = LodCells::new();
        assert_eq!(lod(c, &mut generated), LodKind::Content);
        assert!(generated.iter().all(|&m| m == M.air || m == floor));
        assert!(generated.contains(&floor));
        // The section's own cells (the downsample has no apron).
        let down = downsampled_from_chunks(&chunks, c);
        for y in 0..N {
            for z in 0..N {
                for x in 0..N {
                    let i = cell_index(x, y, z);
                    assert_eq!(generated[i], down[i], "level {level}");
                }
            }
        }
    }
    let mut cells = LodCells::new();
    assert_eq!(lod(section_at(2, 0, -1000, 0), &mut cells), LodKind::Buried);
    assert!(cells.iter().all(|&m| m == floor));
}

#[test]
fn generate_lod_is_pure_and_bounds_decide_empty_and_buried_exactly() {
    let g = BifacialTerrain::new(0);
    let ground = |x: i32, z: i32| g.local(Face::A).column_at(x, z).height as i64;
    for level in [1, 3, 5, 8] {
        let c = section_at(level, 100, ground(100, -40), -40);
        let (mut first, mut second) = (LodCells::new(), LodCells::new());
        assert_eq!(g.generate_lod(c, &mut first, None, None), LodKind::Content);
        assert_eq!(g.generate_lod(c, &mut second, None, None), LodKind::Content);
        assert_eq!(first, second);
        let bounds = g.lod_bounds_at(level, c.i, c.k);
        for j in first_row(level)..=last_row(level) {
            let s = LodCoord::new(level, c.i, j, c.k);
            let mut cells = LodCells::new();
            let kind = g.generate_lod(s, &mut cells, None, None);
            assert_eq!(kind, kind_from_bounds(s, &bounds), "{s:?}");
            match kind {
                LodKind::Empty => assert!(cells.iter().all(|&m| m == M.air)),
                LodKind::Buried => assert!(cells.iter().all(|&m| m == M.stone)),
                LodKind::Content => {}
            }
        }
    }
    let mut cells = LodCells::new();
    // Under the island on both faces, a level-1 section by the midplane is all ground.
    assert_eq!(
        g.generate_lod(section_at(1, 64, 0, 64), &mut cells, None, None),
        LodKind::Buried
    );
    assert_eq!(
        g.generate_lod(section_at(1, 64, -1, 64), &mut cells, None, None),
        LodKind::Buried
    );
    assert_eq!(
        g.generate_lod(section_at(2, 64, 3000, 64), &mut cells, None, None),
        LodKind::Empty
    );
    assert_eq!(
        g.generate_lod(section_at(2, 64, -9000, 64), &mut cells, None, None),
        LodKind::Empty
    );
}

#[test]
fn a_generated_section_agrees_with_the_downsample_of_chunks() {
    let g = TerrainGenerator::new(0);
    let chunks = generator_for(GENERATOR_TERRAIN, 0);
    let (mut mean_sum, mut count) = (0.0, 0);
    for (name, x, z) in sites(&g) {
        let h = g.column_at(x, z).height.max(0.0) as i64;
        for level in 1..=3 {
            let c = section_at(level, i64::from(x), h.max(-1), i64::from(z));
            let mut generated = LodCells::new();
            g.generate_lod(c, &mut generated, None);
            let (classes, within, mean) =
                compare(&generated, &downsampled_from_chunks(&chunks, c), Face::A);
            assert!(
                classes >= 0.95 && within >= 0.95,
                "{name} level {level}: {classes} {within}"
            );
            mean_sum += mean;
            count += 1;
        }
    }
    assert!(mean_sum / f64::from(count) < 0.5);
}

#[test]
fn a_face_b_section_agrees_with_the_downsample_of_its_chunks() {
    let g = BifacialTerrain::new(0);
    let chunks = generator_for(GENERATOR_TERRAIN, 0);
    for (name, x, z) in sites(g.local(Face::B)) {
        let h = g.local(Face::B).column_at(x, z).height as i32;
        for level in 1..=2 {
            let c = section_at(level, i64::from(x), i64::from(mirror_y(h)), i64::from(z));
            let mut generated = LodCells::new();
            g.generate_lod(c, &mut generated, None, None);
            let (classes, within, _) =
                compare(&generated, &downsampled_from_chunks(&chunks, c), Face::B);
            assert!(
                classes >= 0.95 && within >= 0.95,
                "{name} level {level}: {classes} {within}"
            );
        }
    }
}

#[test]
fn a_straddling_section_keeps_each_cell_on_its_face() {
    let g = BifacialTerrain::new(0);
    // Only the root holds both faces: rows 0–15 face B's, 16–31 face A's (4,096 m cells).
    let c = section_at(MAX_LEVEL, 0, i64::from(MIDPLANE_Y), 0);
    let mut cells = LodCells::new();
    assert_eq!(g.generate_lod(c, &mut cells, None, None), LodKind::Content);
    let at = |x, y, z| cells[cell_index(x, y, z)];
    assert!(lod_solid(at(16, 16, 16)) && lod_solid(at(16, 15, 16)));
    assert_eq!((at(16, 17, 16), at(16, 14, 16)), (M.air, M.air));
    assert_eq!((at(16, 0, 16), at(16, 31, 16)), (M.air, M.air));
}

#[test]
fn coarse_cells_show_the_grounds_surface_not_the_floor() {
    let g = TerrainGenerator::new(0);
    for level in [8, 10, 11] {
        let c = section_at(level, 0, 0, 0);
        let mut cells = LodCells::new();
        assert_eq!(g.generate_lod(c, &mut cells, None), LodKind::Content);
        let (mut tops, mut stone, mut grassy) = (0, 0, 0);
        for z in 0..N {
            for x in 0..N {
                if let Some(m) = (0..N)
                    .rev()
                    .map(|y| cells[cell_index(x, y, z)])
                    .find(|&m| m != M.air && m != M.water)
                {
                    tops += 1;
                    stone += i32::from(m == M.stone);
                    grassy += i32::from(m == M.grass || m == M.sand);
                }
            }
        }
        assert!(
            tops > 0 && stone == 0 && grassy > tops / 2,
            "level {level}: {tops} {stone} {grassy}"
        );
    }
}

#[test]
fn a_sea_shows_its_surface_at_levels_deeper_than_the_sea() {
    let g = TerrainGenerator::new(0);
    for level in [6, 8, 9] {
        let c = section_at(level, 36_000, i64::from(SEA_LEVEL) - 1, -26_000);
        let mut cells = LodCells::new();
        assert_eq!(g.generate_lod(c, &mut cells, None), LodKind::Content);
        let o = section_origin(c);
        let (mut tops, mut water) = (0, 0);
        for z in 0..N {
            for x in 0..N {
                let cs = cell_size(level);
                if level <= 7
                    && g.column_at(
                        (o.x + i64::from(x) * cs) as i32,
                        (o.z + i64::from(z) * cs) as i32,
                    )
                    .height
                        >= SEA_LEVEL as f32 - 10.0
                {
                    continue;
                }
                if let Some(m) = (0..N)
                    .rev()
                    .map(|y| cells[cell_index(x, y, z)])
                    .find(|&m| m != M.air)
                {
                    tops += 1;
                    water += i32::from(m == M.water);
                }
            }
        }
        assert!(
            tops > 0 && water >= tops * 9 / 10,
            "level {level}: {water}/{tops}"
        );
    }
}

#[test]
fn section_content_round_trips_through_the_codec() {
    let g = TerrainGenerator::new(0);
    for level in [1, 4, 9] {
        let mut cells = LodCells::new();
        g.generate_lod(section_at(level, 0, 0, 0), &mut cells, None);
        let bytes = encode_lod_cells(&cells);
        let back = decode_lod_cells(&bytes).expect("decodes");
        assert_eq!(back, cells);
        assert_eq!(encode_lod_cells(&back), bytes);
        assert!(bytes.len() < 16384, "level {level}: {} bytes", bytes.len());
    }
    let bytes = encode_lod_cells(&vec![M.stone; VOLUME]);
    assert!(decode_lod_cells(&bytes[..bytes.len() - 1]).is_none());
    let mut extra = bytes.clone();
    extra.push(0);
    assert!(decode_lod_cells(&extra).is_none());
}

#[test]
fn column_surfaces_put_distant_land_and_seas_at_their_true_height() {
    let g = TerrainGenerator::new(5);
    for level in 1..=MAX_LEVEL {
        let cell = cell_size(level);
        let (mut error_surface, mut columns, mut valid) = (0.0f64, 0, 0);
        for s in 0..6 {
            let (px, pz) = (-20_000 + s * 8000, 3000 - s * 4000);
            let c = section_at(
                level,
                i64::from(px),
                (g.column_at(px, pz).height as i64).max(0),
                i64::from(pz),
            );
            let mut cells = LodCells::new();
            let mut surface = LodSurfaces::new();
            if g.generate_lod(c, &mut cells, Some(&mut surface)) != LodKind::Content {
                continue;
            }
            assert_eq!(surface.len(), (PAD * PAD) as usize);
            let o = section_origin(c);
            for z in 0..N {
                for x in 0..N {
                    let Some(top) = (0..N).rev().find(|&y| cells[cell_index(x, y, z)] != M.air)
                    else {
                        continue;
                    };
                    if top == N - 1 {
                        continue; // the surface is in another section
                    }
                    let truth = g
                        .column_at(
                            (o.x + i64::from(x) * cell + cell / 2) as i32,
                            (o.z + i64::from(z) * cell + cell / 2) as i32,
                        )
                        .height;
                    let sf = surface[column_index(x, z)];
                    columns += 1;
                    if !sf.valid {
                        continue;
                    }
                    valid += 1;
                    if sf.wet {
                        assert!(sf.height < sf.water && sf.water >= 0.0);
                    } else {
                        assert!(sf.height >= 0.0);
                    }
                    error_surface += f64::from(sf.height - truth);
                }
            }
        }
        assert!(columns > 0);
        assert!(valid >= columns * 95 / 100, "level {level}");
        assert!(
            (error_surface / f64::from(valid.max(1))).abs() < 8.0 + 0.002 * cell as f64,
            "level {level}"
        );
    }
}
