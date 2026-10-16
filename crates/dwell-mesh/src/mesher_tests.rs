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
fn dirt() -> u16 {
    require("dwell:dirt")
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
    } = mesh(&[(1, 1, 1, dirt())], false);
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
        }
    }
    let opaque = mesh(&cells, false).opaque;
    // Top, bottom, and one strip per side.
    assert_eq!(quads(&opaque), 6);
    let top = (0..quads(&opaque))
        .find(|q| opaque.normals[q * 12 + 1] == 1.0)
        .expect("a top");
    let (min, max) = bounds(&opaque, top);
    assert_eq!(min, [0.0, 1.0, 0.0]);
    assert_eq!(max, [32.0, 1.0, 32.0]);
    // The texture repeats per block: uvs span 32 blocks, the tile stays the stone tile.
    let r = tile_rect("stone");
    assert_eq!(opaque.uvs.iter().copied().fold(f32::MIN, f32::max), 32.0);
    assert!(close(f64::from(opaque.tiles[0]), r.u0, 2));
    assert!(close(f64::from(opaque.tiles[2]), r.u1 - r.u0, 2));
}

#[test]
fn keeps_materials_apart_and_hides_faces_against_full_neighbours_including_the_apron() {
    // Two different blocks side by side: the shared faces are hidden, the tops do not merge.
    let opaque = mesh(&[(0, 0, 0, dirt()), (1, 0, 0, grass())], false).opaque;
    assert_eq!(quads(&opaque), 10);
    // A block at the chunk's edge next to a solid neighbour chunk (apron): that face is hidden.
    let opaque = mesh(&[(31, 5, 5, dirt()), (32, 5, 5, dirt())], false).opaque;
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
    assert_eq!(kinds.of(oil).style.opacity, 0.8);
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
    // shared/blocks/fixtures/test.json adds `test:leaves` (cutout cube), `test:glass` (translucent
    // cube) and `test:reed` (a three-quad cross) and nothing else: the mesher sorts them by data.
    use dwell_blocks::fixture::{FLUIDS, STATES};
    let id = |name: &str| STATES.iter().position(|s| s.name == name).expect(name) as u16;
    let (leaves, glass, reed, stone, water) = (
        id("test:leaves"),
        id("test:glass"),
        id("test:reed"),
        id("dwell:stone"),
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
fn textures_faces_by_material_and_face_grass_top_and_side_unknown_ids_plain_magenta() {
    let opaque = mesh(&[(0, 0, 0, grass()), (5, 0, 0, UNKNOWN)], false).opaque;
    let tile_of = |q: usize| [opaque.tiles[q * 16], opaque.tiles[q * 16 + 1]];
    let find = |pred: &dyn Fn(usize) -> bool| (0..quads(&opaque)).find(|q| pred(*q)).expect("quad");
    let grass_top = find(&|q| opaque.normals[q * 12 + 1] == 1.0 && bounds(&opaque, q).1[0] == 1.0);
    let grass_side = find(&|q| opaque.normals[q * 12] == 1.0 && bounds(&opaque, q).1[0] == 1.0);
    let unknown = find(&|q| bounds(&opaque, q).0[0] >= 5.0);
    let rect = |name: &str| {
        let r = tile_rect(name);
        [r.u0 as f32, r.v0 as f32]
    };
    assert_eq!(tile_of(grass_top), rect("grass"));
    assert_eq!(tile_of(grass_side), rect("grassSide"));
    assert_eq!(tile_of(unknown), rect("plain"));
    // Textured faces shade white (the texture carries the colour); unknown ids keep a flat colour.
    assert_eq!(opaque.colors[grass_top * 12], 1.0);
    assert_eq!(opaque.colors[unknown * 12 + 1], 0.0); // magenta
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
        let a = mesh(&[(5, 0, 5, grass())], false);
        let b = mesh(&[(5, 0, 5, grass())], true);
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
    let m = mesh(&[(1, 1, 1, dirt())], false);
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
    let s = shape_of("dwell:stone");
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

/// Height of the shape's top surface at (x, z) in the cell (mirrors SurfaceHeight).
fn surface_height(s: &ShapeInfo, x: f64, z: f64) -> f64 {
    let h = |i: usize| f64::from(s.corners[i]) / 2.0;
    if s.diagonal == 0 {
        return if x >= z {
            h(0) + (h(1) - h(0)) * x + (h(2) - h(1)) * z
        } else {
            h(0) + (h(2) - h(3)) * x + (h(3) - h(0)) * z
        };
    }
    if x + z <= 1.0 {
        h(0) + (h(1) - h(0)) * x + (h(3) - h(0)) * z
    } else {
        h(2) + (h(3) - h(2)) * (1.0 - x) + (h(1) - h(2)) * (1.0 - z)
    }
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
    let h = surface_height(s, x, z);
    let lo = if s.inverted { 1.0 - h } else { 0.0 };
    let hi = if s.inverted { 1.0 } else { h };
    hi - lo > 1e-6 && y > lo + 1e-6 && y < hi - 1e-6
}

#[test]
fn shape_adjacency_leaves_no_holes_between_any_two_shapes_on_any_side() {
    let mut distinct: Vec<usize> = Vec::new();
    for st in &dwell_blocks::STATES {
        let s = usize::from(st.shape_index);
        if s != 0 && !distinct.contains(&s) {
            distinct.push(s);
        }
    }
    assert!(!distinct.is_empty());
    const SAMPLES: usize = 7;
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
