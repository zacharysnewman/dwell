//! Structure placements (`shared/worldgen/placements.json`, `placements.rs`): the owner's pines in
//! groves (2026-10-20). Where they stand, how many, that chunks agree on the trees across their
//! borders whichever generates first, that they never overlap, the turn and the mix, and that a
//! placement added by data alone is placed.

use std::collections::HashMap;

use dwell_blocks::{StateId, require};
use dwell_world::*;
use dwell_worldgen::placements::{PLACEMENTS, Placement, Tree};
use dwell_worldgen::*;

const S: i32 = CHUNK_SIZE;

fn gen_chunk(g: &TerrainGenerator, c: ChunkCoord) -> Chunk {
    let mut chunk = Chunk::default();
    g.generate(c, &mut chunk);
    chunk
}

/// The squares sampled: 256 m on a side, spread over the island of a seed.
fn squares() -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    for j in -3..=3 {
        for i in -3..=3 {
            out.push((i * 2_400 + 300, j * 2_400 - 700));
        }
    }
    out
}

/// Squares on the low ground just inland of the coast, on twelve rays from the origin.
fn coast_squares(g: &TerrainGenerator) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    for ray in 0..12 {
        let a = f64::from(ray * 30).to_radians();
        let at = |r: f64| ((r * a.cos()) as i32, (r * a.sin()) as i32);
        let mut r = 12_000.0;
        while g.column_at(at(r).0, at(r).1).height >= SEA_LEVEL as f32 && r < 40_000.0 {
            r += 16.0;
        }
        for back in [300.0, 600.0, 900.0] {
            let (x, z) = at(r - back);
            out.push((x - 128, z - 128));
        }
    }
    out
}

/// Trees anchored in a square (each found once, by its anchor).
fn trees_anchored(g: &TerrainGenerator, (x0, z0): (i32, i32), n: i32) -> Vec<Tree> {
    g.trees_in((x0, z0), (x0 + n - 1, z0 + n - 1))
        .into_iter()
        .filter(|t| (x0..x0 + n).contains(&t.anchor[0]) && (z0..z0 + n).contains(&t.anchor[2]))
        .collect()
}

#[test]
fn pines_stand_on_grass_in_the_height_band_at_about_the_target_density() {
    // The target, from the data: one candidate per grid cell, kept by the density × the grove
    // factor × the height's rise. Of those, the slope, the footprint (all grass, so clear of the
    // shore) and the overlap rule drop some: the trees per area are 35–90 % of the target summed
    // over the candidates on dry grass in the band. Groves hold several times the trees of the
    // ground between them, and high ground more than the low.
    let g = TerrainGenerator::new(7);
    let p = &g.planters()[0];
    let rule = p.rule();
    let (mut target, mut placed) = (0.0f64, 0usize);
    let (mut grove, mut grove_cells, mut open, mut open_cells) = (0usize, 0usize, 0usize, 0usize);
    let (mut low, mut low_cells, mut high, mut high_cells) = (0usize, 0usize, 0usize, 0usize);
    for sq in squares().into_iter().chain(coast_squares(&g)) {
        let n = 256;
        let trees = trees_anchored(&g, sq, n);
        let at: HashMap<(i32, i32), &Tree> = trees.iter().map(|t| (t.cell, t)).collect();
        placed += trees.len();
        let gr = rule.grid;
        for gz in sq.1.div_euclid(gr)..(sq.1 + n).div_euclid(gr) {
            for gx in sq.0.div_euclid(gr)..(sq.0 + n).div_euclid(gr) {
                let (x, z) = p.point(gx, gz);
                if !(sq.0..sq.0 + n).contains(&x) || !(sq.1..sq.1 + n).contains(&z) {
                    continue;
                }
                let Some(top) = g.ground_y(x, z) else {
                    continue;
                };
                let h = top - SEA_LEVEL;
                if g.state_at(x, top, z) != M.grass || h < rule.min_height || h > rule.max_height {
                    assert!(!at.contains_key(&(gx, gz)), "off grass or out of the band");
                    continue;
                }
                target += f64::from(p.chance(x, z, h));
                let here = usize::from(at.contains_key(&(gx, gz)));
                if p.grove(x, z) > 0.9 {
                    (grove, grove_cells) = (grove + here, grove_cells + 1);
                } else if p.grove(x, z) < rule.grove_outside + 0.01 {
                    (open, open_cells) = (open + here, open_cells + 1);
                }
                if h < 60 {
                    (low, low_cells) = (low + here, low_cells + 1);
                } else if h >= rule.rise_full {
                    (high, high_cells) = (high + here, high_cells + 1);
                }
            }
        }
    }
    let share = placed as f64 / target;
    let rate = |n: usize, d: usize| n as f64 / d.max(1) as f64;
    eprintln!(
        "placed {placed} of a target {target:.0} ({share:.2}); groves {:.3}, open {:.3}, low {:.3}, high {:.3}",
        rate(grove, grove_cells),
        rate(open, open_cells),
        rate(low, low_cells),
        rate(high, high_cells)
    );
    eprintln!("cells: grove {grove_cells}, open {open_cells}, low {low_cells}, high {high_cells}");
    assert!(placed > 2_000, "{placed}");
    assert!((0.35..=0.9).contains(&share), "{share}");
    assert!(
        rate(grove, grove_cells) > 5.0 * rate(open, open_cells),
        "groves, clearings between"
    );
    assert!(
        rate(high, high_cells) > 1.5 * rate(low, low_cells),
        "more on higher ground"
    );
}

#[test]
fn no_pine_on_sand_water_slides_or_bare_peaks_in_the_chunks() {
    // Every log and leaf the generator puts stands over a dry grass column, above its ground; at
    // the coast none reach the sand. And no anchor is on a slope steeper than the slide's 44°, or
    // above the band (tall grass and pines both stop at 400 m).
    let log = require("dwell:log");
    let leaves = require("dwell:leaves");
    let tan2 = 44f32.to_radians().tan().powi(2);
    let mut seen = 0;
    for seed in [7u64, 42] {
        let g = TerrainGenerator::new(seed);
        let rule = g.planters()[0].rule();
        // Inland and at the coast on the +x axis.
        let mut coast = 0;
        while g.column_at(coast, 0).height >= SEA_LEVEL as f32 {
            coast += 16;
        }
        for (cx0, cz0) in [(3, -2), (floor_div(coast, S) - 2, -1), (-40, 25)] {
            for cz in cz0..cz0 + 3 {
                for cx in cx0..cx0 + 3 {
                    let mid = g.ground_y(cx * S + 16, cz * S + 16).expect("inside");
                    for cy in floor_div(mid, S) - 1..=floor_div(mid, S) + 1 {
                        let c = ChunkCoord::new(cx, cy, cz);
                        let chunk = gen_chunk(&g, c);
                        for (i, &m) in chunk.voxels().iter().enumerate() {
                            if m != log && m != leaves {
                                continue;
                            }
                            seen += 1;
                            let (x, y, z) = local_xyz(i);
                            let (wx, wy, wz) = (c.x * S + x, c.y * S + y, c.z * S + z);
                            let top = g.ground_y(wx, wz).expect("inside");
                            assert!(wy > top, "{seed}: above the ground");
                            assert!(top >= SEA_LEVEL, "{seed}: never over water");
                            assert!(
                                !dwell_blocks::info(g.state_at(wx, top, wz)).flooded,
                                "{seed}: never on a flooded block"
                            );
                            assert_eq!(
                                g.state_at(wx, top, wz),
                                M.grass,
                                "{seed}: over grass, never sand"
                            );
                        }
                    }
                }
            }
        }
        for sq in squares() {
            for t in trees_anchored(&g, sq, 256) {
                let [x, _, z] = t.anchor;
                let h = |dx: i32, dz: i32| g.column_at(x + dx, z + dz).height;
                let gx = (h(2, 0) - h(-2, 0)) * 0.25;
                let gz = (h(0, 2) - h(0, -2)) * 0.25;
                assert!(gx * gx + gz * gz <= tan2 * 1.0001, "{seed}: on a slide");
                let above = t.anchor[1] - 1 - SEA_LEVEL;
                assert!((rule.min_height..=rule.max_height).contains(&above));
            }
        }
    }
    assert!(seen > 1_000, "{seen} tree voxels checked");
}

/// The inverse of `local_index`.
fn local_xyz(i: usize) -> (i32, i32, i32) {
    let i = i as i32;
    let p = (i & 31, (i >> 5) & 31, (i >> 10) & 31);
    assert_eq!(local_index(p.0, p.1, p.2), i as usize);
    p
}

#[test]
fn chunks_agree_on_trees_across_their_borders_in_any_order() {
    // A tree whose box crosses chunk borders is whole: each of its voxels (where the ground left
    // air) is in the chunk that holds it, generated by itself, in either order, by a fresh
    // generator or a used one.
    let g = TerrainGenerator::new(7);
    let mut crossing = 0;
    for sq in squares().into_iter().take(12) {
        for t in trees_anchored(&g, sq, 256) {
            let fp = g.planters()[0].footprint(&t);
            let cx = (floor_div(fp[0], S), floor_div(fp[2], S));
            let cz = (floor_div(fp[1], S), floor_div(fp[3], S));
            if cx.0 == cx.1 && cz.0 == cz.1 {
                continue;
            }
            crossing += 1;
            let placed = g.planters()[0].place(&t);
            let mut chunks: HashMap<ChunkCoord, Chunk> = HashMap::new();
            let fresh = TerrainGenerator::new(7);
            for (n, (p, state)) in placed.cells().enumerate() {
                let c = ChunkCoord::new(floor_div(p[0], S), floor_div(p[1], S), floor_div(p[2], S));
                // Alternate generators: one used for many chunks already, one fresh.
                let chunk = chunks.entry(c).or_insert_with(|| {
                    if n % 2 == 0 {
                        gen_chunk(&fresh, c)
                    } else {
                        gen_chunk(&g, c)
                    }
                });
                let (lx, ly, lz) = (p[0] - c.x * S, p[1] - c.y * S, p[2] - c.z * S);
                let base = g.state_at(p[0], p[1], p[2]);
                if base == M.air {
                    assert_eq!(
                        chunk.get(lx, ly, lz),
                        state,
                        "tree at {:?}, voxel {p:?}",
                        t.anchor
                    );
                }
            }
            for (c, chunk) in &chunks {
                assert_eq!(
                    chunk.voxels(),
                    gen_chunk(&fresh, *c).voxels(),
                    "a pure function"
                );
            }
            if crossing >= 40 {
                return;
            }
        }
    }
    panic!("only {crossing} border-crossing trees found");
}

#[test]
fn placed_pines_never_overlap_and_any_window_finds_the_same_ones() {
    // Their boxes (and the spacing) are disjoint; and asking for a window finds exactly the trees
    // of a larger one that reach it, so no chunk sees a tree its neighbour dropped.
    let g = TerrainGenerator::new(42);
    let p = &g.planters()[0];
    let gap = p.rule().spacing;
    let mut total = 0;
    for sq in squares() {
        let big = g.trees_in(sq, (sq.0 + 255, sq.1 + 255));
        total += big.len();
        let boxes: Vec<[i32; 4]> = big.iter().map(|t| p.footprint(t)).collect();
        for (i, a) in boxes.iter().enumerate() {
            for b in &boxes[i + 1..] {
                let apart = a[2] + gap < b[0]
                    || b[2] + gap < a[0]
                    || a[3] + gap < b[1]
                    || b[3] + gap < a[1];
                assert!(apart, "{a:?} and {b:?} overlap");
            }
        }
        for (wx, wz) in [
            (sq.0 + 40, sq.1 + 40),
            (sq.0 + 101, sq.1 + 7),
            (sq.0 + 200, sq.1 + 150),
        ] {
            let small = g.trees_in((wx, wz), (wx + 31, wz + 31));
            let expect: Vec<Tree> = big
                .iter()
                .zip(&boxes)
                .filter(|(_, b)| b[0] <= wx + 31 && b[2] >= wx && b[1] <= wz + 31 && b[3] >= wz)
                .map(|(t, _)| *t)
                .collect();
            assert_eq!(small, expect, "window at ({wx}, {wz})");
        }
    }
    assert!(total > 1_000, "{total}");
}

#[test]
fn pines_turn_a_quarter_at_random_and_mix_about_60_40() {
    let g = TerrainGenerator::new(12345);
    let p = &g.planters()[0];
    let mut turns = [0usize; 4];
    let mut tall = 0;
    let mut n = 0;
    for sq in squares() {
        for t in trees_anchored(&g, sq, 256) {
            n += 1;
            turns[t.quarter_turns as usize] += 1;
            tall += usize::from(p.structure_name(&t) == "dwell:tall_pine_tree");
        }
    }
    assert!(n > 2_000, "{n}");
    let share = tall as f64 / n as f64;
    eprintln!("{n} trees: tall {share:.3}, turns {turns:?}");
    // The owner: about 60/40. The tall pine is wider, so the overlap rule drops it more often; its
    // weight (45 of 100) leaves about 40 % of the trees placed tall.
    assert!((0.36..=0.44).contains(&share), "tall pines {share}");
    for k in turns {
        let s = k as f64 / n as f64;
        assert!((0.22..=0.28).contains(&s), "turns {turns:?}");
    }
}

#[test]
fn the_lod_shows_the_pines_as_blobs_near_and_a_canopy_tint_far() {
    // Level 1 (2 m cells): each tree the chunks hold fills the cells its voxels reach, above the
    // ground. Level 3 and up: a share of the columns over groves shows leaves on its top cell,
    // about as many as the trees cover, and none over the sea.
    use dwell_world::lod::{
        LodCells, LodCoord, ORIGIN_X, ORIGIN_Y, ORIGIN_Z, SECTION_CELLS, section_size,
    };
    let section_at = |level: i32, x: i64, y: i64, z: i64| {
        let s = section_size(level);
        let d = |v: i64, o: i64| (v - o).div_euclid(s) as i32;
        LodCoord::new(level, d(x, ORIGIN_X), d(y, ORIGIN_Y), d(z, ORIGIN_Z))
    };
    let g = TerrainGenerator::new(7);
    let leaves = require("dwell:leaves");
    let log = require("dwell:log");
    let mut cells = LodCells::new();
    let (mut blobs, mut tinted, mut columns) = (0usize, 0usize, 0usize);
    for sq in squares() {
        let Some(t) = trees_anchored(&g, sq, 256).into_iter().next() else {
            continue;
        };
        let [x, y, z] = t.anchor;
        let c = section_at(1, x.into(), (y + 2).into(), z.into());
        g.generate_lod(c, &mut cells, None);
        blobs += cells.iter().filter(|&&m| m == leaves || m == log).count();
        for level in [3, 4] {
            let c = section_at(level, x.into(), y.into(), z.into());
            g.generate_lod(c, &mut cells, None);
            tinted += cells.iter().filter(|&&m| m == leaves).count();
            columns += (SECTION_CELLS * SECTION_CELLS) as usize;
        }
    }
    let share = tinted as f64 / columns as f64;
    eprintln!("blob cells {blobs}; tinted {tinted} of {columns} columns ({share:.3})");
    assert!(blobs > 200, "{blobs}");
    assert!((0.005..0.2).contains(&share), "{share}");
    // Over the open sea: nothing.
    let sea = section_at(3, 45_000, i64::from(SEA_LEVEL), -20_000);
    g.generate_lod(sea, &mut cells, None);
    assert!(!cells.contains(&leaves));
}

#[test]
fn players_never_spawn_in_or_under_a_pine() {
    let log = require("dwell:log");
    let leaves = require("dwell:leaves");
    for seed in 0..16u64 {
        let s = spawn_point_for(GENERATOR_TERRAIN, seed);
        let mut world = VoxelWorld::new(generator_for(GENERATOR_TERRAIN, seed), None);
        let (x, y, z) = (
            s[0].floor() as i32,
            s[1].floor() as i32,
            s[2].floor() as i32,
        );
        for dz in -4..=4 {
            for dx in -4..=4 {
                for dy in 1..=12 {
                    let m: StateId = world.get_voxel(x + dx, y + dy, z + dz);
                    assert!(
                        m != log && m != leaves,
                        "seed {seed}: a tree over the spawn"
                    );
                }
            }
        }
    }
}

#[test]
fn a_placement_added_by_data_alone_stands_its_structure_in_the_chunks() {
    // A fixture rule (the proto rock on grass, every 12 m cell, no groves): the generator stands
    // it by its data alone, whole, on the ground.
    let rule = Placement {
        name: "test_rocks",
        structures: &[("dwell:proto_rock", 1)],
        on: &["dwell:grass"],
        footprint_on: false,
        grid: 12,
        density: 1.0,
        grove_wavelength: 0,
        grove_coverage: 1.0,
        grove_outside: 1.0,
        min_height: 0,
        max_height: 1000,
        rise_full: 0,
        rise_low: 1.0,
        max_gradient_sq: 4.0,
        spacing: 0,
        rotations: &[0],
    };
    let g = TerrainGenerator::new(7).with_placements(&[rule]);
    assert_eq!(
        PLACEMENTS[0].name, "pine_groves",
        "the build's own rule is replaced"
    );
    let rock = [M.stone, M.dirt];
    let trees = g.trees_in((64, 64), (127, 127));
    assert!(trees.len() > 10, "{}", trees.len());
    let t = trees[trees.len() / 2];
    let c = ChunkCoord::new(
        floor_div(t.anchor[0], S),
        floor_div(t.anchor[1], S),
        floor_div(t.anchor[2], S),
    );
    let chunk = gen_chunk(&g, c);
    let (lx, ly, lz) = (
        t.anchor[0] - c.x * S,
        t.anchor[1] - c.y * S,
        t.anchor[2] - c.z * S,
    );
    let at = g.planters()[0].place(&t).at(t.anchor);
    assert!(at.is_some_and(|m| rock.contains(&m)));
    if g.state_at(t.anchor[0], t.anchor[1], t.anchor[2]) == M.air {
        assert_eq!(Some(chunk.get(lx, ly, lz)), at);
    }
}
