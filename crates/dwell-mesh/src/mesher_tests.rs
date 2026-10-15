//! `client/src/mesh/mesher.test.ts` and the logic cases of `client/src/world/shapes.test.ts`.

use super::*;
use dwell_blocks::{ShapeFace, require};

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
    } = mesh_chunk_with(&kinds, &cells, false).expect("padded volume");
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
}

#[test]
fn draws_a_ladder_as_one_plate_near_the_back_of_its_cell() {
    // A north ladder faces −Z (face 5): the plate is at z = 1 − 0.05.
    let ladder = require("dwell:ladder[facing=north,flooded=false]");
    let opaque = mesh(&[(0, 0, 0, ladder)], false).opaque;
    assert_eq!(quads(&opaque), 1);
    assert!(close(f64::from(opaque.positions[2]), 0.95, 2));
    assert_eq!(opaque.normals[2], -1.0);
}

#[test]
fn textures_faces_by_material_and_face_grass_top_and_side_unknown_ids_plain_magenta() {
    // A grass block under a cube (so a cube itself: its sides), and the middle of a 3 x 3 field
    // of grass (a flat half-height top, x 10..13).
    let mut cells = vec![(0, 0, 0, grass()), (0, 1, 0, cube_block()), (5, 0, 0, UNKNOWN)];
    for z in 0..3 {
        for x in 10..13 {
            cells.push((x, 0, z, grass()));
        }
    }
    let opaque = mesh(&cells, false).opaque;
    let tile_of = |q: usize| [opaque.tiles[q * 16], opaque.tiles[q * 16 + 1]];
    let find = |pred: &dyn Fn(usize) -> bool| (0..quads(&opaque)).find(|q| pred(*q)).expect("quad");
    let grass_top = find(&|q| opaque.normals[q * 12 + 1] == 1.0 && bounds(&opaque, q).1[0] == 12.0);
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
    assert!(!flat.is_empty() && flat.iter().all(|&y| y == 3.5), "{flat:?}");
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
    let m = mesh(&[(5, 3, 5, require("dwell:sand")), (6, 3, 5, water())], false);
    let tops: Vec<f32> = (0..m.transparent.normals.len() / 3)
        .filter(|&i| m.transparent.normals[i * 3 + 1] == 1.0)
        .map(|i| m.transparent.positions[i * 3])
        .collect();
    // The bank's water top spans x 5..6 and the water cell's 6..7.
    assert!(tops.iter().any(|&x| x < 6.0), "{tops:?}");
    assert!(tops.iter().any(|&x| x >= 6.0), "{tops:?}");
    // No water face stands between them (at x = 6, facing ±x).
    let between = (0..m.transparent.normals.len() / 3).any(|i| {
        m.transparent.normals[i * 3].abs() == 1.0 && (m.transparent.positions[i * 3] - 6.0).abs() < 0.01
    });
    assert!(!between);
}

#[test]
fn face_b_shapes_hang_toward_its_sky() {
    // A lone grass block on face B (meshed mirrored): the pyramid hangs, its slopes face down.
    let m = mesh(&[(5, 3, 5, grass())], true).opaque;
    let ys = normal_ys(&m);
    assert_eq!(ys.iter().filter(|&&y| y == -707).count(), 4, "{ys:?}");
}
