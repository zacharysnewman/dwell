//! `client/src/mesh/mesher.test.ts` and the logic cases of `client/src/world/shapes.test.ts`.

use super::*;
use dwell_blocks::{RenderMode, ShapeFace, require};
use dwell_world::{ChunkCoord, FaceFrame};

const UNKNOWN: u16 = 65_000;

// The mesher emits chunk-local metres (HALF_HEIGHT_GRID.md §2). These tests are about what is
// drawn where in the cells, so they read its output back in cells: the entry points below shadow
// the mesher's, dividing y by the cell height (`emits_metres_one_cell_tall` checks the metres).
fn in_cells(mut m: ChunkMeshes) -> ChunkMeshes {
    for a in [&mut m.opaque, &mut m.transparent, &mut m.cutout] {
        for p in a.positions.iter_mut().skip(1).step_by(3) {
            *p = (f64::from(*p) / dwell_world::CELL_H) as f32;
        }
    }
    m
}
fn mesh_chunk(padded: &[u16], mirror: bool) -> Result<ChunkMeshes, LengthError> {
    super::mesh_chunk(padded, mirror).map(in_cells)
}
fn mesh_chunk_at(
    padded: &[u16],
    mirror: bool,
    origin: [i32; 3],
) -> Result<ChunkMeshes, LengthError> {
    super::mesh_chunk_at(padded, mirror, origin).map(in_cells)
}
fn mesh_chunk_with(
    kinds: &Kinds,
    padded: &[u16],
    mirror: bool,
    origin: [i32; 3],
) -> Result<ChunkMeshes, LengthError> {
    super::mesh_chunk_with(kinds, padded, &[], mirror, origin).map(in_cells)
}

fn water() -> u16 {
    require("dwell:water")
}
fn grass() -> u16 {
    require("dwell:grass")
}
fn stone() -> u16 {
    require("dwell:stone")
}
/// A full cube that is not adaptive (natural blocks shape themselves as top blocks).
fn cube_block() -> u16 {
    require("dwell:sandbox_floor")
}

/// Padded voxels with the given cells set (chunk-local, −1..32).
fn voxels(cells: &[(i32, i32, i32, u16)]) -> Vec<u16> {
    let mut out = vec![0u16; PADDED_VOLUME];
    for &(x, y, z, m) in cells {
        out[padded_index(x, y, z)] = m;
    }
    out
}

fn mesh(cells: &[(i32, i32, i32, u16)], mirror: bool) -> ChunkMeshes {
    mesh_chunk(&voxels(cells), mirror).expect("padded volume")
}

fn vertex(p: &[f32], i: u32) -> [f64; 3] {
    let i = i as usize * 3;
    [f64::from(p[i]), f64::from(p[i + 1]), f64::from(p[i + 2])]
}

fn normal_of(p: &[f32], i0: u32, i1: u32, i2: u32) -> [f64; 3] {
    let a = vertex(p, i0);
    let b = vertex(p, i1);
    let c = vertex(p, i2);
    let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    [
        e1[1] * e2[2] - e1[2] * e2[1],
        e1[2] * e2[0] - e1[0] * e2[2],
        e1[0] * e2[1] - e1[1] * e2[0],
    ]
}

fn quads(a: &MeshArrays) -> usize {
    a.indices.len() / 6
}

/// The cell faces quads cover (their area in cells; merged quads count each cell).
fn quad_area(a: &MeshArrays) -> f32 {
    (0..quads(a))
        .map(|q| {
            let (lo, hi) = bounds(a, q);
            (0..3)
                .map(|i| hi[i] - lo[i])
                .filter(|e| *e > 0.0)
                .product::<f32>()
        })
        .sum()
}

/// Bounds of quad `q`'s vertices.
fn bounds(a: &MeshArrays, q: usize) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for k in 0..4 {
        for i in 0..3 {
            let v = a.positions[(q * 4 + k) * 3 + i];
            min[i] = min[i].min(v);
            max[i] = max[i].max(v);
        }
    }
    (min, max)
}

fn close(a: f64, b: f64, digits: i32) -> bool {
    (a - b).abs() < 10f64.powi(-digits) / 2.0
}

#[test]
fn draws_a_lone_cube_as_six_quads_wound_counter_clockwise_facing_out() {
    let ChunkMeshes {
        opaque,
        transparent,
        ..
    } = mesh(&[(1, 1, 1, cube_block())], false);
    assert_eq!(quads(&opaque), 6);
    assert_eq!(quads(&transparent), 0);
    let mut normals = std::collections::HashSet::new();
    for q in 0..6 {
        let i = &opaque.indices[q * 6..];
        let n = normal_of(&opaque.positions, i[0], i[1], i[2]).map(f64::signum0);
        // The winding agrees with the stored normal.
        let stored = [0, 1, 2].map(|c| f64::from(opaque.normals[q * 12 + c]));
        assert_eq!(n, stored);
        normals.insert(n.map(|v| v as i32));
    }
    assert_eq!(normals.len(), 6);
}

trait Signum0 {
    fn signum0(self) -> f64;
}
impl Signum0 for f64 {
    /// JS `Math.sign`: 0 for 0.
    fn signum0(self) -> f64 {
        if self == 0.0 { 0.0 } else { self.signum() }
    }
}

#[test]
fn merges_a_floor_of_one_material_into_one_quad_per_side() {
    let mut cells = Vec::new();
    for z in 0..32 {
        for x in 0..32 {
            cells.push((x, 0, z, stone()));
            cells.push((x, 1, z, cube_block())); // under a layer of cubes, stone is a cube
        }
    }
    let opaque = mesh(&cells, false).opaque;
    // Per layer: the top, and one strip per side (the two layers' tops and bottoms hide), all
    // open to the sky. The bottom sees none of it: dark inside, its rim lit by the rays that leave
    // past the edge (each vertex's own turn of the ray set), so it merges where its light is level.
    let up = |q: usize| opaque.normals[q * 12 + 1] != -1.0;
    assert_eq!((0..quads(&opaque)).filter(|&q| up(q)).count(), 9);
    let bottoms: Vec<usize> = (0..quads(&opaque)).filter(|&q| !up(q)).collect();
    let inside = bottoms
        .iter()
        .find(|&&q| bounds(&opaque, q) == ([1.0, 0.0, 1.0], [31.0, 0.0, 31.0]))
        .expect("the dark inside of the bottom, one quad");
    assert!(
        opaque.light[inside * 4..inside * 4 + 4]
            .iter()
            .all(|&l| l == 0.0)
    );
    let area: f32 = bottoms
        .iter()
        .map(|&q| {
            let (lo, hi) = bounds(&opaque, q);
            (hi[0] - lo[0]) * (hi[2] - lo[2])
        })
        .sum();
    assert_eq!(area, 32.0 * 32.0);
    // The texture repeats per block: uvs span 32 blocks, the tile stays the stone tile.
    let r = tile_rect("stone");
    assert_eq!(opaque.uvs.iter().copied().fold(f32::MIN, f32::max), 32.0);
    assert!(close(f64::from(opaque.tiles[0]), r.u0, 2));
    assert!(close(f64::from(opaque.tiles[2]), r.u1 - r.u0, 2));
}

#[test]
fn keeps_materials_apart_and_hides_faces_against_full_neighbours_including_the_apron() {
    // Two different blocks side by side: the shared faces are hidden, the tops do not merge.
    let opaque = mesh(&[(0, 0, 0, cube_block()), (1, 0, 0, UNKNOWN)], false).opaque;
    assert_eq!(quads(&opaque), 10);
    // A block at the chunk's edge next to a solid neighbour chunk (apron): that face is hidden.
    let opaque = mesh(&[(31, 5, 5, cube_block()), (32, 5, 5, cube_block())], false).opaque;
    assert_eq!(quads(&opaque), 5);
}

#[test]
fn draws_water_in_the_transparent_pass_with_its_surface_lowered_and_hides_it_against_water() {
    let ChunkMeshes {
        opaque,
        transparent,
        ..
    } = mesh(&[(0, 0, 0, water()), (1, 0, 0, water())], false);
    assert_eq!(quads(&opaque), 0);
    assert_eq!(quads(&transparent), 6); // merged top, bottom, and sides of the 2×1 pool
    let top = (0..quads(&transparent))
        .find(|q| transparent.normals[q * 12 + 1] == 1.0)
        .expect("a top");
    assert_eq!(bounds(&transparent, top).1[1], 0.875);
}

#[test]
fn a_fluid_added_by_data_alone_is_meshed_with_its_own_surface_drop_and_opacity() {
    // shared/blocks/fixtures/test.json adds `test:oil` (surfaceDrop 0.25, opacity 0.8) and nothing
    // else: the mesher draws it from its data, beside water at water's own height.
    use dwell_blocks::fixture::{FLUIDS, STATES};
    let id = |name: &str| STATES.iter().position(|s| s.name == name).expect(name) as u16;
    let (oil, water) = (id("test:oil"), id("dwell:water[fill=8]"));
    let kinds = Kinds::new(&STATES, &FLUIDS, Some(water));
    // Its opacity reaches the meshes through the registry row (the render mode picks the mesh).
    assert_eq!(STATES[usize::from(oil)].opacity, 0.8);
    assert_eq!(kinds.of(oil).style.surface_drop, 0.25);
    let cells = voxels(&[(0, 0, 0, oil), (4, 0, 0, water)]);
    let ChunkMeshes {
        opaque,
        transparent,
        ..
    } = mesh_chunk_with(&kinds, &cells, false, [0; 3]).expect("padded volume");
    assert_eq!(quads(&opaque), 0);
    let mut tops: Vec<(f32, f32)> = (0..quads(&transparent))
        .filter(|q| transparent.normals[q * 12 + 1] == 1.0)
        .map(|q| {
            let (lo, hi) = bounds(&transparent, q);
            (lo[0], hi[1])
        })
        .collect();
    tops.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert_eq!(tops, [(0.0, 0.75), (4.0, 0.875)]);
    // Each vertex carries its fluid's id (the water shader's parameters, WATER_RENDERING.md W1b):
    // oil's faces (x < 1) its own, water's water's.
    assert_eq!(transparent.fluids.len(), transparent.positions.len() / 3);
    for (v, &f) in transparent.fluids.iter().enumerate() {
        let want = if transparent.positions[v * 3] <= 1.0 {
            "test:oil"
        } else {
            "dwell:water"
        };
        assert_eq!(FLUIDS[f as usize].name, want, "vertex {v}");
    }
    assert!(opaque.fluids.is_empty());
}

#[test]
fn blocks_of_each_render_mode_added_by_data_alone_go_to_their_meshes() {
    // shared/blocks/fixtures/test.json adds `test:hedge` (cutout cube), `test:glass` (translucent
    // cube) and `test:reed` (a three-quad cross) and nothing else: the mesher sorts them by data.
    use dwell_blocks::fixture::{FLUIDS, STATES};
    let id = |name: &str| STATES.iter().position(|s| s.name == name).expect(name) as u16;
    let (leaves, glass, reed, stone, water) = (
        id("test:hedge"),
        id("test:glass"),
        id("test:reed"),
        id("dwell:stone[flooded=false]"),
        id("dwell:water[fill=8]"),
    );
    let kinds = Kinds::new(&STATES, &FLUIDS, Some(water));
    let mesh_of = |cells: &[(i32, i32, i32, u16)]| {
        mesh_chunk_with(&kinds, &voxels(cells), false, [0; 3]).expect("padded volume")
    };
    // A lone leaf block: six cutout faces, nothing opaque or blended.
    let m = mesh_of(&[(1, 1, 1, leaves)]);
    assert_eq!(
        (quads(&m.opaque), quads(&m.cutout), quads(&m.transparent)),
        (0, 6, 0)
    );
    // Glass: six blended faces, each tagged as no fluid's.
    let m = mesh_of(&[(1, 1, 1, glass)]);
    assert_eq!(
        (quads(&m.opaque), quads(&m.cutout), quads(&m.transparent)),
        (0, 0, 6)
    );
    assert!(m.transparent.fluids.iter().all(|&f| f == crate::NO_FLUID));
    // See-through blocks hide no face of their neighbours, and none between two of themselves.
    let alone = quads(&mesh_of(&[(2, 1, 1, stone)]).opaque);
    for b in [leaves, glass] {
        let m = mesh_of(&[(1, 1, 1, b), (2, 1, 1, stone), (1, 1, 2, b)]);
        assert_eq!(quads(&m.opaque), alone, "{}", STATES[usize::from(b)].name);
        let see = quad_area(&m.cutout) + quad_area(&m.transparent);
        // Two blocks in a row along z: their shared faces skipped (−x, ±y: two cells each), the
        // end faces, and the +x face of the one beside stone drawn only for the other. (How they
        // merge depends on the corner occlusion the stone casts.)
        assert_eq!(see, 9.0, "{}", STATES[usize::from(b)].name);
    }
    // A water plant (`test:kelp`, `medium: water`) in a pool: drawn inside the water, which shows
    // no hole: its cell draws water like a flooded shape's, its neighbours' faces toward it hidden.
    let kelp = id("test:kelp[flooded=true]");
    let pool = mesh_of(&[(1, 1, 1, water), (2, 1, 1, kelp), (3, 1, 1, water)]);
    assert_eq!(quads(&pool.cutout), 2, "the kelp's cross");
    let tops = (0..quads(&pool.transparent))
        .filter(|q| pool.transparent.normals[q * 12 + 1] == 1.0)
        .count();
    assert_eq!(tops, 3, "three cells of water surface");
    // The reed: its data's three quads, in the cutout mesh with its plants stream.
    let m = mesh_of(&[(1, 1, 1, reed)]);
    assert_eq!(quads(&m.cutout), 3);
    assert_eq!(m.cutout.plants.len(), 3 * 4 * 4);
}

#[test]
fn draws_a_ladder_as_one_cutout_plate_near_the_back_of_its_cell() {
    // A north ladder faces −Z (face 5): the plate is at z = 1 − 0.05, in the cutout mesh (its data
    // says `"render": "cutout"`: the gaps between rails and rungs are see-through).
    let ladder = require("dwell:ladder[facing=north,flooded=false]");
    assert_eq!(dwell_blocks::info(ladder).render, RenderMode::Cutout);
    let m = mesh(&[(0, 0, 0, ladder)], false);
    assert_eq!(quads(&m.opaque), 0);
    let cutout = m.cutout;
    assert_eq!(quads(&cutout), 1);
    assert!(close(f64::from(cutout.positions[2]), 0.95, 2));
    assert_eq!(cutout.normals[2], -1.0);
    assert_eq!(cutout.plants, vec![0.0; 16], "no sway or fade");
}

#[test]
fn a_ladder_hides_no_face_of_its_neighbours() {
    // See-through: the stone behind it keeps the face the ladder stands against.
    let ladder = require("dwell:ladder[facing=north,flooded=false]");
    let alone = quads(&mesh(&[(0, 0, 1, stone())], false).opaque);
    let behind = quads(&mesh(&[(0, 0, 0, ladder), (0, 0, 1, stone())], false).opaque);
    assert_eq!(behind, alone);
}

#[test]
fn a_flooded_ladder_draws_the_water_around_it() {
    // In a pool, a flooded ladder's cell holds water: no hole in the water's surface or sides.
    let ladder = require("dwell:ladder[facing=north,flooded=true]");
    let pool = mesh(
        &[(0, 0, 0, water()), (1, 0, 0, ladder), (2, 0, 0, water())],
        false,
    );
    let dry = mesh(
        &[(0, 0, 0, water()), (1, 0, 0, 0), (2, 0, 0, water())],
        false,
    );
    // The water faces toward the ladder's cell are hidden, and the cell draws its own water.
    let top = |m: &MeshArrays| {
        (0..quads(m))
            .filter(|q| m.normals[q * 12 + 1] == 1.0)
            .map(|q| {
                let (lo, hi) = bounds(m, q);
                f64::from(hi[0] - lo[0]) * f64::from(hi[2] - lo[2])
            })
            .sum::<f64>()
    };
    assert!(
        (top(&pool.transparent) - 3.0).abs() < 0.02,
        "{}",
        top(&pool.transparent)
    );
    assert!((top(&dry.transparent) - 2.0).abs() < 1e-6);
    assert_eq!(quads(&pool.cutout), 1, "the plate");
}

fn tall_grass() -> u16 {
    require("dwell:tall_grass")
}

dwell_world::both_faces!(
    /// WORK_QUEUE item 30 (owner: "Ensure all of our graphics have settings in our graphics
    /// menu"): Graphics → Tall grass meshes `ChunkFlags::plants` percent of the cross plants: a
    /// field of grass draws all of them at 100, none at 0, and about half at 50, the half kept being
    /// the same cells whichever face it is on (red before the flag: every density drew them all).
    fn the_tall_grass_density_thins_the_cross_plants(f: FaceFrame) {
        let mut cells = Vec::new();
        for x in 0..32 {
            for z in 0..32 {
                cells.push((x, 7, z, cube_block()));
                cells.push((x, 8, z, tall_grass()));
            }
        }
        let padded = voxels(&cells);
        let quads_n = usize::from(
            dwell_blocks::info(tall_grass())
                .cross
                .expect("a cross")
                .quads,
        );
        let plants = |percent: u8| {
            let m = super::mesh_chunk_flags(
                &padded,
                &[],
                f.is_b(),
                [0; 3],
                &[],
                None,
                ChunkFlags {
                    plants: percent,
                    ..ChunkFlags::default()
                },
            )
            .expect("padded");
            m.cutout.positions.len() / 3 / 4 / quads_n
        };
        assert_eq!(plants(100), 32 * 32);
        assert_eq!(plants(0), 0);
        let half = plants(50);
        assert!((400..=624).contains(&half), "about half kept: {half}");
        assert!(plants(25) < half && half < plants(75));
    }
);

#[test]
fn draws_tall_grass_as_crossed_cutout_quads_from_its_data() {
    let info = dwell_blocks::info(tall_grass());
    let cross = info.cross.expect("the data gives it a cross");
    // On a cube (a natural block's top would lower it: see the sitting tests below).
    let m = mesh(&[(4, 0, 4, cube_block()), (4, 1, 4, tall_grass())], false);
    let c = &m.cutout;
    // One quad per cross quad (4 vertices, 2 triangles), drawn once: the pipeline is two-sided.
    let quads_n = usize::from(cross.quads);
    assert_eq!(c.positions.len(), quads_n * 4 * 3);
    assert_eq!(c.indices.len(), quads_n * 6);
    assert_eq!(c.plants.len(), quads_n * 4 * 4);
    // The grass block below keeps its top face: the plant hides nothing.
    let alone = quads(&mesh(&[(4, 0, 4, cube_block())], false).opaque);
    assert_eq!(quads(&m.opaque), alone);
    assert_eq!(quads(&m.transparent), 0);
    for v in 0..c.positions.len() / 3 {
        let p = vertex(&c.positions, v as u32);
        // Rooted on the block below, at most the data's height, near the cell.
        assert!(
            p[1] >= 1.0 - 1e-6 && p[1] <= 1.0 + f64::from(cross.height) + 1e-6,
            "{p:?}"
        );
        assert!(
            (p[0] - 4.5).abs() < 1.0 && (p[2] - 4.5).abs() < 1.0,
            "{p:?}"
        );
        // Normals lean up, so the blades are lit from either side.
        assert!(c.normals[v * 3 + 1] > 0.7);
        // Sway: none at the foot, the wind's strength (scaled by the plant's height) at the top.
        let sway = c.plants[v * 4];
        let wind = info.wind.expect("wind");
        if p[1] < 1.0 + 1e-6 {
            assert_eq!(sway, 0.0);
        } else {
            assert!(sway > 0.0 && sway <= wind.strength + 1e-6);
        }
        let fade = info.fade.expect("fade");
        assert_eq!(c.plants[v * 4 + 2..v * 4 + 4], [fade.start, fade.end]);
        assert_eq!(c.tiles[v * 4], tile_rect("tallGrass").u0 as f32);
    }
}

#[test]
fn tall_grass_jitter_is_deterministic_by_world_position() {
    let cells = [(4, 0, 4, grass()), (4, 1, 4, tall_grass())];
    let at = |origin| {
        mesh_chunk_at(&voxels(&cells), false, origin)
            .expect("meshed")
            .cutout
    };
    // The same plant every time it is meshed.
    assert_eq!(at([0, 0, 0]), at([0, 0, 0]));
    // A different place gets a different plant (offset, turn or height).
    let here = at([0, 0, 0]);
    let there = at([32, 0, -64]);
    assert_ne!(here.positions, there.positions);
    // Neighbouring plants differ too, within the data's jitter.
    let field: Vec<(i32, i32, i32, u16)> = (0..8)
        .flat_map(|x| [(x, 0, 0, cube_block()), (x, 1, 0, tall_grass())])
        .collect();
    let m = mesh_chunk_at(&voxels(&field), false, [0; 3])
        .expect("meshed")
        .cutout;
    let cross = dwell_blocks::info(tall_grass()).cross.expect("cross");
    let per = usize::from(cross.quads) * 4;
    let tops: Vec<f32> = (0..8)
        .map(|i| {
            (0..per)
                .map(|v| m.positions[(i * per + v) * 3 + 1])
                .fold(0.0, f32::max)
        })
        .collect();
    assert!(
        tops.windows(2).any(|w| w[0] != w[1]),
        "heights vary: {tops:?}"
    );
    for t in &tops {
        let h = t - 1.0;
        assert!(h >= cross.height * cross.height_min - 1e-5 && h <= cross.height + 1e-5);
    }
}

#[test]
fn ladders_and_plants_share_the_cutout_mesh_with_aligned_streams() {
    let ladder = require("dwell:ladder[facing=north,flooded=false]");
    let m = mesh(
        &[
            (0, 0, 0, ladder),
            (4, 0, 4, grass()),
            (4, 1, 4, tall_grass()),
            (9, 0, 0, ladder),
        ],
        false,
    );
    let c = &m.cutout;
    assert_eq!(c.plants.len() / 4, c.positions.len() / 3);
    assert_eq!(c.tiles.len() / 4, c.positions.len() / 3);
    // Mirrored for face B, it is still cutout and keeps its streams.
    let b = mesh(&[(4, 0, 4, grass()), (4, 1, 4, tall_grass())], true).cutout;
    assert_eq!(b.plants.len() / 4, b.positions.len() / 3);
    assert!(!b.indices.is_empty());
}

#[test]
fn textures_faces_by_material_and_face_grass_top_and_side_unknown_ids_the_fallback_tile() {
    // A grass block under a cube (so a cube itself: its sides).
    let cells = vec![
        (0, 0, 0, grass()),
        (0, 1, 0, cube_block()),
        (5, 0, 0, UNKNOWN),
    ];
    let opaque = mesh(&cells, false).opaque;
    let tile_of = |q: usize| [opaque.tiles[q * 16], opaque.tiles[q * 16 + 1]];
    let find = |pred: &dyn Fn(usize) -> bool| (0..quads(&opaque)).find(|q| pred(*q)).expect("quad");
    let grass_side = find(&|q| opaque.normals[q * 12] == 1.0 && bounds(&opaque, q).1[0] == 1.0);
    // The middle of a 3 x 3 field of grass: a flat half-height top with the grass top tile.
    let mut field = Vec::new();
    for z in 0..3 {
        for x in 10..13 {
            field.push((x, 0, z, grass()));
        }
    }
    let f = mesh(&field, false).opaque;
    let grass_top = (0..f.normals.len() / 3)
        .find(|&v| f.normals[v * 3 + 1] == 1.0)
        .expect("a flat top");
    let unknown = find(&|q| bounds(&opaque, q).0[0] >= 5.0);
    let rect = |name: &str| {
        let r = tile_rect(name);
        [r.u0 as f32, r.v0 as f32]
    };
    assert_eq!(
        [f.tiles[grass_top * 4], f.tiles[grass_top * 4 + 1]],
        rect("grass")
    );
    assert_eq!(tile_of(grass_side), rect("grassSide"));
    // Unknown ids draw with the fallback look (ADR 0038): the magenta and black checker tile.
    assert_eq!(tile_of(unknown), rect(crate::MISSING_TILE));
    // Textured faces shade white (the texture carries the colour).
    assert_eq!(f.colors[grass_top * 3], 1.0);
}

mod face_b {
    use super::*;

    fn tile_of(m: &MeshArrays, normal_y: f32) -> Option<[f64; 4]> {
        (0..m.normals.len() / 3)
            .find(|i| m.normals[i * 3 + 1] == normal_y)
            .map(|i| [0, 1, 2, 3].map(|c| f64::from(m.tiles[i * 4 + c])))
    }
    fn rect(name: &str) -> Option<[f64; 4]> {
        let t = tile_rect(name);
        Some([t.u0, t.v0, t.u1 - t.u0, t.v1 - t.v0])
    }
    fn colour(m: &MeshArrays, ny: f32) -> [f32; 3] {
        let i = (0..m.normals.len())
            .find(|&n| n % 3 == 1 && m.normals[n] == ny)
            .expect("a face");
        [0, 1, 2].map(|c| m.colors[i - 1 + c])
    }

    #[test]
    fn puts_the_top_texture_and_shading_on_the_face_toward_the_sky_which_on_face_b_is_minus_y() {
        // A grass block at the top of face A's chunk, and the same block hanging at the bottom of
        // a face-B chunk's row 0 (its sky is below).
        // (The middle of a 3 x 3 field: its top is flat, at half height.)
        let mut field = Vec::new();
        for z in 4..7 {
            for x in 4..7 {
                field.push((x, 0, z, grass()));
            }
        }
        let a = mesh(&field, false);
        let b = mesh(&field, true);
        assert_eq!(tile_of(&a.opaque, 1.0), rect("grass"));
        assert_eq!(tile_of(&a.opaque, -1.0), rect("dirt"));
        assert_eq!(tile_of(&b.opaque, -1.0), rect("grass"));
        assert_eq!(tile_of(&b.opaque, 1.0), rect("dirt"));
        // Shading: a face-B block's underside is lit as a top (warm), its upper face as a bottom.
        assert_eq!(colour(&b.opaque, -1.0), colour(&a.opaque, 1.0));
        assert_eq!(colour(&b.opaque, 1.0), colour(&a.opaque, -1.0));
    }

    #[test]
    fn winds_every_face_of_the_turned_over_mesh_to_face_its_normal() {
        let m = mesh(&[(5, 3, 5, grass())], true).opaque;
        assert!(!m.indices.is_empty());
        for t in m.indices.chunks_exact(3) {
            let n = normal_of(&m.positions, t[0], t[1], t[2]);
            let given = vertex(&m.normals, t[0]);
            let dot = n[0] * given[0] + n[1] * given[1] + n[2] * given[2];
            assert!(dot > 0.0);
        }
    }
}

#[test]
fn rejects_a_wrong_voxel_count_and_accounts_bytes() {
    // The TS throws a RangeError; `meshBuffers` (transferables) becomes a byte count here.
    assert_eq!(
        mesh_chunk(&[0u16; 10], false),
        Err(LengthError {
            expected: PADDED_VOLUME,
            got: 10
        })
    );
    let m = mesh(&[(1, 1, 1, cube_block())], false);
    // 24 vertices × (3 + 3 + 3 + 2 + 4 + 1 light + 2 flood) floats + 36 indices, 4 bytes each.
    assert_eq!(m.opaque.byte_len(), 4 * (24 * 18 + 36));
    assert_eq!(m.transparent.byte_len(), 0);
}

// --- world/shapes.test.ts -------------------------------------------------------------------

type V3 = [f64; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn pts(f: &ShapeFace) -> Vec<V3> {
    f.v[..usize::from(f.count)]
        .iter()
        .map(|p| p.map(f64::from))
        .collect()
}

/// Volume by the divergence theorem over the fan-triangulated outward faces.
fn volume_of(shape: &ShapeInfo) -> f64 {
    let mut six = 0.0;
    for f in faces_of(shape) {
        let p = pts(f);
        for k in 1..p.len() - 1 {
            six += dot(p[0], cross(p[k], p[k + 1]));
        }
    }
    six / 6.0
}

/// Every directed edge of the surface has its reverse on it exactly as often.
fn is_closed(shape: &ShapeInfo) -> bool {
    let mut edges = std::collections::HashMap::<String, i32>::new();
    let key = |a: V3, b: V3| format!("{a:?}>{b:?}");
    for f in faces_of(shape) {
        let p = pts(f);
        for i in 0..p.len() {
            *edges.entry(key(p[i], p[(i + 1) % p.len()])).or_default() += 1;
        }
    }
    for f in faces_of(shape) {
        let p = pts(f);
        for i in 0..p.len() {
            let (a, b) = (p[i], p[(i + 1) % p.len()]);
            if edges.get(&key(b, a)) != edges.get(&key(a, b)) {
                return false;
            }
        }
    }
    true
}

/// Convex iff every vertex lies on or inside every face's plane.
fn is_convex(shape: &ShapeInfo) -> bool {
    for f in faces_of(shape) {
        let p = pts(f);
        let n = cross(sub(p[1], p[0]), sub(p[2], p[0]));
        for g in faces_of(shape) {
            for q in pts(g) {
                if dot(n, sub(q, p[0])) > 1e-9 {
                    return false;
                }
            }
        }
    }
    true
}

fn shape_of(name: &str) -> &'static ShapeInfo {
    dwell_blocks::shape::shape_of(require(name))
}

#[test]
fn shape_table_describes_the_cube_and_the_empty_shape() {
    // The shaped families were cut with the baseline (Phase 15): the table is the empty shape and
    // the cube. Shaped blocks return as adaptive natural blocks (Phase 18).
    let s = shape_of("dwell:sandbox_floor");
    assert!((volume_of(s) - 1.0).abs() < 1e-9);
    assert!(is_closed(s));
    assert!(is_convex(s));
    assert!(faces_of(&dwell_blocks::SHAPES[0]).is_empty());
}

/// Whether (a, b) on the plane of `axis` lies inside a polygon (projected along the axis).
fn in_polygon(f: &ShapeFace, axis: usize, a: f64, b: f64) -> bool {
    let u = (axis + 1) % 3;
    let v = (axis + 2) % 3;
    let p = pts(f);
    let mut sign = 0;
    for i in 0..p.len() {
        let (s, q) = (p[i], p[(i + 1) % p.len()]);
        let c = q[u] - s[u];
        let cross2 = c * (b - s[v]) - (q[v] - s[v]) * (a - s[u]);
        if cross2.abs() < 1e-9 {
            continue;
        }
        let si = if cross2 > 0.0 { 1 } else { -1 };
        if sign == 0 {
            sign = si;
        } else if si != sign {
            return false;
        }
    }
    sign != 0
}

/// Is there solid on the plane of the cell face `face`, at in-plane point (a, b)?
fn solid_on_face(s: &ShapeInfo, face: usize, a: f64, b: f64) -> bool {
    let axis = face >> 1;
    if axis == 1 {
        return if face == 2 { s.full_top } else { s.full_bottom };
    }
    let x = if axis == 0 {
        if face == 0 { 1.0 } else { 0.0 }
    } else {
        a
    };
    let z = if axis == 0 {
        b
    } else if face == 4 {
        1.0
    } else {
        0.0
    };
    let y = if axis == 0 { a } else { b };
    let span = dwell_blocks::shape::solid_span_at(s, x as f32, z as f32);
    let (lo, hi) = (f64::from(span.lo), f64::from(span.hi));
    hi - lo > 1e-6 && y > lo + 1e-6 && y < hi - 1e-6
}

#[test]
fn shape_adjacency_leaves_no_holes_between_any_two_shapes_on_any_side() {
    // Every shape in the table (the cube and the ramp pieces, upright and hanging), against each
    // other on every side.
    let distinct: Vec<usize> = (1..dwell_blocks::SHAPES.len()).collect();
    assert!(distinct.len() > 4);
    const SAMPLES: usize = 5;
    let mut checked = 0;
    let mut failures = Vec::new();
    for &ia in &distinct {
        for &ib in &distinct {
            let (a, b) = (&dwell_blocks::SHAPES[ia], &dwell_blocks::SHAPES[ib]);
            for face in 0..6 {
                let axis = face >> 1;
                let opposite = face ^ 1;
                let covered_a = face_covered(a, face, Some(b));
                let covered_b = face_covered(b, opposite, Some(a));
                for i in 0..SAMPLES {
                    for j in 0..SAMPLES {
                        let pu = (i as f64 + 0.31) / SAMPLES as f64;
                        let pv = (j as f64 + 0.57) / SAMPLES as f64;
                        let count = |s: &ShapeInfo, tag: usize| {
                            faces_of(s)
                                .iter()
                                .filter(|f| {
                                    usize::from(f.tag) == tag && in_polygon(f, axis, pu, pv)
                                })
                                .count()
                        };
                        let drawn = (if covered_a { 0 } else { count(a, face) })
                            + (if covered_b { 0 } else { count(b, opposite) });
                        let in_a = solid_on_face(a, face, pu, pv);
                        let in_b = solid_on_face(b, opposite, pu, pv);
                        checked += 1;
                        // Solid on exactly one side: that surface is drawn once. Neither: nothing.
                        if ((in_a != in_b && drawn != 1) || (!in_a && !in_b && drawn != 0))
                            && failures.len() < 5
                        {
                            failures.push(format!(
                                "{ia} | {ib} face {face} at {pu},{pv}: drawn {drawn}"
                            ));
                        }
                    }
                }
            }
        }
    }
    assert_eq!(failures, Vec::<String>::new());
    assert!(checked > 0);
}

// --- adaptive natural blocks (ADAPTIVE_BLOCKS.md) --------------------------------------------

/// The opaque triangles' unit normals' y, rounded to 3 places.
fn normal_ys(m: &MeshArrays) -> Vec<i32> {
    m.indices
        .chunks_exact(3)
        .map(|t| {
            let n = normal_of(&m.positions, t[0], t[1], t[2]);
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            (n[1] / len * 1000.0).round() as i32
        })
        .collect()
}

#[test]
fn a_lone_grass_block_is_a_cube_and_a_field_is_flat_at_its_top() {
    // Natural blocks are not adaptive on the half-height grid: no half pyramid, no resting at half
    // height (Phase 18); a lone block is a cube.
    let lone = mesh(&[(5, 3, 5, grass())], false).opaque;
    let ys = normal_ys(&lone);
    assert!(ys.iter().all(|&y| [1000, 0, -1000].contains(&y)), "{ys:?}");
    assert_eq!(ys.iter().filter(|&&y| y == 1000).count(), 2, "{ys:?}");
    assert!(lone.positions.chunks_exact(3).any(|p| p[1] == 4.0));
    let mut field = Vec::new();
    for z in 4..7 {
        for x in 4..7 {
            field.push((x, 3, z, grass()));
        }
    }
    let m = mesh(&field, false).opaque;
    let flat: Vec<f32> = (0..m.normals.len() / 3)
        .filter(|&i| m.normals[i * 3 + 1] == 1.0)
        .map(|i| m.positions[i * 3 + 1])
        .collect();
    assert!(
        !flat.is_empty() && flat.iter().all(|&y| y == 4.0),
        "{flat:?}"
    );
}

#[test]
fn a_grass_block_with_a_block_on_it_stays_a_cube() {
    let m = mesh(&[(5, 3, 5, grass()), (5, 4, 5, cube_block())], false).opaque;
    // Two cubes stacked: 10 quads, every triangle axis-aligned.
    assert_eq!(quads(&m), 10);
    assert!(normal_ys(&m).iter().all(|&y| y == 0 || y.abs() == 1000));
}

#[test]
fn a_level_field_of_natural_blocks_merges_its_flat_tops() {
    // 10 × 10 grass: natural blocks are not adaptive on the half-height grid (Phase 41), so the
    // whole field is flat at its cells' top in one rectangle (under Phase 18 the rim sloped and the
    // inside rested at half height).
    let mut field = Vec::new();
    for z in 0..10 {
        for x in 0..10 {
            field.push((x, 2, z, grass()));
        }
    }
    let m = mesh(&field, false).opaque;
    let flat: Vec<usize> = (0..m.normals.len() / 3)
        .filter(|&v| m.normals[v * 3 + 1] == 1.0)
        .collect();
    assert_eq!(flat.len(), 4, "one merged quad");
    for &v in &flat {
        assert_eq!(m.positions[v * 3 + 1], 3.0);
    }
}

/// A stored ramp state of the half-height grid (Phase 41, docs/HALF_HEIGHT_GRID.md §3).
fn ramp(material: &str, shape: &str, facing: &str, half: &str, flooded: bool) -> u16 {
    dwell_blocks::state(&format!(
        "dwell:{material}_ramp[facing={facing},flooded={flooded},half={half},shape={shape}]"
    ))
    .expect("a ramp state")
}

#[test]
fn stored_ramp_pieces_mesh_their_own_shape() {
    // A ½ m ramp descending east: its sloped top rises toward −x; the two 45° halves stacked make
    // one slope, the upper one open beyond its middle (no floor drawn there).
    let r = mesh(
        &[(5, 3, 5, ramp("grass", "ramp", "east", "bottom", false))],
        false,
    )
    .opaque;
    let ys = normal_ys(&r);
    assert!(ys.contains(&707), "{ys:?}");
    let upper = ramp("grass", "steep_upper", "east", "bottom", false);
    let lower = ramp("grass", "steep_lower", "east", "bottom", false);
    let m = mesh(&[(5, 3, 5, lower), (5, 4, 5, upper)], false).opaque;
    // Every sloped triangle of the pair lies in the one plane through (5, 5) and (6, 3) in x–y.
    for t in m.indices.chunks_exact(3) {
        let n = normal_of(&m.positions, t[0], t[1], t[2]);
        if n[0] > 0.0 && n[1] > 0.0 {
            for &v in t {
                let p = vertex(&m.positions, v);
                assert!((p[1] - (5.0 - 2.0 * (p[0] - 5.0))).abs() < 1e-5, "{p:?}");
            }
        }
    }
    // Nothing is drawn in the upper cell's open half at its floor (y = 4, x > 5.5).
    let floor_in_open_half = m.indices.chunks_exact(3).any(|t| {
        t.iter().all(|&v| {
            let p = vertex(&m.positions, v);
            (p[1] - 4.0).abs() < 1e-6 && p[0] > 5.5 - 1e-6
        })
    });
    assert!(!floor_in_open_half);
}

#[test]
fn a_flooded_ramp_beside_water_holds_water_in_its_open_part() {
    // A sand ramp descending into water at the water's row, stored flooded (the generator sets it;
    // HALF_HEIGHT_GRID.md §5): its cell's water faces are drawn (toward the air), and the water
    // beside it draws none against it.
    let m = mesh(
        &[
            (5, 3, 5, ramp("sand", "ramp", "east", "bottom", true)),
            (6, 3, 5, water()),
        ],
        false,
    );
    let tops: Vec<f32> = (0..m.transparent.normals.len() / 3)
        .filter(|&i| m.transparent.normals[i * 3 + 1] == 1.0)
        .map(|i| m.transparent.positions[i * 3])
        .collect();
    // The bank's water top spans x 5..6 and the water cell's 6..7.
    assert!(tops.iter().any(|&x| x < 6.0), "{tops:?}");
    assert!(tops.iter().any(|&x| x >= 6.0), "{tops:?}");
    // No water face stands between them (at x = 6, facing ±x).
    let between = (0..m.transparent.normals.len() / 3).any(|i| {
        m.transparent.normals[i * 3].abs() == 1.0
            && (m.transparent.positions[i * 3] - 6.0).abs() < 0.01
    });
    assert!(!between);
}

/// A shore: water at row 3 east of x = 6, sand banks west of it (some a block higher, for step
/// slopes and ramps), a dry bank cell that only touches water diagonally, and water open to the
/// air at its south edge.
fn shore_scene() -> Vec<(i32, i32, i32, u16)> {
    let sand = require("dwell:sand");
    let mut cells = Vec::new();
    for z in 2..10 {
        for x in 0..12 {
            cells.push((x, 2, z, sand));
            let bank = x < 6 || (x == 6 && z == 5);
            cells.push((x, 3, z, if bank { sand } else { water() }));
            if x < 3 || (x == 4 && z == 7) {
                cells.push((x, 4, z, sand));
            }
        }
    }
    // A dry bank cell beside a bank cell whose only water is diagonal.
    cells.retain(|&(x, y, z, _)| !(y == 3 && (x == 6 || x == 7) && z == 9));
    cells.push((6, 3, 9, sand));
    cells
}

#[test]
fn fluid_side_faces_stand_only_over_the_open_part_beside_them_and_under_the_fluids_surface() {
    //! Regression (owner, 2026-10-20: flat blue walls along shores): a water cell's and a flooded
    //! bank's side faces were drawn over the whole cell side, up to the cell's top (above the
    //! water's drawn surface) and over the solid part of a shaped neighbour (behind its slope or
    //! ramp). A fluid side face now covers only the part of its side where the neighbour is open,
    //! up to the fluid's drawn surface (WATER_RENDERING.md §9).
    let cells = shore_scene();
    let at = |x: i32, y: i32, z: i32| {
        cells
            .iter()
            .rev()
            .find(|c| (c.0, c.1, c.2) == (x, y, z))
            .map_or(0, |c| c.3)
    };
    let m = mesh(&cells, false).transparent;
    let water_id = f32::from(dwell_blocks::fluid("dwell:water").expect("water"));
    let top = 1.0 - dwell_blocks::require_fluid("dwell:water").surface_drop;
    let mut sides = 0;
    for t in m.indices.chunks_exact(3) {
        let [a, b, c] = [t[0], t[1], t[2]].map(|i| vertex(&m.positions, i));
        // Every fluid face carries the water's fluid id (the water shader's parameters).
        for &i in t {
            assert_eq!(m.fluids[i as usize], water_id);
        }
        let n = [
            f64::from(m.normals[t[0] as usize * 3]),
            f64::from(m.normals[t[0] as usize * 3 + 1]),
            f64::from(m.normals[t[0] as usize * 3 + 2]),
        ];
        if n[1] != 0.0 {
            continue;
        }
        sides += 1;
        let centre = [0, 1, 2].map(|i| (a[i] + b[i] + c[i]) / 3.0);
        // The face's own cell: its side stops at the drawn surface unless water is over it.
        let own = [0, 1, 2].map(|i| (centre[i] - n[i] * 0.01).floor() as i32);
        let over = |dx: i32, dy: i32, dz: i32| at(own[0] + dx, own[1] + 1 + dy, own[2] + dz);
        let wet_above = over(0, 0, 0) == water()
            || dwell_blocks::try_info(over(0, 0, 0)).is_some_and(|s| s.flooded);
        let limit = f64::from(own[1]) + if wet_above { 1.0 } else { top };
        for p in [a, b, c] {
            assert!(
                p[1] <= limit + 1e-5,
                "a side face above the water's surface: {a:?} {b:?} {c:?}"
            );
        }
        // Just across the face: the neighbour cell, which must be open there.
        let q = [0, 1, 2].map(|i| centre[i] + n[i] * 0.01);
        let [qx, qy, qz] = q.map(|v| v.floor() as i32);
        let shape = dwell_blocks::shape::shape_of(at(qx, qy, qz));
        let local = [
            q[0] - f64::from(qx),
            q[1] - f64::from(qy),
            q[2] - f64::from(qz),
        ]
        .map(|v| v as f32);
        assert!(
            !dwell_blocks::shape::point_in_solid(shape, local[0], local[1], local[2], -1e-3),
            "a side face against its neighbour's solid at {q:?}: {a:?} {b:?} {c:?}"
        );
    }
    assert!(
        sides > 0,
        "the water's open south edge and the dry bank show sides"
    );
}

// face-b-only: a hanging ramp is face B's (face A's upright ramps are tested above)
#[test]
fn a_hanging_ramp_on_face_b_faces_its_sky() {
    // A hanging ramp on face B (meshed mirrored): its slope faces down, toward face B's sky.
    let m = mesh(
        &[(5, 3, 5, ramp("grass", "ramp", "east", "top", false))],
        true,
    )
    .opaque;
    let ys = normal_ys(&m);
    assert!(ys.contains(&-707), "{ys:?}");
    assert!(!ys.contains(&707), "{ys:?}");
}

#[test]
fn emits_metres_one_cell_tall() {
    // A cube spans the cell height in y (HALF_HEIGHT_GRID.md §2), and a ½ m ramp's stored normal
    // is its slope in metres (26.57°, normal y = cos 26.57° ≈ 0.894), not the cell-space 45°.
    let m = super::mesh_chunk(&voxels(&[(4, 3, 4, stone())]), false).expect("padded volume");
    let ys: Vec<f32> = m
        .opaque
        .positions
        .iter()
        .skip(1)
        .step_by(3)
        .copied()
        .collect();
    let h = dwell_world::CELL_H as f32;
    assert_eq!(ys.iter().copied().fold(f32::MAX, f32::min), 3.0 * h);
    assert_eq!(ys.iter().copied().fold(f32::MIN, f32::max), 4.0 * h);
    let r = super::mesh_chunk(
        &voxels(&[(4, 3, 4, ramp("grass", "ramp", "east", "bottom", false))]),
        false,
    )
    .expect("padded volume");
    let slope_y = (0..r.opaque.normals.len() / 3)
        .map(|v| r.opaque.normals[v * 3 + 1])
        .filter(|&y| y > 0.1 && y < 0.99)
        .fold(0.0f32, f32::max);
    let want = (1.0 / (1.0 + (h as f64).powi(2)).sqrt()) as f32;
    assert!((slope_y - want).abs() < 1e-3, "{slope_y} vs {want}");
}

#[test]
fn a_side_texture_spans_its_metres_and_a_grass_fringe_hangs_from_each_blocks_top() {
    // A 1 m tile spans 1 m of side whatever the cell height, and v runs down from the side's top
    // edge, which sits on a tile boundary: a ½ m grass block in any row shows the tile's top half,
    // the fringe at its top (HALF_HEIGHT_GRID.md §6).
    for row in [3, 4] {
        let m = super::mesh_chunk(&voxels(&[(4, row, 4, grass())]), false).expect("padded volume");
        let side: Vec<f32> = (0..m.opaque.normals.len() / 3)
            .filter(|&v| m.opaque.normals[v * 3] == 1.0)
            .map(|v| m.opaque.uvs[v * 2 + 1])
            .collect();
        assert_eq!(side.len(), 4, "one side quad");
        let (lo, hi) = (
            side.iter().copied().fold(f32::MAX, f32::min),
            side.iter().copied().fold(f32::MIN, f32::max),
        );
        assert_eq!(
            hi.fract(),
            0.0,
            "row {row}: the top edge on a tile boundary"
        );
        assert!(
            ((hi - lo) as f64 - dwell_world::CELL_H).abs() < 1e-6,
            "row {row}"
        );
    }
}

/// The tile (u0, v0) of the side faces facing +X at plane `x`, by each cell (y, z) a triangle's
/// bounds cover (a merged quad covers several).
fn plus_x_tiles(m: &MeshArrays, x: f32) -> Vec<((i32, i32), [f32; 2])> {
    let mut out = Vec::new();
    for t in m.indices.chunks_exact(3) {
        let v = t[0] as usize;
        if m.normals[v * 3] != 1.0 {
            continue;
        }
        let at = |i: u32, axis: usize| m.positions[i as usize * 3 + axis];
        if t.iter().any(|&i| (at(i, 0) - x).abs() > 1e-4) {
            continue;
        }
        let range = |axis: usize| {
            let lo = t.iter().map(|&i| at(i, axis)).fold(f32::INFINITY, f32::min);
            let hi = t
                .iter()
                .map(|&i| at(i, axis))
                .fold(f32::NEG_INFINITY, f32::max);
            lo.floor() as i32..hi.ceil() as i32
        };
        let tile = [m.tiles[v * 4], m.tiles[v * 4 + 1]];
        for y in range(1) {
            for z in range(2) {
                if !out.contains(&((y, z), tile)) {
                    out.push(((y, z), tile));
                }
            }
        }
    }
    out.sort_by_key(|e| e.0);
    out
}

fn rect_of(name: &str) -> [f32; 2] {
    let r = tile_rect(name);
    [r.u0 as f32, r.v0 as f32]
}

/// A 3 × 5 plateau (x 3..=5, z 3..=7) two cells deep of `body` with `top` on it (and `cap` on the
/// top if any; 0: none), ending in a cliff at x = 6: the +X faces of its edge column are exposed.
fn plateau(body: u16, top: u16, cap: u16) -> Vec<(i32, i32, i32, u16)> {
    let mut cells = Vec::new();
    for z in 3..=7 {
        for x in 3..=5 {
            cells.push((x, 0, z, body));
            cells.push((x, 1, z, body));
            if top != 0 {
                cells.push((x, 2, z, top));
            }
            if cap != 0 {
                cells.push((x, 3, z, cap));
            }
        }
    }
    cells
}

#[test]
fn a_dirt_side_under_sloped_grass_shows_the_turf_skirt_and_not_under_air_or_stone() {
    // dirt's `sideUnder` (shared/blocks/dwell.json): a dirt side face with grass directly above
    // takes the `dirtSkirt` tile. At a cliff the grass's edge is a slope down to the dirt's top
    // (on the half-height grid a stored grass ramp descending east, Phase 41).
    let dirt = require("dwell:dirt");
    let edge_ramp = |cells: Vec<(i32, i32, i32, u16)>| -> Vec<(i32, i32, i32, u16)> {
        let r = ramp("grass", "ramp", "east", "bottom", false);
        cells
            .into_iter()
            .map(|(x, y, z, m)| (x, y, z, if x == 5 && m == grass() { r } else { m }))
            .collect()
    };
    let m = mesh(&edge_ramp(plateau(dirt, grass(), 0)), false).opaque;
    let faces = plus_x_tiles(&m, 6.0);
    for z in 3..=7 {
        assert!(
            faces.contains(&((1, z), rect_of("dirtSkirt"))),
            "z {z}: {faces:?}"
        );
        assert!(
            faces.contains(&((0, z), rect_of("dirt"))),
            "z {z}: {faces:?}"
        );
        assert!(
            !faces.contains(&((1, z), rect_of("dirt"))),
            "z {z}: {faces:?}"
        );
    }
    // The grass is a slope there: it shows no side of its own above the dirt's face.
    assert!(faces.iter().all(|e| e.0.0 < 2), "{faces:?}");
    // Under air (dirt is the top block) and under stone: plain dirt.
    for (top, what) in [(0, "air"), (stone(), "stone")] {
        let faces = plus_x_tiles(&mesh(&plateau(dirt, top, 0), false).opaque, 6.0);
        assert!(!faces.is_empty(), "{what}");
        assert!(
            faces.iter().all(|e| e.1 != rect_of("dirtSkirt")),
            "under {what}: {faces:?}"
        );
    }
    // Under a full grass cube (a block on it), the grass side already hangs its skirt: the dirt
    // below stays plain, never a second skirt.
    let faces = plus_x_tiles(&mesh(&plateau(dirt, grass(), stone()), false).opaque, 6.0);
    assert!(faces.contains(&((2, 5), rect_of("grassSide"))), "{faces:?}");
    assert!(faces.contains(&((1, 5), rect_of("dirt"))), "{faces:?}");
    assert!(
        faces.iter().all(|e| e.1 != rect_of("dirtSkirt")),
        "{faces:?}"
    );
    // Face B (meshed mirrored; its sky is toward −y): the plateau hangs from the chunk's top with
    // the grass under the dirt, and the skirt hangs from the edge toward its sky.
    let hanging: Vec<_> = edge_ramp(plateau(dirt, grass(), 0))
        .into_iter()
        .map(|(x, y, z, m)| (x, N - 1 - y, z, m))
        .collect();
    let b = mesh(&hanging, true).opaque;
    assert!(
        (0..b.tiles.len() / 4)
            .any(|v| [b.tiles[v * 4], b.tiles[v * 4 + 1]] == rect_of("dirtSkirt")),
        "face B"
    );
}

#[test]
fn a_side_tile_rule_added_by_data_alone_is_applied() {
    // shared/blocks/fixtures/test.json adds `test:loam` (sand tiles) with `sideUnder`
    // `{ "dwell:stone": "grassSide" }` and nothing else. (On the half-height grid the stone over
    // it is a stored ramp: a full stone cube hangs no skirt, it shows its own side.)
    use dwell_blocks::fixture::{FLUIDS, STATES};
    let id = |name: &str| STATES.iter().position(|s| s.name == name).expect(name) as u16;
    let (loam, stone, water) = (
        id("test:loam"),
        id("dwell:stone_ramp[facing=east,flooded=false,half=bottom,shape=ramp]"),
        id("dwell:water[fill=8]"),
    );
    let kinds = Kinds::new(&STATES, &FLUIDS, Some(water));
    let mesh_of = |cells: &[(i32, i32, i32, u16)]| {
        mesh_chunk_with(&kinds, &voxels(cells), false, [0; 3])
            .expect("padded volume")
            .opaque
    };
    let faces = plus_x_tiles(&mesh_of(&[(1, 1, 1, loam), (1, 2, 1, stone)]), 2.0);
    assert!(faces.contains(&((1, 1), rect_of("grassSide"))), "{faces:?}");
    let faces = plus_x_tiles(&mesh_of(&[(1, 1, 1, loam)]), 2.0);
    assert_eq!(faces, [((1, 1), rect_of("sand"))]);
}

#[test]
fn the_cutout_mesh_carries_which_vertices_take_no_ink() {
    // Owner's report (2026-10-20, `?outline=creases`): tall grass was mostly black. Block data
    // `outline` (cross plants default to false) reaches the frame through the cutout mesh's
    // `bare` stream: 1 on a plant that takes no ink, 0 on a ladder, a cutout cube and a cross
    // that says `outline: true` (`test:reed`, by data alone).
    let ladder = require("dwell:ladder[facing=north,flooded=false]");
    let m = mesh(
        &[
            (4, 0, 4, grass()),
            (4, 1, 4, tall_grass()),
            (6, 1, 6, ladder),
        ],
        false,
    );
    let c = &m.cutout;
    assert_eq!(c.bare.len(), c.positions.len() / 3);
    let plant = c.bare.iter().filter(|&&b| b == 1.0).count();
    assert_eq!(
        plant,
        c.positions.len() / 3 - 4,
        "every plant vertex, not the ladder's 4"
    );
    assert!(!dwell_blocks::info(tall_grass()).outline);
    use dwell_blocks::fixture::{FLUIDS, STATES};
    let id = |name: &str| STATES.iter().position(|s| s.name == name).expect(name) as u16;
    let kinds = Kinds::new(&STATES, &FLUIDS, Some(id("dwell:water[fill=8]")));
    for name in ["test:reed", "test:hedge"] {
        let m = mesh_chunk_with(&kinds, &voxels(&[(1, 1, 1, id(name))]), false, [0; 3])
            .expect("padded volume");
        assert!(!m.cutout.bare.is_empty(), "{name}");
        assert!(m.cutout.bare.iter().all(|&b| b == 0.0), "{name} is inked");
    }
}

/// A cross's base vertices (the foot of its texture), in the cutout mesh's order.
fn plant_feet(c: &MeshArrays) -> Vec<[f64; 3]> {
    (0..c.positions.len() / 3)
        .filter(|&v| c.uvs[v * 2 + 1] < 0.5)
        .map(|v| vertex(&c.positions, v as u32))
        .collect()
}

/// The height of the surface of the support cell `s` (chunk-local) at world (x, z), as
/// the terrain's queries see it: `cells` read as the padded voxels.
fn surface_under(cells: &[(i32, i32, i32, u16)], s: [i32; 3], x: f64, z: f64) -> f64 {
    let padded = voxels(cells);
    let shape = dwell_blocks::shape::shape_of(padded[padded_index(s[0], s[1], s[2])]);
    let span = dwell_blocks::shape::solid_span_at(
        shape,
        (x - f64::from(s[0])) as f32,
        (z - f64::from(s[2])) as f32,
    );
    f64::from(s[1]) + f64::from(span.hi)
}

/// A plant at (4, 1, 4) on a grass ramp (a ½ m ramp descending east) at (4, 0, 4) in a grass field:
/// the one shaped support a plant can meet on ½ m cells (`sitOnSupport`, TRANSPARENT_BLOCKS.md §4).
fn ramp_with_plant(plant: u16) -> Vec<(i32, i32, i32, u16)> {
    let ramp =
        dwell_world::ramps::gentle_cell("dwell:grass", dwell_world::ramps::RampFacing::East, false)
            .expect("a grass ramp on ½ m cells");
    let mut cells: Vec<_> = (3..6)
        .flat_map(|z| (3..6).map(move |x| (x, 0, z, grass())))
        .filter(|&(x, _, z, _)| (x, z) != (4, 4))
        .collect();
    cells.push((4, 0, 4, ramp));
    cells.push((4, 1, 4, plant));
    cells
}

#[test]
fn tall_grass_over_a_ramp_follows_its_height_under_each_foot() {
    let cells = ramp_with_plant(tall_grass());
    let feet = plant_feet(&mesh(&cells, false).cutout);
    assert!(!feet.is_empty());
    let mut heights = Vec::new();
    for p in &feet {
        let want = surface_under(&cells, [4, 0, 4], p[0], p[2]);
        assert!((p[1] - want).abs() < 1e-4, "{p:?} want {want}");
        heights.push(p[1]);
    }
    assert!(
        heights.iter().any(|&h| (h - heights[0]).abs() > 0.05),
        "the feet follow the ramp: {heights:?}"
    );
}

#[test]
fn tall_grass_on_a_full_cube_is_unchanged() {
    let c = mesh(&[(4, 0, 4, cube_block()), (4, 1, 4, tall_grass())], false).cutout;
    for p in plant_feet(&c) {
        assert!((p[1] - 1.0).abs() < 1e-6, "{p:?}");
    }
}

#[test]
fn a_plant_that_does_not_sit_on_its_support_keeps_to_its_cells_floor_by_data_alone() {
    // shared/blocks/fixtures/test.json's `test:reed` says `"sitOnSupport": false`; the fixture's
    // tall grass keeps the default. Both over the same ramp.
    use dwell_blocks::fixture::{FLUIDS, STATES};
    let id = |name: &str| STATES.iter().position(|s| s.name == name).expect(name) as u16;
    let (reed, plant, water) = (
        id("test:reed"),
        id("dwell:tall_grass[stage=3]"),
        id("dwell:water[fill=8]"),
    );
    // The fixture keeps the build's blocks first, so the build's ramp states are the fixture's.
    let ground = id("dwell:grass[flooded=false]");
    assert_eq!(
        STATES[usize::from(ground)].name,
        dwell_blocks::STATES[usize::from(ground)].name
    );
    let kinds = Kinds::new(&STATES, &FLUIDS, Some(water));
    let feet_of = |p: u16| {
        plant_feet(
            &mesh_chunk_with(&kinds, &voxels(&ramp_with_plant(p)), false, [0; 3])
                .expect("padded volume")
                .cutout,
        )
    };
    let reed_feet = feet_of(reed);
    assert!(!reed_feet.is_empty());
    assert!(
        reed_feet.iter().all(|p| (p[1] - 1.0).abs() < 1e-6),
        "{reed_feet:?}"
    );
    assert!(feet_of(plant).iter().any(|p| (p[1] - 1.0).abs() > 0.05));
}

// --- Face parity (BIFACIAL_WORLD.md §11) --------------------------------------------------------

/// A generated face-A chunk and its sky apron copied onto `f`'s face as the face-local mirror
/// image, then gathered as the client does (the mesh block from the world) and meshed for that
/// face.
fn mesh_on_face(source: &mut dwell_world::VoxelWorld, c: ChunkCoord, f: FaceFrame) -> ChunkMeshes {
    let mut world = dwell_world::VoxelWorld::new(dwell_world::empty_generator(), None);
    let n = CHUNK_SIZE;
    let o = [c.x * n, c.y * n, c.z * n];
    let [ax, ay, az] = dwell_world::SKY_APRON;
    for y in -ay..n + ay {
        for z in -az..n + az {
            for x in -ax..n + ax {
                let m = source.get_voxel(o[0] + x, o[1] + y, o[2] + z);
                let [wx, wy, wz] = f.cell([o[0] + x, o[1] + y, o[2] + z]);
                world.set_voxel(wx, wy, wz, f.state(m));
            }
        }
    }
    let wc = f.chunk(c);
    let padded = world.padded_box(wc, dwell_world::SKY_APRON);
    let mirror = FaceFrame::at_chunk_y(wc.y).is_b();
    assert_eq!(mirror, f.is_b());
    mesh_chunk_at(&padded, mirror, [wc.x * n, wc.y * n, wc.z * n]).expect("padded")
}

/// Positions and normals turned into the face-local frame (y within the chunk mirrored on B,
/// triangles re-wound), so meshes of the two faces compare.
fn local_mesh(f: FaceFrame, mut m: MeshArrays) -> MeshArrays {
    if f.is_b() {
        for p in m.positions.iter_mut().skip(1).step_by(3) {
            *p = CHUNK_SIZE as f32 - *p;
        }
        for v in m.normals.iter_mut().skip(1).step_by(3) {
            *v = -*v;
        }
        // A leaf card's packed plane normal (`leaves::card_speed`, negative) is a normal too.
        for speed in m.plants.iter_mut().skip(1).step_by(4) {
            if *speed < 0.0 {
                let n = leaves::card_normal_of(*speed);
                *speed = leaves::card_speed([n[0], -n[1], n[2]]);
            }
        }
        for t in m.indices.chunks_exact_mut(3) {
            t.swap(1, 2);
        }
    }
    m
}

dwell_world::both_faces!(
    /// The chunk mesher draws a face-B chunk as the exact mirror image of the face-A chunk it
    /// mirrors — terrain shapes, grass skirts, plants (their jitter too), trees, water and shores,
    /// and their baked ambient light — gathered from the world the way the client gathers it.
    fn a_chunk_meshes_the_same_face_local_on_either_face(f: FaceFrame) {
        let g = dwell_worldgen::generator_for(dwell_world::GENERATOR_TERRAIN, 7);
        let air = dwell_worldgen::air_test_for(dwell_world::GENERATOR_TERRAIN, 7);
        let mut source = dwell_world::VoxelWorld::new(g, Some(air));
        let spawn = dwell_worldgen::spawn_point_for(dwell_world::GENERATOR_TERRAIN, 7);
        let at = |x: f64, y: f64, z: f64| {
            ChunkCoord::new(
                (x / 32.0).floor() as i32,
                (y / (32.0 * dwell_world::CELL_H)).floor() as i32,
                (z / 32.0).floor() as i32,
            )
        };
        let s = at(spawn[0], spawn[1] - 1.0, spawn[2]);
        let mut chunks = vec![
            s,
            ChunkCoord::new(s.x + 1, s.y, s.z),
            ChunkCoord::new(s.x, s.y, s.z - 1),
        ];
        // A shore: the first column east of the spawn at sea level.
        let t = dwell_worldgen::BifacialTerrain::new(7);
        let local = t.local(dwell_world::Face::A);
        let mut x = spawn[0] as i32;
        while local.column_at(x, spawn[2] as i32).height >= dwell_world::SEA_LEVEL_M as f32 {
            x += 8;
        }
        chunks.push(at(f64::from(x), dwell_world::SEA_LEVEL_M - 0.25, spawn[2]));
        for c in chunks {
            let a = mesh_on_face(&mut source, c, FaceFrame::A);
            let b = mesh_on_face(&mut source, c, f);
            assert!(!a.opaque.indices.is_empty(), "{c:?} is empty");
            for (name, ma, mb) in [
                ("opaque", a.opaque, b.opaque),
                ("transparent", a.transparent, b.transparent),
                ("cutout", a.cutout, b.cutout),
            ] {
                let mb = local_mesh(f, mb);
                for (field, x, y) in [
                    ("positions", &mb.positions, &ma.positions),
                    ("normals", &mb.normals, &ma.normals),
                    ("colors", &mb.colors, &ma.colors),
                    ("uvs", &mb.uvs, &ma.uvs),
                    ("tiles", &mb.tiles, &ma.tiles),
                    ("plants", &mb.plants, &ma.plants),
                    ("bare", &mb.bare, &ma.bare),
                    ("light", &mb.light, &ma.light),
                    ("fluids", &mb.fluids, &ma.fluids),
                ] {
                    // To float rounding: the mirror's positions are turned in f64 and back.
                    let i = x
                        .iter()
                        .zip(y.iter())
                        .position(|(p, q)| (p - q).abs() > 1e-4);
                    assert!(
                        x.len() == y.len() && i.is_none(),
                        "{c:?} {name} {field}: {:?} lens {} {} at {i:?}: {:?} vs {:?}",
                        f.face,
                        x.len(),
                        y.len(),
                        i.map(|i| &x[i.saturating_sub(3)..(i + 3).min(x.len())]),
                        i.map(|i| &y[i.saturating_sub(3)..(i + 3).min(y.len())])
                    );
                }
                assert_eq!(mb.indices, ma.indices, "{c:?} {name} indices: {:?}", f.face);
            }
        }
    }
);

// --- Texture phase: a face's uv is its place's, not its merged quad's ----------------------------

/// The uv at face-local point `p` (cells) on the face facing `axis`/`sign` whose plane holds `p`,
/// interpolated in the triangle that covers it; None where no face is drawn.
fn uv_at(m: &MeshArrays, axis: usize, sign: f32, p: [f64; 3]) -> Option<[f64; 2]> {
    let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
    for t in m.indices.chunks_exact(3) {
        let t: [u32; 3] = [t[0], t[1], t[2]];
        let i = t[0] as usize;
        if m.normals[i * 3 + axis] != sign {
            continue;
        }
        let q = t.map(|v| vertex(&m.positions, v));
        if q.iter().any(|q| (q[axis] - p[axis]).abs() > 1e-4) {
            continue;
        }
        // Barycentric coordinates of p in the triangle, in the plane's two axes.
        let (x0, y0) = (q[0][a], q[0][b]);
        let (x1, y1) = (q[1][a] - x0, q[1][b] - y0);
        let (x2, y2) = (q[2][a] - x0, q[2][b] - y0);
        let (px, py) = (p[a] - x0, p[b] - y0);
        let det = x1 * y2 - x2 * y1;
        let l1 = (px * y2 - x2 * py) / det;
        let l2 = (x1 * py - px * y1) / det;
        let l0 = 1.0 - l1 - l2;
        if l0 < -1e-9 || l1 < -1e-9 || l2 < -1e-9 {
            continue;
        }
        let uv = |k: usize| t.map(|v| f64::from(m.uvs[v as usize * 2 + k]));
        let (u, v) = (uv(0), uv(1));
        return Some([
            l0 * u[0] + l1 * u[1] + l2 * u[2],
            l0 * v[0] + l1 * v[1] + l2 * v[2],
        ]);
    }
    None
}

dwell_world::both_faces!(
    /// Breaking a block leaves every other face's texture where it was (owner's playtest,
    /// 2026-10-21: the wall block beside a broken one changed its tiling). Faces merge into
    /// rectangles, and an edit changes how they merge, so each face's uv must come from where it
    /// is (world metres, or a hanging tile's own block top), never from its quad's corner: on ½ m
    /// cells a quad starting on an odd row shifted a 1 m tile by half. A 9 × 13 × 9 block of dirt
    /// capped with grass, with a block broken out of each wall and the top in an odd and an even
    /// row (½ m and 1 m aligned), sampled at four points of every face still drawn.
    fn breaking_a_block_leaves_the_texture_of_every_other_face_alone(f: FaceFrame) {
        let dirt = require("dwell:dirt");
        let (lo, hi, top) = (4, 12, 16);
        let n = CHUNK_SIZE;
        let c = ChunkCoord::new(0, 2, 0);
        let o = [c.x * n, c.y * n, c.z * n];
        let build = |holes: &[[i32; 3]]| {
            let mut source = dwell_world::VoxelWorld::new(dwell_world::empty_generator(), None);
            for y in lo..=top {
                for z in lo..=hi {
                    for x in lo..=hi {
                        if holes.contains(&[x, y, z]) {
                            continue;
                        }
                        let m = if y == top { grass() } else { dirt };
                        source.set_voxel(o[0] + x, o[1] + y, o[2] + z, m);
                    }
                }
            }
            let m = mesh_on_face(&mut source, c, f);
            local_mesh(f, m.opaque)
        };
        let before = build(&[]);
        for row in [9, 10] {
            let mid = (lo + hi) / 2;
            let holes = [
                [hi, row, mid],
                [lo, row + 1, mid],
                [mid, row, hi],
                [mid, row + 1, lo],
                [mid, top, mid],
            ];
            let after = build(&holes);
            let mut sampled = 0;
            for (axis, sign) in FACES {
                let s = sign as f32;
                let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
                // The plane of the block's face facing (axis, sign), in cells.
                let plane = if sign > 0 {
                    f64::from(if axis == 1 { top } else { hi } + 1)
                } else {
                    f64::from(lo)
                };
                for i in 0..n {
                    for j in 0..n {
                        for (da, db) in [(0.2, 0.3), (0.7, 0.2), (0.3, 0.8), (0.8, 0.7)] {
                            let mut p = [0.0; 3];
                            p[axis] = plane;
                            p[a] = f64::from(i) + da;
                            p[b] = f64::from(j) + db;
                            let (Some(x), Some(y)) =
                                (uv_at(&before, axis, s, p), uv_at(&after, axis, s, p))
                            else {
                                continue;
                            };
                            sampled += 1;
                            assert!(
                                (x[0] - y[0]).abs() < 1e-4 && (x[1] - y[1]).abs() < 1e-4,
                                "{:?} row {row}: face ({axis}, {sign}) at {p:?}: uv {x:?} before, \
                                 {y:?} after",
                                f.face
                            );
                        }
                    }
                }
            }
            assert!(sampled > 1000, "{sampled} samples");
        }
    }
);

// face-a-only: a fluid surface of the face-local mesher, which meshes face B's chunks mirrored into the same frame.
#[test]
fn a_river_step_down_closes_the_strip_between_the_two_surfaces() {
    //! docs/BUGS.md 4 (gaps between water at height steps): an upper reach's column (water over
    //! water) beside a lower reach's last cell (air over it, its surface drawn `surface_drop` below
    //! its top) culled the face between them as fluid beside the same fluid, leaving an open strip
    //! from the lower surface up to the upper column's next cell. Since BUGS.md #92 the open cell
    //! over the lower water is a step cell: its slope runs from the upper surface at the shared
    //! edge down to the lower one, closing the strip, and no face stands inside it.
    let st = stone();
    let mut cells = Vec::new();
    for x in 4..8 {
        for z in 4..7 {
            cells.push((x, 0, z, st));
        }
    }
    cells.push((5, 1, 5, water()));
    cells.push((5, 2, 5, water()));
    cells.push((6, 1, 5, water()));
    let m = mesh(&cells, false).transparent;
    let top = 1.0 - dwell_blocks::require_fluid("dwell:water").surface_drop;
    let near = |p: &[f64; 3], x: f64, y: f64| (p[0] - x).abs() < 1e-6 && (p[1] - y).abs() < 1e-5;
    let mut slope = false;
    for t in m.indices.chunks_exact(3) {
        let n = normal_of(&m.positions, t[0], t[1], t[2]);
        let ps = [t[0], t[1], t[2]].map(|i| vertex(&m.positions, i));
        if n[1] > 0.0 && n[0] > 0.0 {
            slope |= ps.iter().any(|p| near(p, 6.0, 2.0 + top))
                && ps.iter().any(|p| near(p, 7.0, 1.0 + top));
        }
        let inside = ps
            .iter()
            .all(|p| (p[0] - 6.0).abs() < 1e-6 && p[1] > 1.0 + top + 1e-6);
        assert!(!inside, "a face inside the step's water at x 6: {ps:?}");
    }
    assert!(
        slope,
        "no slope closes the step from y {} at x 6 down to y {} at x 7",
        2.0 + top,
        1.0 + top
    );
}

#[test]
fn a_log_lying_along_x_shows_its_end_grain_on_its_ends_and_its_bark_along_it() {
    // docs/FELLING.md: a felled trunk settles lying along x or z (the log's `axis`). Its ends (±x)
    // take the end-grain tiles; its top and sides the bark, turned so the grain runs along x.
    let log =
        |axis: &str| dwell_blocks::parse_state(&format!("dwell:log[axis={axis}]")).expect("log");
    let rect = |name: &str| {
        let r = tile_rect(name);
        [r.u0 as f32, r.v0 as f32]
    };
    for (axis, end_normal) in [("y", 1usize), ("x", 0), ("z", 2)] {
        let m = mesh(&[(4, 4, 4, log(axis))], false).opaque;
        // Polygon by polygon (an octagonal log's caps have eight corners, ADR 0072): each
        // triangle's first vertex carries its face's normal and tile; a triangle of a rectangle
        // spans its face's v.
        for t in m.indices.chunks_exact(3) {
            let i = t[0] as usize;
            let n = [0, 1, 2].map(|k| m.normals[i * 3 + k]);
            let tile = [m.tiles[i * 4], m.tiles[i * 4 + 1]];
            let Some(along) = n.iter().position(|c| c.abs() == 1.0) else {
                // A chamfer (45 degrees): bark.
                assert_eq!(tile, rect("logSide"), "axis {axis}, chamfer {n:?}");
                continue;
            };
            if along == end_normal {
                // The cap: the heart's end grain inside the bark ring (Phase 55 T3).
                assert!(
                    tile == rect("logHeart") || tile == rect("logSide"),
                    "axis {axis}, cap tile {tile:?}"
                );
                continue;
            }
            assert_eq!(tile, rect("logSide"), "axis {axis}, face normal {n:?}");
            if along != end_normal {
                // The bark's v runs along the log: over the face, v spans the log's own length
                // (1 m along x or z, a half-metre cell along y), u the other way.
                let vs: Vec<f32> = t.iter().map(|&c| m.uvs[c as usize * 2 + 1]).collect();
                let span = vs.iter().fold(f32::MIN, |a, &b| a.max(b))
                    - vs.iter().fold(f32::MAX, |a, &b| a.min(b));
                let length = if end_normal == 1 { 0.5 } else { 1.0 };
                assert!(
                    (span - length).abs() < 1e-6,
                    "axis {axis}, normal {n:?}: v spans {span}"
                );
            }
        }
    }
}

#[test]
fn water_part_full_is_drawn_at_its_fill_and_its_step_to_full_water_closed() {
    // docs/WATER_FLOW.md §15 (owner, 2026-11-03: water fills smoothly): a water cell half full
    // (`fill=4` of 8) has its surface at half its drawn height and its sides up to it; full water
    // beside it in the same row closes the step between the two surfaces. Red before: drawn as
    // full water.
    let half = require("dwell:water[fill=4]");
    let st = stone();
    let mut cells = Vec::new();
    for x in 4..8 {
        for z in 4..7 {
            cells.push((x, 0, z, st));
        }
    }
    cells.push((5, 1, 5, half));
    cells.push((6, 1, 5, water()));
    let m = mesh(&cells, false).transparent;
    let full = 1.0 - dwell_blocks::require_fluid("dwell:water").surface_drop;
    let want = 1.0 + 0.5 * full;
    let mut top = false;
    let mut strip = false;
    for t in m.indices.chunks_exact(3) {
        let i = t[0] as usize * 3;
        let n = [m.normals[i], m.normals[i + 1], m.normals[i + 2]];
        let ps = [t[0], t[1], t[2]].map(|i| vertex(&m.positions, i));
        let over_half = ps.iter().all(|p| p[0] >= 5.0 - 1e-6 && p[0] <= 6.0 + 1e-6);
        if n[1] > 0.5 && over_half && ps.iter().all(|p| (p[1] - want).abs() < 1e-5) {
            top = true;
        }
        // The full cell's side facing the half one (−x, on x = 6) from the half surface up.
        if n[0] < -0.5 && ps.iter().all(|p| (p[0] - 6.0).abs() < 1e-6) {
            let lo = ps.iter().map(|p| p[1]).fold(f64::MAX, f64::min);
            let hi = ps.iter().map(|p| p[1]).fold(f64::MIN, f64::max);
            if (lo - want).abs() < 1e-5 && (hi - (1.0 + full)).abs() < 1e-5 {
                strip = true;
            }
        }
    }
    assert!(top, "no surface at y {want} over the half-full cell");
    assert!(
        strip,
        "no face closes the step from {want} to {}",
        1.0 + full
    );
}

/// The heights of the water's upward faces over the cell column x..x+1 (z 5..6).
fn water_tops_over(m: &MeshArrays, x: f64) -> Vec<f64> {
    let mut tops = Vec::new();
    for t in m.indices.chunks_exact(3) {
        let i = t[0] as usize * 3;
        if m.normals[i + 1] < 0.5 {
            continue;
        }
        let ps = [t[0], t[1], t[2]].map(|i| vertex(&m.positions, i));
        if ps
            .iter()
            .all(|p| p[0] >= x - 1e-6 && p[0] <= x + 1.0 + 1e-6)
        {
            tops.push(ps[0][1]);
        }
    }
    tops
}

#[test]
fn a_flooded_ramp_draws_its_water_at_the_fill_of_the_water_beside_it() {
    // docs/WATER_FLOW.md §15 (owner, 2026-11-04: slopes don't show the water rising): a flooded
    // ramp's water is drawn at the fill of the water beside it in its row, and none above its
    // floor while the water stands only below it. Red before: always at the full row.
    let quarter = require("dwell:water[fill=2]");
    let full = 1.0 - dwell_blocks::require_fluid("dwell:water").surface_drop;
    let bank = ramp("sand", "ramp", "east", "bottom", true);
    let m = mesh(&[(5, 3, 5, bank), (6, 3, 5, quarter)], false).transparent;
    let tops = water_tops_over(&m, 5.0);
    let want = 3.0 + 0.25 * full;
    assert!(!tops.is_empty(), "no water top over the ramp");
    for y in &tops {
        assert!(
            (y - want).abs() < 1e-5,
            "ramp water at {y}, the water beside at {want}: {tops:?}"
        );
    }
    // Water only in the row below (the ramp's row not begun): nothing over the ramp's floor.
    let m = mesh(
        &[(5, 3, 5, bank), (5, 2, 5, water()), (6, 2, 5, water())],
        false,
    )
    .transparent;
    let tops = water_tops_over(&m, 5.0);
    assert!(tops.iter().all(|&y| y <= 3.0 + 1e-6), "{tops:?}");
}

#[test]
fn a_fall_reaches_down_to_the_water_it_lands_in() {
    // Owner, 2026-11-04: "there's often a half block gap between the bottom of waterfalls and the
    // river below". A fall is written down to the cell over the pool it lands in, and the pool's
    // top is drawn below its cell's top (part full here): the fall's sides go on down to that
    // surface. Red before: they stopped at the cell's floor, air showing under them.
    let running = require("dwell:running_water");
    let quarter = require("dwell:water[fill=2]");
    let mut cells = Vec::new();
    for x in 3..=7 {
        for z in 3..=7 {
            cells.push((x, 1, z, stone()));
            cells.push((x, 2, z, quarter));
        }
    }
    cells.push((5, 3, 5, running));
    cells.push((5, 4, 5, running));
    let m = mesh(&cells, false).transparent;
    let full = 1.0 - dwell_blocks::require_fluid("dwell:water").surface_drop;
    let surface = 2.0 + 0.25 * full;
    let reaches = m.indices.chunks_exact(3).any(|t| {
        let i = t[0] as usize * 3;
        let side = m.normals[i + 1].abs() < 0.5;
        let ps = [t[0], t[1], t[2]].map(|i| vertex(&m.positions, i));
        let near = ps
            .iter()
            .all(|p| p[0] > 4.9 && p[0] < 6.1 && p[2] > 4.9 && p[2] < 6.1);
        side && near && ps.iter().any(|p| (p[1] - surface).abs() < 1e-4)
    });
    assert!(reaches, "no fall face down to the surface at {surface}");
}

#[test]
fn water_pouring_over_a_half_block_step_is_drawn_as_a_slope() {
    // Owner, 2026-11-04: "Would it make sense to add river slopes for 'flowing' water edges to
    // smooth out rivers big and small? Water edges (waterfalls) look really bad right now." A
    // pool's edge over an open cell above water half a block lower is drawn as a slope across
    // that cell down to the lower surface, not a vertical sheet. Red before: a vertical face.
    let mut cells = Vec::new();
    for z in 3..=5 {
        for x in 0..=9 {
            cells.push((x, 0, z, stone()));
            cells.push((x, 1, z, water()));
        }
        for x in 0..=4 {
            cells.push((x, 2, z, water()));
        }
    }
    let m = mesh(&cells, false).transparent;
    let full = 1.0 - dwell_blocks::require_fluid("dwell:water").surface_drop;
    let (high, low) = (2.0 + full, 1.0 + full);
    let mut slope = false;
    let mut sheet = false;
    for t in m.indices.chunks_exact(3) {
        let i = t[0] as usize * 3;
        let n = [m.normals[i], m.normals[i + 1], m.normals[i + 2]];
        let ps = [t[0], t[1], t[2]].map(|i| vertex(&m.positions, i));
        let over = ps.iter().all(|p| p[0] > 4.99 && p[0] < 6.01);
        if over && n[1] > 0.2 && n[1] < 0.99 && n[0] > 0.0 {
            let ys: Vec<f64> = ps.iter().map(|p| p[1]).collect();
            if ys.iter().any(|y| (y - high).abs() < 1e-4)
                && ys.iter().any(|y| (y - low).abs() < 1e-4)
            {
                slope = true;
            }
        }
        if n[0] > 0.9 && ps.iter().all(|p| (p[0] - 5.0).abs() < 1e-6 && p[1] > 1.99) {
            sheet = true;
        }
    }
    assert!(slope, "no slope from {high} down to {low} over the step");
    assert!(!sheet, "a vertical sheet at the step");
}

/// The first triangle of `m` a ray from `eye` along `dir` meets (Möller–Trumbore), as its
/// distance and the triangle's winding normal (the face its front faces), in cells.
fn first_hit(m: &MeshArrays, eye: [f64; 3], dir: [f64; 3]) -> Option<(f64, [f64; 3])> {
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let mut best: Option<(f64, [f64; 3])> = None;
    for t in m.indices.chunks_exact(3) {
        let [a, b, c] = [t[0], t[1], t[2]].map(|i| vertex(&m.positions, i));
        let (e1, e2) = (sub(b, a), sub(c, a));
        let p = cross(dir, e2);
        let det = dot(e1, p);
        if det.abs() < 1e-12 {
            continue;
        }
        let s = sub(eye, a);
        let u = dot(s, p) / det;
        let q = cross(s, e1);
        let v = dot(dir, q) / det;
        let d = dot(e2, q) / det;
        if u < 0.0 || v < 0.0 || u + v > 1.0 || d <= 1e-6 {
            continue;
        }
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, cross(e1, e2)));
        }
    }
    best
}

dwell_world::both_faces!(
    /// Owner, 2026-11-06: "diamond-ish shaped visual cuts appear in water slopes in rivers." A
    /// river crossing the grid on the diagonal steps down in a staircase, and each pool's edge is
    /// drawn as a slope down across the open cell beyond it (`slope_over`). Under each slope is a
    /// wedge of water whose side was left open where the cell beside it holds no such slope, so
    /// from the lower pool's level the eye looked in through the open side at the slope's back.
    /// The water shader discards a surface's back seen from outside the water (BUGS.md #31), so a
    /// triangle of the bank behind showed through. Every ray from the air over the lower pool
    /// toward the step must first meet a water face from its front, never a surface's back.
    fn a_diagonal_river_step_shows_no_slopes_back_from_the_lower_pool(f: FaceFrame) {
        let n = CHUNK_SIZE;
        let c = ChunkCoord::new(0, 2, 0);
        let o = [c.x * n, c.y * n, c.z * n];
        let mut source = dwell_world::VoxelWorld::new(dwell_world::empty_generator(), None);
        // Stone at row 4, the lower pool at row 5, the upper pool at row 6 where x + z ≤ 18.
        for z in 2..=26 {
            for x in 2..=26 {
                source.set_voxel(o[0] + x, o[1] + 4, o[2] + z, stone());
                source.set_voxel(o[0] + x, o[1] + 5, o[2] + z, water());
                if x + z <= 18 {
                    source.set_voxel(o[0] + x, o[1] + 6, o[2] + z, water());
                }
            }
        }
        let m = local_mesh(f, mesh_on_face(&mut source, c, f).transparent);
        let full = 1.0 - dwell_blocks::require_fluid("dwell:water").surface_drop;
        let (low, high) = (5.0 + full, 6.0 + full);
        let eye_y = low + 0.08;
        let mut rays = 0;
        let mut holes = Vec::new();
        for eye in [
            [16.5, eye_y, 12.5],
            [12.5, eye_y, 16.5],
            [15.2, eye_y, 15.7],
            [20.5, eye_y, 9.5],
        ] {
            // Toward the open cells of the step (x + z = 19), across them and up their slopes.
            for x in 4..=16 {
                let z = 19 - x;
                for (du, dv) in [
                    (0.1, 0.5),
                    (0.5, 0.1),
                    (0.5, 0.5),
                    (0.9, 0.3),
                    (0.3, 0.9),
                    (0.7, 0.7),
                ] {
                    for k in 1..=6 {
                        let h = low + (high - low) * f64::from(k) / 7.0;
                        let target = [f64::from(x) + du, h, f64::from(z) + dv];
                        let d = [target[0] - eye[0], target[1] - eye[1], target[2] - eye[2]];
                        let Some((_, nrm)) = first_hit(&m, eye, d) else {
                            continue;
                        };
                        rays += 1;
                        // In metres, as the shader reads the normal.
                        let nm = [nrm[0], nrm[1] / dwell_world::CELL_H, nrm[2]];
                        let len = (nm[0] * nm[0] + nm[1] * nm[1] + nm[2] * nm[2]).sqrt();
                        let back = nrm[0] * d[0] + nrm[1] * d[1] + nrm[2] * d[2] > 0.0;
                        if back && (nm[1] / len).abs() > 0.5 {
                            holes.push((eye, target));
                        }
                    }
                }
            }
        }
        assert!(rays > 1000, "{rays} rays met the water");
        assert!(
            holes.is_empty(),
            "{:?}: {} of {rays} rays from the lower pool met a surface's back first, e.g. {:?}",
            f.face,
            holes.len(),
            &holes[..holes.len().min(4)]
        );
    }
);

dwell_world::both_faces!(
    /// Owner, 2026-11-07: "underwater leaves z fighting, I think they should be 'flooded' so
    /// shouldn't get foam and stuff". A flooded leaf inside a pond draws its water as water: the
    /// pond's mesh is the one it has with water in the leaf's cell (no surface or side faces round
    /// the leaf, none on the leaf's faces), and the leaf is drawn inside it.
    fn a_flooded_leaf_in_a_pond_draws_no_water_faces_round_it(f: FaceFrame) {
        let c = ChunkCoord::new(0, 2, 0);
        let n = CHUNK_SIZE;
        let o = [c.x * n, c.y * n, c.z * n];
        let pond = |middle: u16| {
            let mut source = dwell_world::VoxelWorld::new(dwell_world::empty_generator(), None);
            for y in 3..=8 {
                for z in 3..=9 {
                    for x in 3..=9 {
                        let wall = y == 3 || x == 3 || x == 9 || z == 3 || z == 9;
                        let m = if wall {
                            stone()
                        } else if y == 8 {
                            0
                        } else if [x, y, z] == [6, 5, 6] {
                            middle
                        } else {
                            water()
                        };
                        source.set_voxel(o[0] + x, o[1] + y, o[2] + z, m);
                    }
                }
            }
            mesh_on_face(&mut source, c, f)
        };
        let all_water = pond(water());
        let flooded = pond(require("dwell:leaves[flooded=true]"));
        let dry = pond(require("dwell:leaves[flooded=false]"));
        let water_area = |m: &ChunkMeshes| quad_area(&m.transparent);
        assert!(
            (water_area(&flooded) - water_area(&all_water)).abs() < 1e-3,
            "the flooded leaf's water is the pond's: {} vs {}",
            water_area(&flooded),
            water_area(&all_water)
        );
        assert!(
            water_area(&dry) > water_area(&all_water) + 1.0,
            "a dry leaf has the water's faces round it"
        );
        // A lone leaf (shared/look/leaves.json): 6 faces and 12 edge cuts (quads) and 8 corner
        // cuts (triangles, half a quad each); its cards (`cards`, a quad each) besides.
        let cards = flooded
            .cutout
            .plants
            .chunks(16)
            .filter(|p| p[3] > 0.0)
            .count();
        assert_eq!(
            quads(&flooded.cutout) - cards,
            22,
            "the leaf drawn inside the water"
        );
    }
);

/// The opaque mesh of `cells` with `cuts` carved (chunk-local, world-oriented), in cells.
fn mesh_cut(cells: &[(i32, i32, i32, u16)], cuts: &[([i32; 3], Cut)], mirror: bool) -> MeshArrays {
    in_cells(super::mesh_chunk_cut(&voxels(cells), &[], mirror, [0; 3], cuts).expect("padded"))
        .opaque
}

/// The triangles of `m` as (corners, unit normal, whether its tile is `tile`).
fn triangles(m: &MeshArrays, tile: &str) -> Vec<([[f64; 3]; 3], [f64; 3], bool)> {
    let r = tile_rect(tile);
    m.indices
        .chunks_exact(3)
        .map(|t| {
            let p = [0, 1, 2].map(|k| vertex(&m.positions, t[k]));
            let n = normal_of(&m.positions, t[0], t[1], t[2]);
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
            let i = t[0] as usize * 4;
            let is = (f64::from(m.tiles[i]) - r.u0).abs() < 1e-6
                && (f64::from(m.tiles[i + 1]) - r.v0).abs() < 1e-6;
            (p, n.map(|c| c / l), is)
        })
        .collect()
}

dwell_world::both_faces!(
    /// A cut log is meshed carved (ADR 0067 §11): a standing trunk notched 0.15 m into its −x
    /// side with a kerf past it shows the V's fresh-wood faces sloping in to the apex 0.15 m deep
    /// at the row's middle, the slit deeper, and no bark left over the notch's mouth; the cells
    /// above and below stay cubes. A trunk lying along x notched down from its face-local top is
    /// cut the same, from whichever world side is that face's up.
    fn a_cut_log_is_carved_into_the_chunk_mesh(f: FaceFrame) {
        let log = require("dwell:log");
        let (x, z) = (5, 5);
        let cells: Vec<_> = (10..13).map(|y| (x, y, z, log)).collect();
        let mut cut = Cut::default();
        cut.notch[0] = 0.15;
        cut.kerf[0][1] = 0.05;
        let m = mesh_cut(&cells, &[([x, 11, z], cut)], f.is_b());
        let tris = triangles(&m, "logCut");
        let fresh: Vec<_> = tris.iter().filter(|t| t.2).collect();
        assert!(!fresh.is_empty(), "fresh-wood faces");
        // Every fresh face lies in the cut: within 0.2 m of the −x side, inside the cut row.
        for (p, n, _) in &fresh {
            for q in p {
                assert!(q[0] >= 5.0 - 1e-6 && q[0] <= 5.2 + 1e-6, "{q:?}");
                assert!(q[1] >= 11.0 - 1e-6 && q[1] <= 12.0 + 1e-6, "{q:?}");
            }
            assert!(n[0] <= 1e-6, "faces out of the −x side: {n:?}");
        }
        // The V's apex line, 0.15 m in at the row's middle (a ½ m row: 1 cell).
        let has = |px: f64, py: &dyn Fn(f64) -> bool| {
            m.positions
                .chunks_exact(3)
                .any(|p| (f64::from(p[0]) - px).abs() < 1e-5 && py(f64::from(p[1])))
        };
        assert!(has(5.15, &|y| (y - 11.5).abs() < 1e-5), "the apex");
        // The kerf's slit, 0.2 m in, in the face-locally upper band.
        let upper = |y: f64| if f.is_b() { y < 11.5 } else { y > 11.5 };
        assert!(has(5.2, &upper), "the kerf's slit in the upper band");
        // No bark (a −x face on the side's plane) over the mouth: 2.5 × 0.15 = 0.375 m open.
        let hw = 0.5 * 2.5 * 0.15 / 0.5;
        for (p, n, is) in &tris {
            let c = [0, 1, 2].map(|k| (p[0][k] + p[1][k] + p[2][k]) / 3.0);
            let on_side = n[0] < -0.999 && (c[0] - 5.0).abs() < 1e-6;
            assert!(
                *is || !on_side || (c[1] - 11.5).abs() >= hw - 1e-3,
                "a cube face left in the notch: {p:?}"
            );
        }
        // Closed: the carved cell's faces' area vectors sum to zero but for its two end squares
        // (hidden by the logs above and below), which cancel each other.
        let mut sum = [0.0; 3];
        for (p, _, _) in &tris {
            let inside = p.iter().all(|q| {
                q[0] > 5.0 - 1e-6 && q[0] < 6.0 + 1e-6 && q[1] > 11.0 - 1e-6 && q[1] < 12.0 + 1e-6
            });
            let flat = |y: f64| p.iter().all(|q| (q[1] - y).abs() < 1e-6);
            if inside && !flat(11.0) && !flat(12.0) {
                let e1 = [0, 1, 2].map(|k| p[1][k] - p[0][k]);
                let e2 = [0, 1, 2].map(|k| p[2][k] - p[0][k]);
                let c = [
                    e1[1] * e2[2] - e1[2] * e2[1],
                    e1[2] * e2[0] - e1[0] * e2[2],
                    e1[0] * e2[1] - e1[1] * e2[0],
                ];
                sum = [0, 1, 2].map(|k| sum[k] + c[k]);
            }
        }
        assert!(sum.iter().all(|c| c.abs() < 1e-4), "open: {sum:?}");
        // Uncut: the column meshes as without cuts, with no fresh wood.
        let plain = mesh_cut(&cells, &[], f.is_b());
        assert!(triangles(&plain, "logCut").iter().all(|t| !t.2));
        assert_eq!(plain, mesh_chunk(&voxels(&cells), f.is_b()).unwrap().opaque);

        // A log lying along x, notched 0.1 m from its face-local top (world: the face's up).
        let lying = dwell_blocks::state("dwell:log[axis=x]").expect("a lying log");
        let up = f.vector([0.0, 1.0, 0.0]);
        let side = (0..4)
            .find(|&s| f64::from(dwell_protocol::chopping::side_normal(0, s)[1]) == up[1])
            .expect("the up side");
        let mut cut = Cut::default();
        cut.notch[side] = 0.1;
        let m = mesh_cut(&[(8, 10, 8, lying)], &[([8, 10, 8], cut)], f.is_b());
        // 0.1 m of a ½ m row: 0.2 of a cell down from the face-local top, at x's middle.
        let apex_y = if f.is_b() { 10.2 } else { 10.8 };
        let apex = m.positions.chunks_exact(3).any(|p| {
            (f64::from(p[0]) - 8.5).abs() < 1e-5 && (f64::from(p[1]) - apex_y).abs() < 1e-5
        });
        assert!(apex, "the lying log's apex at y {apex_y}");
        assert!(triangles(&m, "logCut").iter().any(|t| t.2));
    }
);

dwell_world::both_faces!(
    /// The axe's bites (WORK_QUEUE item 26): one hit on a standing trunk's −x side, struck 0.45 m
    /// along z (on the octagon's flat face), is carved as a V centred there, not a slit across
    /// the side (red before item 26: the kerf's fresh wood ran from z 0 to 1), grown by the bite
    /// rule (WORK_QUEUE item 18: its first of the swings to fell's share of half the log, so wider
    /// and deeper than the head), its mouth in the notch's band round the row's middle and its two
    /// faces both turned out of the notch: the lower one facing face-local up, the upper one
    /// down (the gash's ramps' short sides at the cut); and the cell carries the wood's cracks
    /// while its neighbours don't.
    fn a_bitten_log_is_carved_where_it_was_struck_and_cracks(f: FaceFrame) {
        let log = require("dwell:log");
        let wood = dwell_protocol::chopping::wood("dwell:log").unwrap();
        let axe = dwell_protocol::chopping::tool("dwell:stone_axe").unwrap();
        let (x, z) = (5, 5);
        let cells: Vec<_> = (10..13).map(|y| (x, y, z, log)).collect();
        let mut cut = Cut::default();
        cut.swing(axe, 1, 0, 0, 1.0);
        cut.bite(1, 0, 0, 0.45, &wood.cut.bite);
        let m = mesh_cut(&cells, &[([x, 11, z], cut)], f.is_b());
        let fresh: Vec<_> = triangles(&m, "logCut")
            .into_iter()
            .filter(|t| t.2)
            .collect();
        assert!(!fresh.is_empty(), "fresh-wood faces");
        let deep = fresh
            .iter()
            .flat_map(|t| t.0.iter())
            .fold(0.0f64, |a, q| a.max(q[0] - 5.0));
        let b = wood.cut.bite;
        assert!(
            deep > 0.2 && deep < 0.6,
            "the first swing's share of the rule: {deep}"
        );
        let half = 0.5 * (deep * b.width_per_depth).max(b.width_m) + 1e-4;
        let (mut zlo, mut zhi) = (f64::INFINITY, f64::NEG_INFINITY);
        for (p, n, _) in &fresh {
            for q in p {
                zlo = zlo.min(q[2]);
                zhi = zhi.max(q[2]);
                assert!(
                    (q[2] - 5.45).abs() <= half,
                    "inside the grown bite round the hit: {q:?}"
                );
                assert!(
                    (q[1] - 11.5).abs() <= 0.5 * b.band_m / dwell_world::CELL_H + 1e-4,
                    "in the notch's band: {q:?}"
                );
                assert!(q[0] >= 5.0 - 1e-6 && q[0] <= 5.0 + deep + 1e-5, "{q:?}");
            }
            // Face-local: the row's lower half's face looks up and out of the notch (−x), the
            // upper half's down and out.
            let mid = (p[0][1] + p[1][1] + p[2][1]) / 3.0;
            let below = if f.is_b() { mid > 11.5 } else { mid < 11.5 };
            let up = if f.is_b() { -n[1] } else { n[1] };
            if (mid - 11.5).abs() > 1e-3 && n[0] < -0.05 {
                assert!(
                    if below { up > 0.05 } else { up < -0.05 },
                    "the V's faces turn out of the notch: {p:?} {n:?}"
                );
            }
        }
        assert!(
            ((zlo + zhi) * 0.5 - 5.45).abs() < 0.03,
            "centred where it struck: {zlo}..{zhi}"
        );
        // Its cracks: the cut cell's vertices carry the wood's first crack tile, the logs above
        // and below none.
        assert_eq!(
            m.cracks.len(),
            m.positions.len() / 3,
            "the stream covers every vertex"
        );
        let first = wood.cut.cracks.unwrap().tiles[0];
        let slot = crate::textures::TILE_ORDER
            .iter()
            .position(|t| *t == first)
            .unwrap()
            + 1;
        for (v, p) in m.positions.chunks_exact(3).enumerate() {
            let y = f64::from(p[1]);
            let slot_here = (m.cracks[v] >> 16) & 0xff;
            if y > 11.0 + 1e-4 && y < 12.0 - 1e-4 {
                assert_eq!(slot_here as usize, slot, "the cut cell cracks: {p:?}");
            } else if !(10.999..=12.001).contains(&y) {
                assert_eq!(slot_here, 0, "a neighbour doesn't: {p:?}");
            }
        }
    }
);

/// The opaque mesh of logs (`(x, y, z, axis)`, chunk-local) in chunk (0, 2, 0) of face `f`,
/// read back face-local: each triangle's (centroid, normal).
fn log_triangles(f: FaceFrame, logs: &[(i32, i32, i32, &str)]) -> Vec<([f64; 3], [f64; 3])> {
    let states: Vec<(i32, i32, i32, StateId)> = logs
        .iter()
        .map(|&(x, y, z, axis)| (x, y, z, require(&format!("dwell:log[axis={axis}]"))))
        .collect();
    state_triangles(f, &states)
}

/// As `log_triangles`, for face-local states.
fn state_triangles(f: FaceFrame, cells: &[(i32, i32, i32, StateId)]) -> Vec<([f64; 3], [f64; 3])> {
    let c = ChunkCoord::new(0, 2, 0);
    let n = CHUNK_SIZE;
    let mut source = dwell_world::VoxelWorld::new(dwell_world::empty_generator(), None);
    for &(x, y, z, m) in cells {
        source.set_voxel(x, c.y * n + y, z, m);
    }
    let m = local_mesh(f, mesh_on_face(&mut source, c, f).opaque);
    m.indices
        .chunks_exact(3)
        .map(|t| {
            let p: Vec<[f64; 3]> = t.iter().map(|&i| vertex(&m.positions, i)).collect();
            let mid = std::array::from_fn(|a| (p[0][a] + p[1][a] + p[2][a]) / 3.0);
            (mid, vertex(&m.normals, t[0]))
        })
        .collect()
}

fn diagonal(n: [f64; 3]) -> bool {
    n.iter().filter(|c| c.abs() > 0.1).count() == 2
}

dwell_world::both_faces!(
    /// A lone log is an octagonal prism (ADR 0072): eight bark sides round its axis, four of them
    /// the 45° chamfers, and end-grain caps; nothing outside the octagon's corners is drawn.
    fn a_lone_log_meshes_as_an_octagonal_prism(f: FaceFrame) {
        let tris = log_triangles(f, &[(8, 8, 8, "y")]);
        let normals: std::collections::BTreeSet<[i64; 3]> = tris
            .iter()
            .map(|(_, n)| n.map(|c| (c * 100.0).round() as i64))
            .collect();
        assert_eq!(normals.len(), 10, "8 sides and 2 caps: {normals:?}");
        assert_eq!(
            tris.iter().filter(|(_, n)| diagonal(*n)).count(),
            8,
            "4 chamfer quads"
        );
        // The chamfers cut the corners: nothing is drawn at a corner column of the cell.
        for (mid, _) in &tris {
            let (u, v) = (mid[0] - 8.0, mid[2] - 8.0);
            assert!(!(u < 0.05 && v < 0.05), "a corner face at {mid:?}");
        }
    }
);

dwell_world::both_faces!(
    /// Two logs side by side along one axis join flat (owner, 2026-11-09: "full width sides where
    /// they meet"): no face between them, no chamfer on the corners at the join, and their sides
    /// run on flat across it. Logs of different axes keep their chamfers.
    fn two_same_axis_logs_join_full_width_and_different_axes_do_not(f: FaceFrame) {
        let joined = log_triangles(f, &[(8, 8, 8, "y"), (9, 8, 8, "y")]);
        assert!(
            joined.iter().all(|(m, _)| (m[0] - 9.0).abs() > 1e-6),
            "a face on the shared plane x = 9"
        );
        assert_eq!(
            joined.iter().filter(|(_, n)| diagonal(*n)).count(),
            8,
            "2 chamfers each"
        );
        let crossed = log_triangles(f, &[(8, 8, 8, "y"), (9, 8, 8, "z")]);
        assert_eq!(
            crossed.iter().filter(|(_, n)| diagonal(*n)).count(),
            16,
            "4 chamfers each"
        );
    }
);

/// The face-local top caps' triangles of logs (as `log_triangles`) with each one's tile.
fn log_top_tiles(f: FaceFrame, logs: &[(i32, i32, i32, &str)]) -> Vec<([f64; 3], [f32; 2])> {
    let c = ChunkCoord::new(0, 2, 0);
    let n = CHUNK_SIZE;
    let mut source = dwell_world::VoxelWorld::new(dwell_world::empty_generator(), None);
    for &(x, y, z, axis) in logs {
        let m = require(&format!("dwell:log[axis={axis}]"));
        source.set_voxel(x, c.y * n + y, z, m);
    }
    let m = local_mesh(f, mesh_on_face(&mut source, c, f).opaque);
    m.indices
        .chunks_exact(3)
        .filter(|t| vertex(&m.normals, t[0])[1] > 0.5)
        .map(|t| {
            let p: Vec<[f64; 3]> = t.iter().map(|&i| vertex(&m.positions, i)).collect();
            let mid = std::array::from_fn(|a| (p[0][a] + p[1][a] + p[2][a]) / 3.0);
            let i = t[0] as usize;
            (mid, [m.tiles[i * 4], m.tiles[i * 4 + 1]])
        })
        .collect()
}

dwell_world::both_faces!(
    /// Phase 55 T3 (owner, 2026-11-09: "the outside bark ring should follow the octagon, it
    /// doesn't, it gets cut off on the corners"): a lone log's top is end grain (the ringless
    /// heart tile) inside a band of bark that runs along all eight edges, the chamfers too. Red
    /// before: the whole cap was the square-ringed end tile, its ring clipped at the corners.
    fn a_lone_logs_cap_rings_the_octagon_in_bark(f: FaceFrame) {
        let tris = log_top_tiles(f, &[(8, 8, 8, "y")]);
        assert!(
            tris.iter().any(|(_, t)| *t == rect_of("logHeart")),
            "no heart"
        );
        let bark: Vec<_> = tris
            .iter()
            .filter(|(_, t)| *t == rect_of("logSide"))
            .collect();
        // Bark at a chamfer: near the cut corner (u + v about the chamfer), on all four.
        for (cu, cv) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
            let near = bark.iter().any(|(m, _)| {
                let (u, v) = ((m[0] - 8.0 - cu).abs(), (m[2] - 8.0 - cv).abs());
                (0.2..0.45).contains(&(u + v)) && u < 0.4 && v < 0.4
            });
            assert!(near, "no bark along the chamfer at corner ({cu}, {cv})");
        }
    }
);

dwell_world::both_faces!(
    /// Joined logs' tops have no bark between them: no ring runs along the joined side, so a
    /// joined wall's top reads as continuous wood; the open sides keep their ring.
    fn joined_logs_tops_have_no_ring_between_them(f: FaceFrame) {
        let tris = log_top_tiles(f, &[(8, 8, 8, "y"), (9, 8, 8, "y")]);
        let bark: Vec<_> = tris
            .iter()
            .filter(|(_, t)| *t == rect_of("logSide"))
            .collect();
        assert!(!bark.is_empty(), "the open sides keep their ring");
        assert!(
            bark.iter().all(|(m, _)| (m[0] - 9.0).abs() > 0.15),
            "bark along the joined side x = 9: {:?}",
            bark.iter().map(|b| b.0).collect::<Vec<_>>()
        );
    }
);

dwell_world::both_faces!(
    /// Phase 55 T4 (owner, 2026-11-09: "everything log/tree should be octagonal"): a stump's wood
    /// ramp is the log's octagon prism cut by its slant: its slant and sides stay inside the
    /// octagon (nothing at the cell's cut corners), and it meets the log below flat (neither cap
    /// between them drawn). Red before: the square ramp drew its corners.
    fn a_stumps_wood_ramp_is_the_octagon_cut_by_its_slant(f: FaceFrame) {
        let log = require("dwell:log[axis=y]");
        let ramp = require("dwell:log_ramp[facing=west,flooded=false,half=bottom,shape=ramp]");
        let tris = state_triangles(f, &[(8, 8, 8, log), (8, 9, 8, ramp)]);
        let slant = |n: [f64; 3]| n[1].abs() > 0.1 && n[1].abs() < 0.99;
        assert!(tris.iter().any(|(_, n)| slant(*n)), "a slant");
        // The log's four chamfers are 8 triangles; the ramp's run on up its sides.
        let chamfers = tris
            .iter()
            .filter(|(_, n)| n[1].abs() < 0.01 && diagonal(*n))
            .count();
        assert!(chamfers > 8, "the ramp's chamfers too: {chamfers}");
        for (mid, n) in &tris {
            let (u, v) = (mid[0] - 8.0, mid[2] - 8.0);
            let corner = |a: f64| !(0.05..=0.95).contains(&a);
            assert!(
                !(corner(u) && corner(v)),
                "a corner face at {mid:?} ({n:?})"
            );
        }
        assert!(
            !tris.iter().any(|(_, n)| n[1] > 0.99),
            "no flat top: the log's cap is covered and the ramp's is its slant"
        );
        assert!(tris.iter().any(|(_, n)| n[1] < -0.99), "the log's bottom");
    }
);

dwell_world::both_faces!(
    /// Owner, 2026-11-10: "Top of water slopes from up river looking down, have some 'gap'
    /// artifacts": thin see-through lines along a river's ½ m water steps seen from upstream. A
    /// water step is watertight from above: every ray from the air over the upper pool down into
    /// the water beyond the step (the upper top, the pour slope, the lower top) meets a water face
    /// from its front first, never nothing or a face's back.
    fn a_water_step_is_watertight_seen_from_upstream(f: FaceFrame) {
        let n = CHUNK_SIZE;
        let c = ChunkCoord::new(0, 2, 0);
        let o = [c.x * n, c.y * n, c.z * n];
        let mut source = dwell_world::VoxelWorld::new(dwell_world::empty_generator(), None);
        // Stone at row 4, the lower pool at row 5, the upper pool at row 6 where x ≤ 12.
        for z in 2..=26 {
            for x in 2..=26 {
                source.set_voxel(o[0] + x, o[1] + 4, o[2] + z, stone());
                source.set_voxel(o[0] + x, o[1] + 5, o[2] + z, water());
                if x <= 12 {
                    source.set_voxel(o[0] + x, o[1] + 6, o[2] + z, water());
                }
            }
        }
        let m = local_mesh(f, mesh_on_face(&mut source, c, f).transparent);
        let mut rays = 0;
        let mut holes = Vec::new();
        for eye in [
            [4.5, 9.0, 14.5],
            [6.2, 7.6, 11.3],
            [8.5, 12.0, 17.5],
            [3.0, 7.2, 14.0],
        ] {
            for i in 0..=80 {
                let x = 11.0 + f64::from(i) * 0.05;
                for z in [6.37, 10.5, 13.01, 14.99, 15.0, 15.5, 18.73, 22.2] {
                    // a point under both surfaces (in the stone's row), so the ray crosses one
                    let target = [x, 4.6, z];
                    let d = [target[0] - eye[0], target[1] - eye[1], target[2] - eye[2]];
                    rays += 1;
                    match first_hit(&m, eye, d) {
                        None => holes.push((eye, target, "nothing")),
                        Some((_, nrm)) => {
                            if nrm[0] * d[0] + nrm[1] * d[1] + nrm[2] * d[2] > 0.0 {
                                holes.push((eye, target, "a back"));
                            }
                        }
                    }
                }
            }
        }
        assert!(
            holes.is_empty(),
            "{:?}: {} of {rays} rays from upstream met no water front first, e.g. {:?}",
            f.face,
            holes.len(),
            &holes[..holes.len().min(6)]
        );
    }
);

/// The T-junctions of `m` across a crease: a vertex of one triangle lying inside an edge of
/// another whose plane differs (where two surfaces meet at an angle, the rasterizer leaves a crack
/// along such an edge: the sky or the bed shows through in a dashed hairline).
fn crease_t_junctions(m: &MeshArrays) -> Vec<[f64; 3]> {
    let tris: Vec<([[f64; 3]; 3], [f32; 3])> = m
        .indices
        .chunks_exact(3)
        .map(|t| {
            let ps = [t[0], t[1], t[2]].map(|i| vertex(&m.positions, i));
            let i = t[0] as usize * 3;
            (ps, [m.normals[i], m.normals[i + 1], m.normals[i + 2]])
        })
        .collect();
    let mut out = Vec::new();
    for (ps, n) in &tris {
        for p in ps {
            for (qs, nq) in &tris {
                if n[0] * nq[0] + n[1] * nq[1] + n[2] * nq[2] > 0.999 {
                    continue;
                }
                for e in 0..3 {
                    let (a, b) = (qs[e], qs[(e + 1) % 3]);
                    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                    let ap = [p[0] - a[0], p[1] - a[1], p[2] - a[2]];
                    let len2 = ab[0] * ab[0] + ab[1] * ab[1] + ab[2] * ab[2];
                    let t = (ap[0] * ab[0] + ap[1] * ab[1] + ap[2] * ab[2]) / len2;
                    if t <= 1e-6 || t >= 1.0 - 1e-6 {
                        continue;
                    }
                    let off = [ap[0] - t * ab[0], ap[1] - t * ab[1], ap[2] - t * ab[2]];
                    if off[0] * off[0] + off[1] * off[1] + off[2] * off[2] < 1e-12 {
                        out.push(*p);
                    }
                }
            }
        }
    }
    out
}

dwell_world::both_faces!(
    /// Owner, 2026-11-10 (the dashed hairlines along a river's steps): where a pool's top meets
    /// its pour slope, and the slope the lower top, no vertex of one lies inside an edge of the
    /// other. The tops were merged into long rectangles while each slope is one cell wide, so
    /// along the step the slopes' corners fell inside the tops' edges (T-junctions), and the
    /// rasterizer left a crack at each that showed the bed through the water.
    fn a_water_step_has_no_t_junctions_along_its_creases(f: FaceFrame) {
        let n = CHUNK_SIZE;
        let c = ChunkCoord::new(0, 2, 0);
        let o = [c.x * n, c.y * n, c.z * n];
        let mut source = dwell_world::VoxelWorld::new(dwell_world::empty_generator(), None);
        // Stone at row 4, the lower pool at row 5, the upper pool at row 6 where x ≤ 8; a second
        // step in fill within row 5 (x ≥ 12 a lower fill), its riser vertical.
        let low_fill = require("dwell:water[fill=4]");
        for z in 2..=13 {
            for x in 2..=13 {
                source.set_voxel(o[0] + x, o[1] + 4, o[2] + z, stone());
                let lower = if x >= 12 { low_fill } else { water() };
                source.set_voxel(o[0] + x, o[1] + 5, o[2] + z, lower);
                if x <= 8 {
                    source.set_voxel(o[0] + x, o[1] + 6, o[2] + z, water());
                }
            }
        }
        let m = local_mesh(f, mesh_on_face(&mut source, c, f).transparent);
        // along the steps, away from the pool's open sides (a river's are banks)
        let ts: Vec<[f64; 3]> = crease_t_junctions(&m)
            .into_iter()
            .filter(|p| p[2] > 2.5 && p[2] < 13.5 && p[0] > 2.5 && p[0] < 13.5)
            .collect();
        assert!(
            ts.is_empty(),
            "{:?}: {} T-junctions along the step's creases, e.g. {:?}",
            f.face,
            ts.len(),
            &ts[..ts.len().min(6)]
        );
    }
);

dwell_world::both_faces!(
    /// Leaf cards (shared/look/leaves.json `cards`): a lone leaf's cards are in the cutout mesh
    /// with the data's distance fade (none past `fade_end_m`) and no ink; meshed without cards (a
    /// CPU adapter, the cheapest tier) the leaf draws none.
    fn a_lone_leaf_carries_cards_that_fade_by_distance_and_none_on_the_cpu_tier(f: FaceFrame) {
        let leaf = require("dwell:leaves");
        let padded = voxels(&[(8, 8, 8, leaf)]);
        let mesh = |cards: bool| {
            in_cells(
                super::mesh_chunk_flags(
                    &padded,
                    &[],
                    f.is_b(),
                    [0; 3],
                    &[],
                    None,
                    ChunkFlags {
                        water_tops: true,
                        cards,
                        ..ChunkFlags::default()
                    },
                )
                .expect("padded"),
            )
            .cutout
        };
        let (with, without) = (mesh(true), mesh(false));
        let look = dwell_look::leaves::LEAVES.cards;
        let carded: Vec<usize> = (0..with.positions.len() / 3)
            .filter(|&i| with.plants[i * 4 + 3] > 0.0)
            .collect();
        assert!(!carded.is_empty(), "a lone leaf has cards");
        assert_eq!(carded.len() % 4, 0);
        for &i in &carded {
            assert_eq!(with.plants[i * 4 + 2], look.fade_start_m as f32);
            assert_eq!(with.plants[i * 4 + 3], look.fade_end_m as f32);
            assert_eq!(with.bare[i], 1.0);
        }
        assert_eq!(
            with.positions.len() / 3,
            without.positions.len() / 3 + carded.len()
        );
        // Its faces are a tree's leaves (`TREE_LEAF_FADE`): no sway, handed over to the far
        // trees' impostors by the frame's distances (WORK_QUEUE item 31).
        assert!(!without.plants.is_empty());
        for v in without.plants.chunks(4) {
            assert_eq!(v, [0.0, 0.0, crate::TREE_LEAF_FADE, crate::TREE_LEAF_FADE]);
        }
        let faces = (0..with.positions.len() / 3).filter(|&i| with.plants[i * 4 + 3] < 0.0);
        assert_eq!(faces.count(), without.positions.len() / 3);
    }
);

/// Face B's mirror re-packs a leaf card's packed normal y-flipped (`flip_arrays_y`); turning it
/// back must give face A's bits, or a face-B card's normal drifts a grid step from its mirror's
/// (`a_chunk_meshes_the_same_face_local_on_either_face`). Red before: on the odd 2047-step grid
/// the octahedral fold falls between grid points.
#[test]
fn a_leaf_cards_packed_normal_flips_exactly() {
    // both-faces: the packing's own y flip, the mirror between the faces.
    let flip = |p: f32| {
        let n = leaves::card_normal_of(p);
        leaves::card_speed([n[0], -n[1], n[2]])
    };
    let mut checked = 0;
    for i in 0..64 {
        for j in 0..32 {
            let (a, b) = (
                f64::from(i) / 64.0 * std::f64::consts::TAU,
                f64::from(j) / 31.0 * std::f64::consts::PI,
            );
            let n = [b.sin() * a.cos(), b.cos(), b.sin() * a.sin()];
            let p = leaves::card_speed(n);
            assert!(
                p < 0.0 && (-p) < (1 << 24) as f32,
                "{p} marks a card, exactly"
            );
            assert_eq!(flip(flip(p)), p, "{n:?} packs {p}, and back through a flip");
            checked += 1;
        }
    }
    assert_eq!(checked, 64 * 32);
}

dwell_world::both_faces!(
    /// BUGS.md #78: each leaf card carries its plane's normal packed in `plants.y` (negative: a
    /// card, `leaves::card_speed`) for the vertex shader's far-side cull. On both faces it is
    /// perpendicular to the card and on its lit side (with the vertex normals), so a card is dropped
    /// only when seen from behind; face B's mirror re-packs it (red without: the y flips missed).
    fn a_leaf_cards_packed_normal_is_its_plane_on_both_faces(f: FaceFrame) {
        let leaf = require("dwell:leaves");
        let m = in_cells(
            super::mesh_chunk_flags(
                &voxels(&[(8, 8, 8, leaf)]),
                &[],
                f.is_b(),
                [0; 3],
                &[],
                None,
                ChunkFlags {
                    water_tops: true,
                    cards: true,
                    ..ChunkFlags::default()
                },
            )
            .expect("padded"),
        )
        .cutout;
        let carded: Vec<usize> = (0..m.positions.len() / 3)
            .filter(|&i| m.plants[i * 4 + 3] > 0.0)
            .collect();
        assert!(!carded.is_empty());
        let pos = |i: usize| -> [f64; 3] {
            // Cells to metres (y rows are CELL_H tall) for the angle checks.
            let c = |a: usize| f64::from(m.positions[i * 3 + a]);
            [c(0), c(1) * CELL_H, c(2)]
        };
        for card in carded.chunks_exact(4) {
            let speed = m.plants[card[0] * 4 + 1];
            assert!(speed < 0.0, "a card's plants.y marks it: {speed}");
            for &i in card {
                assert_eq!(
                    m.plants[i * 4 + 1],
                    speed,
                    "one normal for the card's corners"
                );
            }
            let n = leaves::card_normal_of(speed);
            let p0 = pos(card[0]);
            for &i in &card[1..] {
                let e: [f64; 3] = std::array::from_fn(|a| pos(i)[a] - p0[a]);
                let len = e.iter().map(|v| v * v).sum::<f64>().sqrt();
                let along = (0..3).map(|a| e[a] * n[a]).sum::<f64>() / len;
                assert!(
                    along.abs() < 0.01,
                    "the packed normal {n:?} is off the card: {along}"
                );
            }
            let vn: f64 = (0..3)
                .map(|a| f64::from(m.normals[card[0] * 3 + a]) * n[a])
                .sum();
            assert!(
                vn > 0.0,
                "the packed normal {n:?} faces the card's lit side: {vn}"
            );
        }
    }
);

dwell_world::both_faces!(
    /// BUGS.md #78 (the M1 at High, owner: "the internal leaf cards may be sucking up a lot of
    /// unneeded power"): a crown with a cavity inside (a 7-cell shell of leaves round a 3-cell
    /// hollow) draws no card into the hollow, where every one would be hidden, and keeps its
    /// outer faces' cards. Red before: the hollow's six inner walls carried cards.
    fn a_crowns_inner_cavity_draws_no_cards(f: FaceFrame) {
        let leaf = require("dwell:leaves");
        let shell = 5..=11;
        let hollow = 7..=9;
        let mut cells = Vec::new();
        for x in shell.clone() {
            for y in shell.clone() {
                for z in shell.clone() {
                    if !(hollow.contains(&x) && hollow.contains(&y) && hollow.contains(&z)) {
                        cells.push((x, y, z, leaf));
                    }
                }
            }
        }
        let m = in_cells(
            super::mesh_chunk_flags(
                &voxels(&cells),
                &[],
                f.is_b(),
                [0; 3],
                &[],
                None,
                ChunkFlags {
                    water_tops: true,
                    cards: true,
                    ..ChunkFlags::default()
                },
            )
            .expect("padded"),
        )
        .cutout;
        let carded: Vec<[f64; 3]> = (0..m.positions.len() / 3)
            .filter(|&i| m.plants[i * 4 + 3] > 0.0)
            .map(|i| std::array::from_fn(|a| f64::from(m.positions[i * 3 + a])))
            .collect();
        assert_eq!(carded.len() % 4, 0);
        let (lo, hi) = (f64::from(*hollow.start()), f64::from(*hollow.end() + 1));
        let inner = carded
            .chunks_exact(4)
            .filter(|card| {
                // Its centre in the hollow (its corners may reach into the walls round it).
                (0..3).all(|a| {
                    let c = card.iter().map(|p| p[a]).sum::<f64>() / 4.0;
                    c > lo + 1e-4 && c < hi - 1e-4
                })
            })
            .count();
        let total = carded.len() / 4;
        assert_eq!(
            inner, 0,
            "{inner} of the crown's {total} cards lie in its hollow"
        );
        // The outer faces keep theirs: 6 faces of 7 × 7 cells, about one card per face or more.
        assert!(
            total > 6 * 49 / 2,
            "the outer faces keep their cards: {total}"
        );
    }
);

dwell_world::both_faces!(
    /// Leaf cards never reach into a trunk (owner, 2026-11-11: "lots of z-fighting on tree trunks
    /// with the leaf cards"): a leaf with logs on its four sides and below it, as a crown's cell
    /// beside a trunk, draws no card with a corner inside a log's cell, nor one lying within ε of a
    /// log cell's face.
    fn leaf_cards_stay_out_of_the_logs_beside_them(f: FaceFrame) {
        let (leaf, log) = (require("dwell:leaves"), require("dwell:log[axis=y]"));
        // Face-locally below the leaf: the row under it on A, over it in storage on B.
        let below = if f.is_b() { 9 } else { 7 };
        let logs = [(9, 8, 8), (7, 8, 8), (8, 8, 9), (8, 8, 7), (8, below, 8)];
        let mut cells = vec![(8, 8, 8, leaf)];
        cells.extend(logs.iter().map(|&(x, y, z)| (x, y, z, log)));
        let m = in_cells(
            super::mesh_chunk_flags(
                &voxels(&cells),
                &[],
                f.is_b(),
                [0; 3],
                &[],
                None,
                ChunkFlags {
                    water_tops: true,
                    cards: true,
                    ..ChunkFlags::default()
                },
            )
            .expect("padded"),
        )
        .cutout;
        let carded: Vec<[f64; 3]> = (0..m.positions.len() / 3)
            .filter(|&i| m.plants[i * 4 + 3] > 0.0)
            .map(|i| std::array::from_fn(|a| f64::from(m.positions[i * 3 + a])))
            .collect();
        assert_eq!(carded.len() % 4, 0);
        const EPS: f64 = 0.02;
        for &(x, y, z) in &logs {
            let lo = [f64::from(x), f64::from(y), f64::from(z)];
            for (q, card) in carded.chunks_exact(4).enumerate() {
                for p in card {
                    let inside = (0..3).all(|a| p[a] > lo[a] + 1e-4 && p[a] < lo[a] + 1.0 - 1e-4);
                    assert!(!inside, "card {q} reaches into the log at {lo:?}: {p:?}");
                }
                for a in 0..3 {
                    for plane in [lo[a], lo[a] + 1.0] {
                        let along = card.iter().all(|p| (p[a] - plane).abs() < EPS);
                        assert!(
                            !along,
                            "card {q} lies along the log at {lo:?}'s face: {card:?}"
                        );
                    }
                }
            }
        }
    }
);

/// WORK_QUEUE item 26 round 2 (the in-game check: "no grain cracks seen round the bites … at 1, 3,
/// 6 swings"): after 1, 3 and 6 hits at one spot of a standing log, the wood's crack strokes show
/// on the bark left round the bites, not only inside the cut. Red before: the split runs along
/// the grain through the newest bite, and the V filled the cell's height there, so every dark
/// texel of the crack within the cell lay in the cut.
#[test]
fn a_bitten_logs_cracks_show_on_the_bark_round_the_bites() {
    // both-faces: face-local geometry, no up (a face-B chunk's cut is mirrored first,
    // `carve::mirror_cut`; the meshed cracks on both faces: `a_bitten_log_is_carved_…`).
    use dwell_protocol::chopping as chop;
    let wood = chop::wood("dwell:log").unwrap();
    let axe = chop::tool("dwell:stone_axe").unwrap();
    let look = wood.cut;
    let face = face_of(0, -1);
    let (pu, pv) = damage_mesh::plane_axes(0);
    let size = |a: usize| if a == 1 { CELL_H } else { 1.0 };
    for swings in [1, 3, 6] {
        let mut cut = Cut::default();
        for _ in 0..swings {
            let band = cut.open_band(0, 0);
            cut.swing(axe, 1, 0, band, 1.0);
            cut.bite(1, 0, band, 0.5, &look.bite);
        }
        let layer = super::cut_cracks(wood, &look, 1, &cut, [0, 0, 0], None).expect("cracks");
        let tile = crate::textures::TILE_ORDER[usize::from(layer.slot) - 1];
        let ts = crate::textures::TILE as i64;
        let (mut on_bark, mut in_cut) = (0, 0);
        for i in 0..=80 {
            for k in 0..=80 {
                let (t, u) = (f64::from(i) / 80.0, f64::from(k) / 80.0);
                let mut p = [0.0; 3];
                p[1] = t;
                p[2] = u;
                let q = [p[pu] * size(pu), p[pv] * size(pv)];
                let [a, b] = layer.uv(face, q);
                if !(0.0..1.0).contains(&a) || !(0.0..1.0).contains(&b) {
                    continue;
                }
                let px =
                    crate::textures::texel_of(tile, (a * ts as f64) as i64, (b * ts as f64) as i64);
                if px[0] >= 200 {
                    continue;
                }
                if crate::carve::depth_at(1, &cut, &look, cut.progress(wood, 1), 0, t, u) > 1e-9 {
                    in_cut += 1;
                } else {
                    on_bark += 1;
                }
            }
        }
        assert!(
            on_bark > 0,
            "{swings} swings: the crack shows on the bark ({on_bark} there, {in_cut} in the cut)"
        );
    }
}

/// WORK_QUEUE item 26 round 2 (the in-game check: "no grain cracks seen round the bites … on the
/// dark bark"): each wood's crack strokes, at the wood's crack `strength`, darken its bark (its
/// block's side tile) well past the bark's own darkest streaks, so a split reads on the bark.
/// Red before: they were painted in the bark ramp's shadow, the colour of the bark's own streaks
/// along the grain, at 0.6: a split the same dark as the grain it runs along.
#[test]
fn wood_cracks_read_darker_than_the_barks_own_streaks() {
    // both-faces: texture data only, no up.
    let lum =
        |p: [u8; 4]| 0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2]);
    let ts = crate::textures::TILE as i64;
    for w in dwell_protocol::chopping::WOODS {
        let Some(cracks) = w.cut.cracks else { continue };
        let side = dwell_blocks::info(require(w.block))
            .textures
            .map(|t| t.side)
            .expect("the wood's side tile");
        let mut bark: Vec<f64> = (0..ts * ts)
            .map(|k| lum(crate::textures::texel_of(side, k % ts, k / ts)))
            .collect();
        bark.sort_by(f64::total_cmp);
        let (darkest, median) = (bark[bark.len() / 20], bark[bark.len() / 2]);
        for tile in cracks.tiles {
            let crack = (0..ts * ts)
                .map(|k| crate::textures::texel_of(tile, k % ts, k / ts))
                .min_by(|a, b| lum(*a).total_cmp(&lum(*b)))
                .unwrap();
            // The shader's multiply: mix(1, crack, strength), on the bark's median texel.
            let k = 1.0 - cracks.strength * (1.0 - lum(crack) / 255.0);
            let split = median * k;
            assert!(
                split < 0.7 * darkest,
                "{} {tile}: a split darkens the bark to {split:.0}, its own streaks are {darkest:.0}",
                w.block
            );
        }
    }
}

dwell_world::both_faces!(
    /// docs/ECOLOGY.md §10: each growth stage of tall grass draws at its height from the block's
    /// data (`cross.heights`), a sprout to full (the default, 0.85 as before), on either face.
    fn tall_grass_draws_each_growth_stage_at_its_height(f: FaceFrame) {
        let grass_block = require("dwell:tall_grass");
        let stages: Vec<u16> = ["0", "1", "2", "3"]
            .iter()
            .map(|v| dwell_blocks::with_property(grass_block, "stage", v).expect("a stage"))
            .collect();
        assert_eq!(stages[3], grass_block, "the default is full height");
        let c = ChunkCoord::new(0, dwell_world::MIDPLANE_CHUNK_Y + 4, 0);
        let o = [0, c.y * CHUNK_SIZE, 0];
        let mut tops = Vec::new();
        for &s in &stages {
            let mut source = dwell_world::VoxelWorld::new(dwell_world::empty_generator(), None);
            for x in 0..8 {
                source.set_voxel(o[0] + 4 + x, o[1] + 4, o[2] + 4, cube_block());
                source.set_voxel(o[0] + 4 + x, o[1] + 5, o[2] + 4, s);
            }
            let m = local_mesh(f, mesh_on_face(&mut source, c, f).cutout);
            let ys: Vec<f32> = m.positions.iter().skip(1).step_by(3).copied().collect();
            assert!(!ys.is_empty(), "stage {s} drawn");
            // The tallest of the 8 (their jitter scales each), above the cube's top at y 5.
            tops.push(ys.iter().copied().fold(f32::MIN, f32::max) - 5.0);
        }
        for (i, &s) in stages.iter().enumerate() {
            let cross = dwell_blocks::info(s).cross.expect("a cross");
            assert!(
                tops[i] <= cross.height + 1e-4 && tops[i] >= cross.height * cross.height_min - 1e-4,
                "stage {i}: drawn {} for height {}",
                tops[i],
                cross.height
            );
        }
        assert!(
            tops.windows(2).all(|w| w[0] < w[1]),
            "taller stage by stage: {tops:?}"
        );
        assert_eq!(dwell_blocks::info(stages[3]).cross.unwrap().height, 0.85);
    }
);

/// The face-local upward water surface of `m` (transparent, after `local_mesh`): each triangle's
/// corners whose normal points up (its tops and slopes, not its sides or undersides).
fn up_triangles(m: &MeshArrays) -> Vec<[[f64; 3]; 3]> {
    m.indices
        .chunks_exact(3)
        .filter_map(|t| {
            let n = normal_of(&m.positions, t[0], t[1], t[2]);
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            (n[1] / l > 0.05).then(|| [t[0], t[1], t[2]].map(|i| vertex(&m.positions, i)))
        })
        .collect()
}

/// The heights at (x, z) of every triangle of `tris` over it (projected on xz).
fn heights_at(tris: &[[[f64; 3]; 3]], x: f64, z: f64) -> Vec<f64> {
    heights_within(tris, x, z, 1e-9)
}

/// [`heights_at`] counting a triangle only where (x, z) is `margin` (barycentric) inside it:
/// positive for strictly inside (a point on an edge two triangles share counts for neither),
/// negative to count a point just outside.
fn heights_within(tris: &[[[f64; 3]; 3]], x: f64, z: f64, margin: f64) -> Vec<f64> {
    tris.iter()
        .filter_map(|[a, b, c]| {
            let d = (b[0] - a[0]) * (c[2] - a[2]) - (c[0] - a[0]) * (b[2] - a[2]);
            if d.abs() < 1e-12 {
                return None;
            }
            let u = ((x - a[0]) * (c[2] - a[2]) - (c[0] - a[0]) * (z - a[2])) / d;
            let v = ((b[0] - a[0]) * (z - a[2]) - (x - a[0]) * (b[2] - a[2])) / d;
            (u >= -margin && v >= -margin && u + v <= 1.0 + margin)
                .then(|| a[1] + u * (b[1] - a[1]) + v * (c[1] - a[1]))
        })
        .collect()
}

/// BUGS.md #92's river (owner, 2026-11-12): a held river on sand stepping down ½ m (a cell) at
/// a time in diagonal bands, its bed stepping with it on flooded sand ramps. Water level row 7
/// where x + z ≤ 24, row 6 where x + z ≤ 34, row 5 beyond; two cells deep.
fn diagonal_staircase(f: FaceFrame) -> MeshArrays {
    let n = CHUNK_SIZE;
    let c = ChunkCoord::new(0, 2, 0);
    let o = [c.x * n, c.y * n, c.z * n];
    let mut source = dwell_world::VoxelWorld::new(dwell_world::empty_generator(), None);
    let sand = require("dwell:sand[flooded=true]");
    let ramp = require("dwell:sand_ramp[facing=south,flooded=true,half=bottom,shape=ramp]");
    let top = |x: i32, z: i32| match x + z {
        s if s <= 24 => 7,
        s if s <= 34 => 6,
        _ => 5,
    };
    for z in 0..=31 {
        for x in 0..=31 {
            let t = top(x, z);
            // The bed steps where the next column upstream stands a level higher.
            let stepped = top(x - 1, z) > t || top(x, z - 1) > t;
            for y in 1..=t {
                let m = if y >= t - 1 {
                    if y == t - 1 && stepped { ramp } else { water() }
                } else {
                    sand
                };
                source.set_voxel(o[0] + x, o[1] + y, o[2] + z, m);
            }
        }
    }
    local_mesh(f, mesh_on_face(&mut source, c, f).transparent)
}

dwell_world::both_faces!(
    /// BUGS.md #92 (owner, 2026-11-12: "I'm seeing some edge triangles and thin lines at slope
    /// transitions on downhill water."): a river stepping down ½ m in diagonal bands reads as one
    /// continuous surface. Over every column inside the river exactly one upward water face (no
    /// gap, no two slopes crossing in one cell, no top left under a slope), its height continuous
    /// across every column edge (the steps blend into slopes, no vertical riser between), no
    /// vertical water face standing above the lowest surface (the wedges where two steps met at a
    /// corner), and no vertex of one surface inside another's edge across a crease (cracks).
    fn a_diagonal_staircase_river_is_one_continuous_surface(f: FaceFrame) {
        let m = diagonal_staircase(f);
        let up = up_triangles(&m);
        let full = 1.0 - dwell_blocks::require_fluid("dwell:water").surface_drop;
        let lowest = 5.0 + full;
        let inside = |p: &[f64; 3]| (4.0..=28.0).contains(&p[0]) && (4.0..=28.0).contains(&p[2]);
        // One surface over each column, sampled off the cells' edges and diagonals.
        let mut bad = Vec::new();
        for z in 4..28 {
            for x in 4..28 {
                for (du, dv) in [(0.13, 0.31), (0.71, 0.18), (0.42, 0.87), (0.83, 0.57)] {
                    let (px, pz) = (f64::from(x) + du, f64::from(z) + dv);
                    // No gap (a crack): something over it, counting its edges; no overlap
                    // (two slopes, or a top under a slope): one at most strictly inside.
                    let loose = heights_within(&up, px, pz, 1e-6);
                    let strict = heights_within(&up, px, pz, -1e-6);
                    if loose.is_empty() || strict.len() > 1 {
                        bad.push(([px, pz], loose));
                    }
                }
            }
        }
        assert!(
            bad.is_empty(),
            "{:?}: {} points without exactly one surface over them, e.g. {:?}",
            f.face,
            bad.len(),
            &bad[..bad.len().min(4)]
        );
        // Continuous across every column edge, along x and along z.
        let mut jumps = Vec::new();
        for z in 4..28 {
            for x in 5..28 {
                for t in [0.27, 0.64] {
                    for (a, b) in [
                        (
                            [f64::from(x) - 0.01, f64::from(z) + t],
                            [f64::from(x) + 0.01, f64::from(z) + t],
                        ),
                        (
                            [f64::from(z) + t, f64::from(x) - 0.01],
                            [f64::from(z) + t, f64::from(x) + 0.01],
                        ),
                    ] {
                        let (ha, hb) = (heights_at(&up, a[0], a[1]), heights_at(&up, b[0], b[1]));
                        if let (Some(ha), Some(hb)) = (ha.first(), hb.first())
                            && (ha - hb).abs() > 0.05
                        {
                            jumps.push((a, *ha, *hb));
                        }
                    }
                }
            }
        }
        assert!(
            jumps.is_empty(),
            "{:?}: {} steps across a column edge, e.g. {:?}",
            f.face,
            jumps.len(),
            &jumps[..jumps.len().min(4)]
        );
        // No vertical water face above the lowest surface inside the river.
        let walls: Vec<[f64; 3]> = m
            .indices
            .chunks_exact(3)
            .filter_map(|t| {
                let nrm = normal_of(&m.positions, t[0], t[1], t[2]);
                let l = (nrm[0] * nrm[0] + nrm[1] * nrm[1] + nrm[2] * nrm[2]).sqrt();
                let ps = [t[0], t[1], t[2]].map(|i| vertex(&m.positions, i));
                let mid = std::array::from_fn(|a| (ps[0][a] + ps[1][a] + ps[2][a]) / 3.0);
                ((nrm[1] / l).abs() < 0.05
                    && inside(&mid)
                    && ps.iter().any(|p| p[1] > lowest + 1e-6))
                .then_some(mid)
            })
            .collect();
        assert!(
            walls.is_empty(),
            "{:?}: {} vertical water faces inside the river, e.g. {:?}",
            f.face,
            walls.len(),
            &walls[..walls.len().min(4)]
        );
        let ts: Vec<[f64; 3]> = crease_t_junctions(&m).into_iter().filter(inside).collect();
        assert!(
            ts.is_empty(),
            "{:?}: {} T-junctions across the river's creases, e.g. {:?}",
            f.face,
            ts.len(),
            &ts[..ts.len().min(6)]
        );
    }
);

/// A one-layer 3×3 crown round a trunk three logs tall: the leaves at y = 8 round the logs at
/// (8, 7..=9, 8), chunk-local storage cells (symmetric in y, so the same cells on both faces).
fn crown_round_a_trunk(leaf: u16, log: u16) -> Vec<(i32, i32, i32, u16)> {
    let mut cells: Vec<(i32, i32, i32, u16)> = (7..=9).map(|y| (8, y, 8, log)).collect();
    for z in 7..=9 {
        for x in 7..=9 {
            if (x, z) != (8, 8) {
                cells.push((x, 8, z, leaf));
            }
        }
    }
    cells
}

/// WORK_QUEUE item 48's checks on a crown round a trunk at (8, _, 8) (cells): each leaf beside the
/// log lies flat on the log's face (its inner face the whole 1×1 square there), no chamfer of
/// those leaves leans toward the trunk, no leaf vertex lies inside a log's cell, and the leaves'
/// surface is closed (its area vectors sum to zero: no gap or overlap at a seam).
fn assert_flush_round_the_trunk(m: &ChunkMeshes) {
    let c = &m.cutout;
    // The leaf faces, not their cards (`bare` 1).
    let tris: Vec<([[f64; 3]; 3], [f64; 3])> = c
        .indices
        .chunks_exact(3)
        .filter(|t| c.bare.get(t[0] as usize).is_none_or(|&b| b < 0.5))
        .map(|t| {
            let p = [0, 1, 2].map(|k| vertex(&c.positions, t[k]));
            (p, normal_of(&c.positions, t[0], t[1], t[2]))
        })
        .collect();
    let len = |n: [f64; 3]| (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    // Closed: Σ area · n = 0.
    let mut sum = [0.0; 3];
    for (_, n) in &tris {
        for a in 0..3 {
            sum[a] += n[a] / 2.0;
        }
    }
    assert!(
        len(sum) < 1e-4,
        "the crown's surface is open: Σ area·n = {sum:?}"
    );
    for (x, z) in [(9, 8), (7, 8), (8, 9), (8, 7)] {
        let toward = [f64::from(8 - x), 0.0, f64::from(8 - z)];
        let a = if x != 8 { 0 } else { 2 };
        let plane = if toward[a] < 0.0 { 9.0 } else { 8.0 };
        let (lo, hi) = (
            [f64::from(x), 8.0, f64::from(z)],
            [f64::from(x) + 1.0, 9.0, f64::from(z) + 1.0],
        );
        let mine = |p: &[[f64; 3]; 3]| {
            p.iter()
                .all(|q| (0..3).all(|b| q[b] > lo[b] - 1e-6 && q[b] < hi[b] + 1e-6))
        };
        let flat: f64 = tris
            .iter()
            .filter(|(p, n)| {
                mine(p) && p.iter().all(|q| (q[a] - plane).abs() < 1e-5) && n[a] * toward[a] > 0.0
            })
            .map(|(_, n)| len(*n) / 2.0)
            .sum();
        assert!(
            (flat - 1.0).abs() < 1e-4,
            "the leaf at ({x}, 8, {z}) lies {flat} m² flat on the log's face, not the whole square"
        );
        for (p, n) in tris.iter().filter(|(p, _)| mine(p)) {
            let along = (n[0] * toward[0] + n[1] * toward[1] + n[2] * toward[2]) / len(*n);
            let flat_face = (along - 1.0).abs() < 1e-6;
            assert!(
                along < 1e-6 || flat_face,
                "the leaf at ({x}, 8, {z}) has a chamfer leaning to the trunk: {p:?}"
            );
        }
    }
    for y in 7..=9 {
        let lo = [8.0, f64::from(y), 8.0];
        for (p, _) in &tris {
            for q in p {
                let inside = (0..3).all(|b| q[b] > lo[b] + 1e-6 && q[b] < lo[b] + 1.0 - 1e-6);
                assert!(!inside, "a leaf vertex inside the log at {lo:?}: {q:?}");
            }
        }
    }
    // The trunk's octagon stays inside its cells (its corners don't poke into the leaves).
    let o = &m.opaque;
    for i in 0..o.positions.len() / 3 {
        let q = vertex(&o.positions, i as u32);
        assert!(
            q[0] > 8.0 - 1e-5 && q[0] < 9.0 + 1e-5 && q[2] > 8.0 - 1e-5 && q[2] < 9.0 + 1e-5,
            "the trunk pokes out of its cells: {q:?}"
        );
    }
}

dwell_world::both_faces!(
    /// WORK_QUEUE item 48 (owner, 2026-11-12: "can we make it so the inside leave edges make a
    /// flat side against the logs, so it doesn't leave quite so much gapping where they connect to
    /// the tree"; "Flat leaf applied when a full Face touching"): a crown round a trunk of
    /// `dwell:log` (block data `leafFlush`) lies flat on it with its seams closed.
    fn leaves_lie_flat_against_a_trunk(f: FaceFrame) {
        let cells = crown_round_a_trunk(require("dwell:leaves"), require("dwell:log[axis=y]"));
        assert_flush_round_the_trunk(&mesh(&cells, f.is_b()));
    }
);

dwell_world::both_faces!(
    /// The same by data alone: shared/blocks/fixtures/test.json's `test:post` (an octagonal post
    /// with `leafFlush`) takes a crown flat against it.
    fn leaves_lie_flat_against_a_log_like_block_added_by_data(f: FaceFrame) {
        use dwell_blocks::fixture::{FLUIDS, STATES};
        let id = |name: &str| STATES.iter().position(|s| s.name == name).expect(name) as u16;
        let kinds = Kinds::new(&STATES, &FLUIDS, Some(id("dwell:water[fill=8]")));
        assert!(STATES[usize::from(id("test:post[axis=y]"))].leaf_flush);
        let cells = crown_round_a_trunk(id("dwell:leaves[flooded=false]"), id("test:post[axis=y]"));
        let m = mesh_chunk_with(&kinds, &voxels(&cells), f.is_b(), [0; 3]).expect("padded");
        assert_flush_round_the_trunk(&m);
    }
);

dwell_world::both_faces!(
    /// The forest edge (WORK_QUEUE item 32): a chunk's cutout mesh has its trees' leaf faces last,
    /// so a chunk past the far trees' hand-over draws `0..tree_leaves_from` (its plants, the
    /// leaves' cards) and none of the leaves the shader would drop there. Leaves and tall grass
    /// mixed along a row: every triangle before the split is a plant's or a card's, every one from
    /// it a tree leaf's, and the leaves' faces are all still there.
    fn a_chunks_cutout_puts_its_tree_leaves_after_its_plants(f: FaceFrame) {
        let leaf = require("dwell:leaves");
        let ground = cube_block();
        let mut cells = Vec::new();
        for x in 2..14 {
            cells.push((x, 4, 8, ground));
            cells.push((x, 5, 8, if x % 2 == 0 { tall_grass() } else { leaf }));
            cells.push((x, 9, 8, leaf));
        }
        let padded = voxels(&cells);
        let m = super::mesh_chunk_flags(
            &padded,
            &[],
            f.is_b(),
            [0; 3],
            &[],
            None,
            ChunkFlags {
                water_tops: true,
                cards: true,
                plants: 100,
                ..ChunkFlags::default()
            },
        )
        .expect("padded")
        .cutout;
        let leafy = |v: u32| m.plants[v as usize * 4 + 3] == crate::TREE_LEAF_FADE;
        let split = crate::tree_leaves_from(&m);
        assert!(
            split > 0 && split < m.indices.len(),
            "{split} of {}",
            m.indices.len()
        );
        assert_eq!(split % 3, 0);
        assert!(
            m.indices[..split].iter().all(|&v| !leafy(v)),
            "a leaf before the split"
        );
        assert!(
            m.indices[split..].iter().all(|&v| leafy(v)),
            "a plant after the split"
        );
        let leaf_vertices = (0..m.positions.len() as u32 / 3)
            .filter(|&v| leafy(v))
            .count();
        let mut drawn: Vec<u32> = m.indices[split..].to_vec();
        drawn.sort_unstable();
        drawn.dedup();
        assert_eq!(
            drawn.len(),
            leaf_vertices,
            "every leaf face drawn from the split"
        );
    }
);
