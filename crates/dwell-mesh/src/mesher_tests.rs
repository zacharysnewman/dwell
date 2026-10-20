//! `client/src/mesh/mesher.test.ts` and the logic cases of `client/src/world/shapes.test.ts`.

use super::*;
use dwell_blocks::{RenderMode, ShapeFace, require};

const UNKNOWN: u16 = 65_000;

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
    // Per layer: top or bottom, and one strip per side (the two layers' tops and bottoms hide).
    assert_eq!(quads(&opaque), 10);
    let bottom = (0..quads(&opaque))
        .find(|q| opaque.normals[q * 12 + 1] == -1.0)
        .expect("a bottom");
    let (min, max) = bounds(&opaque, bottom);
    assert_eq!(min, [0.0, 0.0, 0.0]);
    assert_eq!(max, [32.0, 0.0, 32.0]);
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
    let (oil, water) = (id("test:oil"), id("dwell:water"));
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
        id("dwell:water"),
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
        let see = quads(&m.cutout) + quads(&m.transparent);
        // Two blocks in a row along z: their shared faces skipped, coplanar faces merged (−x, ±y),
        // the end faces, and the +x face of the one beside stone drawn only for the other.
        assert_eq!(see, 6, "{}", STATES[usize::from(b)].name);
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

#[test]
fn draws_tall_grass_as_crossed_cutout_quads_from_its_data() {
    let info = dwell_blocks::info(tall_grass());
    let cross = info.cross.expect("the data gives it a cross");
    let m = mesh(&[(4, 0, 4, grass()), (4, 1, 4, tall_grass())], false);
    let c = &m.cutout;
    // One quad per cross quad (4 vertices, 2 triangles), drawn once: the pipeline is two-sided.
    let quads_n = usize::from(cross.quads);
    assert_eq!(c.positions.len(), quads_n * 4 * 3);
    assert_eq!(c.indices.len(), quads_n * 6);
    assert_eq!(c.plants.len(), quads_n * 4 * 4);
    // The grass block below keeps its top face: the plant hides nothing.
    let alone = quads(&mesh(&[(4, 0, 4, grass())], false).opaque);
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
        .flat_map(|x| [(x, 0, 0, grass()), (x, 1, 0, tall_grass())])
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
    // 24 vertices × (3 + 3 + 3 + 2 + 4) floats + 36 indices, 4 bytes each.
    assert_eq!(m.opaque.byte_len(), 4 * (24 * 15 + 36));
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
    // Every shape in the table: the states' and every shape the adaptive rules derive, upright and
    // hanging, against each other on every side.
    let distinct: Vec<usize> = (1..dwell_blocks::SHAPES.len()).collect();
    assert!(distinct.len() > 190);
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
fn a_lone_grass_block_is_a_half_pyramid_and_a_field_rests_at_half_height() {
    let lone = mesh(&[(5, 3, 5, grass())], false).opaque;
    let ys = normal_ys(&lone);
    // Four faces sloping at 45° (normal y = cos 45°) over the floor; no vertical side.
    assert_eq!(ys.iter().filter(|&&y| y == 707).count(), 4, "{ys:?}");
    assert!(ys.iter().all(|&y| y == 707 || y == -1000), "{ys:?}");
    assert!(lone.positions.chunks_exact(3).all(|p| p[1] <= 3.5));
    // The middle of a field: flat at half height, with the grass top tile.
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
        !flat.is_empty() && flat.iter().all(|&y| y == 3.5),
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
fn a_shaped_bank_beside_water_holds_water_in_its_open_part() {
    // Sand at the water's row beside water: shaped, and flooded — its cell's water faces are drawn
    // (toward the air), and the water beside it draws none against it.
    let m = mesh(
        &[(5, 3, 5, require("dwell:sand")), (6, 3, 5, water())],
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
        let wet_above = over(0, 0, 0) == water() || dwell_blocks::adaptive::flooded_at(over, 1);
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
        let shape = dwell_blocks::adaptive::shape_at(|dx, dy, dz| at(qx + dx, qy + dy, qz + dz), 1);
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

#[test]
fn face_b_shapes_hang_toward_its_sky() {
    // A lone grass block on face B (meshed mirrored): the pyramid hangs, its slopes face down.
    let m = mesh(&[(5, 3, 5, grass())], true).opaque;
    let ys = normal_ys(&m);
    assert_eq!(ys.iter().filter(|&&y| y == -707).count(), 4, "{ys:?}");
}

#[test]
fn a_level_field_of_natural_blocks_merges_its_flat_tops() {
    // 10 × 10 grass: the 8 × 8 inside rests at half height in one rectangle; the rim slopes.
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
        assert_eq!(m.positions[v * 3 + 1], 2.5);
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
    // takes the `dirtSkirt` tile. At a cliff the grass's edge is a slope down to the dirt's top.
    let dirt = require("dwell:dirt");
    let m = mesh(&plateau(dirt, grass(), 0), false).opaque;
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
    let hanging: Vec<_> = plateau(dirt, grass(), 0)
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
    // `{ "dwell:stone": "grassSide" }` and nothing else.
    use dwell_blocks::fixture::{FLUIDS, STATES};
    let id = |name: &str| STATES.iter().position(|s| s.name == name).expect(name) as u16;
    let (loam, stone, water) = (
        id("test:loam"),
        id("dwell:stone[flooded=false]"),
        id("dwell:water"),
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
    let kinds = Kinds::new(&STATES, &FLUIDS, Some(id("dwell:water")));
    for name in ["test:reed", "test:hedge"] {
        let m = mesh_chunk_with(&kinds, &voxels(&[(1, 1, 1, id(name))]), false, [0; 3])
            .expect("padded volume");
        assert!(!m.cutout.bare.is_empty(), "{name}");
        assert!(m.cutout.bare.iter().all(|&b| b == 0.0), "{name} is inked");
    }
}
