//! Block damage carved into the struck faces (docs/BLOCK_DAMAGE.md §6.2, WORK_QUEUE item 18: "a
//! real 3D chunk, not a 2D decal"; look only, collision stays the full block). A struck face of a
//! damaged cell leaves the greedy merge and is meshed as a grid over its marks' reach, its vertices
//! pushed in by the height field of `relief` (a soil's merged bowls, smooth-lit; a stone's angular
//! facets round each hit, flat-lit), with the face's own uvs (the material runs on into the dent),
//! its corner light, and the floor darker and more occluded by depth. The field is 0 on the cell's
//! edge, so the face's boundary stays where it was: watertight with the cell's other faces and its
//! neighbours'. Per block, from its `damage.look` data (`profile`, `facets`, `grid`, `triangles`).

use super::{MeshArrays, N, relief, vertex_rgb};
use crate::textures::TileRect;
use dwell_protocol::damage::{CarveProfile, DamageLook, DamageView, HitMark};
use dwell_world::CELL_H;
use relief::{Carve, Dent, Facets, Shard};

/// A damaged cell turned face-local: a face-B chunk is meshed with its rows turned over, so the
/// cell's row, its marks' up and down faces and their points' heights turn over too.
pub(crate) fn face_local(
    cell: [i32; 3],
    view: &DamageView,
    mirror: bool,
) -> ([i32; 3], DamageView) {
    if !mirror {
        return (cell, view.clone());
    }
    let marks = view
        .marks
        .iter()
        .map(|m| HitMark {
            // face-boundary: the mesher's face-local turn-over (as `carve::mirror_cut`).
            face: match m.face {
                2 => 3,
                3 => 2,
                f => f,
            },
            point: [m.point[0], 255 - m.point[1], m.point[2]],
        })
        .collect();
    (
        [cell[0], N - 1 - cell[1], cell[2]],
        DamageView {
            fraction: view.fraction,
            marks,
        },
    )
}

/// A struck face the mesher found visible: everything it knows about the face as a greedy quad
/// would have drawn it (chunk-local face-local cell, `FACES` index, colour, tile, corner light).
pub(crate) struct StruckFace {
    pub cell: [i32; 3],
    pub face: usize,
    pub color: u32,
    pub tile: TileRect,
    /// Corner light and flood light (u0v0, u1v0, u1v1, u0v1), as `Builder::quad` takes them.
    pub light: [f32; 4],
    pub flood: [[f32; 2]; 4],
}

/// A cell's extent along an axis (m).
fn ext(a: usize) -> f64 {
    if a == 1 { CELL_H } else { 1.0 }
}

/// The face's in-plane axes (u, v), as `Builder::quad` lays them (e_u × e_v = e_axis).
fn plane_axes(axis: usize) -> (usize, usize) {
    ((axis + 1) % 3, (axis + 2) % 3)
}

/// The height field of a face's marks, in the face's (u, v) metres (`relief::Carve`).
pub(crate) fn carve_of(face: usize, marks: &[&HitMark], look: &DamageLook, fraction: f64) -> Carve {
    let axis = face / 2;
    let (u, v) = plane_axes(axis);
    let (eu, ev) = (ext(u), ext(v));
    let grow = 0.55 + 0.45 * fraction;
    let radius = 0.5 * look.size * grow;
    let depth = look.depth * grow;
    // A mark's centre on the face (m), kept far enough in for its dent to fit the cell.
    let centre = |m: &HitMark, keep: f64| -> [f64; 2] {
        let at = |a: usize, e: f64| {
            let k = keep.min(0.5 * e);
            (f64::from(m.point[a]) / 255.0 * e).clamp(k, e - k)
        };
        [at(u, eu), at(v, ev)]
    };
    let (lo, hi) = ([0.0, 0.0], [eu, ev]);
    match look.profile {
        CarveProfile::Bowl => Carve::Bowl(Dent {
            centres: marks.iter().take(4).map(|m| centre(m, radius)).collect(),
            lo,
            hi,
            radius,
            depth,
            // A carve only cuts: the loose rim was the shader relief's.
            rim: 0.0,
        }),
        CarveProfile::Facets => Carve::Facets(Facets {
            shards: marks
                .iter()
                .enumerate()
                .take(4)
                .map(|(i, m)| shard(m, i, look.facets.max(3), radius, depth, centre(m, radius)))
                .collect(),
            lo,
            hi,
        }),
    }
}

/// A shard round `c` (face metres): `facets` uneven corners and an off-centre apex, jittered
/// stably by the mark (its point and order), so a remesh carves it the same.
fn shard(m: &HitMark, index: usize, facets: u32, r: f64, depth: f64, c: [f64; 2]) -> Shard {
    let seed = (u32::from(m.point[0]).wrapping_mul(73_856_093))
        ^ (u32::from(m.point[1]).wrapping_mul(19_349_663))
        ^ (u32::from(m.point[2]).wrapping_mul(83_492_791))
        ^ (index as u32).wrapping_mul(2_654_435_761);
    let jitter = |k: u32| {
        let h = (seed ^ k.wrapping_mul(0x9E37_79B9)).wrapping_mul(0x85EB_CA6B);
        f64::from(h >> 8) / f64::from(1u32 << 24)
    };
    let turn = jitter(99) * std::f64::consts::TAU;
    let corners = (0..facets)
        .map(|k| {
            let th = turn + f64::from(k) * std::f64::consts::TAU / f64::from(facets);
            let rr = r * (0.7 + 0.3 * jitter(k));
            [c[0] + rr * th.cos(), c[1] + rr * th.sin()]
        })
        .collect();
    let off = jitter(7) * std::f64::consts::TAU;
    let ar = r * 0.25 * jitter(8);
    Shard {
        apex: [c[0] + ar * off.cos(), c[1] + ar * off.sin()],
        corners,
        depth,
    }
}

/// The grid per side a face gets: the data's `grid`, coarsened until `faces` struck faces of the
/// cell fit its `triangles` budget (a face's tensor grid is (g + 2)² quads at most: the dent's g × g
/// and the flat strips round it), never under 2.
pub(crate) fn grid_for(look: &DamageLook, faces: usize) -> usize {
    let per_face = f64::from(look.triangles) / (2.0 * faces.max(1) as f64);
    let fit = (per_face.sqrt().floor() as usize).saturating_sub(2);
    (look.grid as usize).min(fit).max(2)
}

/// Grid lines along one axis of the face (m): its two edges and the dent's `g` divisions.
fn lines(lo: f64, hi: f64, e: f64, g: usize) -> Vec<f64> {
    let mut out = vec![0.0];
    for k in 0..=g {
        let x = lo + (hi - lo) * k as f64 / g as f64;
        if x > 1e-6 && x < e - 1e-6 {
            out.push(x);
        }
    }
    out.push(e);
    out
}

/// Carves one struck face into `m` (cell space, rows up, as the chunk's other faces): returns the
/// triangles it added.
pub(crate) fn carve_face(
    m: &mut MeshArrays,
    sf: &StruckFace,
    carve: &Carve,
    look: &DamageLook,
    grid: usize,
) -> usize {
    let axis = sf.face / 2;
    let sign = if sf.face.is_multiple_of(2) { 1.0 } else { -1.0 };
    let (u, v) = plane_axes(axis);
    let (eu, ev) = (ext(u), ext(v));
    let (lo, hi) = carve.reach();
    let us = lines(lo[0], hi[0], eu, grid);
    let vs = lines(lo[1], hi[1], ev, grid);
    let plane = f64::from(sf.cell[axis]) + if sign > 0.0 { 1.0 } else { 0.0 };
    let deepest = carve.depth().max(1e-6);
    // A point of the face (face metres) carved: its cell-space position and its depth share.
    let place = |pu: f64, pv: f64| -> ([f64; 3], f64) {
        let h = carve.height([pu, pv]);
        let mut p = [0.0; 3];
        p[axis] = plane + sign * h / ext(axis);
        p[u] = f64::from(sf.cell[u]) + pu / eu;
        p[v] = f64::from(sf.cell[v]) + pv / ev;
        (p, (-h / deepest).clamp(0.0, 1.0))
    };
    // Metres, for normals.
    let metres = |p: [f64; 3]| [p[0], p[1] * CELL_H, p[2]];
    // The smooth normal of the field (metres, world axes).
    let smooth_normal = |pu: f64, pv: f64| -> [f64; 3] {
        const E: f64 = 0.004;
        let gu = (carve.height([pu + E, pv]) - carve.height([pu - E, pv])) / (2.0 * E);
        let gv = (carve.height([pu, pv + E]) - carve.height([pu, pv - E])) / (2.0 * E);
        let l = (gu * gu + gv * gv + 1.0).sqrt();
        let mut n = [0.0; 3];
        n[u] = -gu / l;
        n[v] = -gv / l;
        n[axis] = sign / l;
        n
    };
    let bilinear = |a: f64, b: f64| -> (f32, [f32; 2]) {
        let w = [(1.0 - a) * (1.0 - b), a * (1.0 - b), a * b, (1.0 - a) * b];
        let mut l = 0.0;
        let mut f = [0.0; 2];
        for ((w, l0), f0) in w.iter().zip(sf.light).zip(sf.flood) {
            l += w * f64::from(l0);
            f[0] += w * f64::from(f0[0]);
            f[1] += w * f64::from(f0[1]);
        }
        (l as f32, [f[0] as f32, f[1] as f32])
    };
    let top = f64::from(sf.cell[1] + 1);
    let uv = |p: [f64; 3]| -> [f32; 2] {
        // The face's own uvs (`Builder::quad`): blocks across, metres up a side.
        let ty = if sf.tile.hang {
            (p[1] - top) * CELL_H + top
        } else {
            p[1] * CELL_H
        };
        let (su, sv) = match axis {
            1 => (p[0], p[2]),
            0 => (p[2], ty),
            _ => (p[0], ty),
        };
        let (su, sv) = if sf.tile.turn { (sv, su) } else { (su, sv) };
        [su as f32, sv as f32]
    };
    // Each grid point carved once: its position, depth share, smooth normal and light.
    struct Pt {
        p: [f64; 3],
        s: f64,
        n: [f64; 3],
        light: f32,
        flood: [f32; 2],
    }
    let smooth = carve.smooth();
    let grid: Vec<Pt> = vs
        .iter()
        .flat_map(|&pv| us.iter().map(move |&pu| (pu, pv)))
        .map(|(pu, pv)| {
            let (p, s) = place(pu, pv);
            let n = if smooth {
                smooth_normal(pu, pv)
            } else {
                [0.0; 3]
            };
            let (light, flood) = bilinear(pu / eu, pv / ev);
            Pt {
                p,
                s,
                n,
                light,
                flood,
            }
        })
        .collect();
    let row = us.len();
    let tile = [
        sf.tile.u0 as f32,
        sf.tile.v0 as f32,
        (sf.tile.u1 - sf.tile.u0) as f32,
        (sf.tile.v1 - sf.tile.v0) as f32,
    ];
    let order: [[usize; 3]; 2] = if sign > 0.0 {
        [[0, 1, 2], [0, 2, 3]]
    } else {
        [[0, 2, 1], [0, 3, 2]]
    };
    let vertex = |m: &mut MeshArrays, g: &Pt, n: [f64; 3]| {
        let rgb = vertex_rgb(sf.color, dwell_look::normal_tint(n[0], n[1], n[2]));
        let dim = 1.0 - look.darken * g.s;
        m.positions.extend(g.p.map(|x| x as f32));
        m.normals.extend(n.map(|x| x as f32));
        m.colors.extend(rgb.map(|c| (c * dim) as f32));
        m.uvs.extend(uv(g.p));
        m.tiles.extend(tile);
        // The floor of the dent is occluded by its walls.
        m.light.push(g.light * (1.0 - 0.4 * g.s as f32));
        m.flood.extend(g.flood);
    };
    // A bowl's grid shares its vertices (smooth normals); facets get their own per triangle (each
    // lit flat).
    let shared = (m.positions.len() / 3) as u32;
    if smooth {
        for g in &grid {
            vertex(m, g, g.n);
        }
    }
    let mut tris = 0;
    for j in 0..vs.len() - 1 {
        for i in 0..row - 1 {
            let corners = [
                i + row * j,
                i + 1 + row * j,
                i + 1 + row * (j + 1),
                i + row * (j + 1),
            ];
            for t in order {
                let ids = t.map(|c| corners[c]);
                tris += 1;
                if smooth {
                    m.indices.extend(ids.map(|k| shared + k as u32));
                    continue;
                }
                let [a, b, c] = ids.map(|k| metres(grid[k].p));
                let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
                let n = [
                    e1[1] * e2[2] - e1[2] * e2[1],
                    e1[2] * e2[0] - e1[0] * e2[2],
                    e1[0] * e2[1] - e1[1] * e2[0],
                ];
                let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
                let flat = n.map(|x| x / l);
                for k in ids {
                    vertex(m, &grid[k], flat);
                }
                let base = (m.positions.len() / 3) as u32 - 3;
                m.indices.extend([base, base + 1, base + 2]);
            }
        }
    }
    tris
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesher::{ChunkFlags, PADDED_VOLUME, mesh_chunk_damaged, padded_index};
    use dwell_protocol::damage::{DamageLookKind, look_of};
    use dwell_world::FaceFrame;
    use std::collections::HashMap;

    /// A face-local cell and its world twin on face `f` (face B's rows turned over).
    fn world_cell(f: FaceFrame, local: [i32; 3]) -> [i32; 3] {
        if f == FaceFrame::B {
            [local[0], N - 1 - local[1], local[2]]
        } else {
            local
        }
    }

    /// A face-local face index as face `f` names it in the world (B: up and down swap).
    fn world_face(f: FaceFrame, local: u8) -> u8 {
        match (f == FaceFrame::B, local) {
            (true, 2) => 3,
            (true, 3) => 2,
            (_, x) => x,
        }
    }

    /// Meshes one `block` cell (face-local `local`) on face `f` struck at `marks` (face-local
    /// faces, world points); returns the opaque mesh's vertices in face-local cells and the
    /// triangles.
    fn mesh_one(
        f: FaceFrame,
        block: &str,
        local: [i32; 3],
        marks: &[(u8, [u8; 3])],
        fraction: u8,
    ) -> (Vec<[f64; 3]>, Vec<[u32; 3]>) {
        let cell = world_cell(f, local);
        let mut padded = vec![0u16; PADDED_VOLUME];
        padded[padded_index(cell[0], cell[1], cell[2])] = dwell_blocks::require(block);
        let view = DamageView {
            fraction,
            marks: marks
                .iter()
                .map(|&(face, point)| HitMark {
                    face: world_face(f, face),
                    point,
                })
                .collect(),
        };
        let m = mesh_chunk_damaged(
            &padded,
            &[],
            f == FaceFrame::B,
            [0, 0, 0],
            &[],
            None,
            ChunkFlags::default(),
            &[(cell, view)],
        )
        .expect("padded volume")
        .opaque;
        let pts = m
            .positions
            .chunks_exact(3)
            .map(|p| {
                let y = f64::from(p[1]) / CELL_H;
                [
                    f64::from(p[0]),
                    if f == FaceFrame::B {
                        f64::from(N) - y
                    } else {
                        y
                    },
                    f64::from(p[2]),
                ]
            })
            .collect();
        let tris = m
            .indices
            .chunks_exact(3)
            .map(|t| [t[0], t[1], t[2]])
            .collect();
        (pts, tris)
    }

    fn local_view(marks: &[(u8, [u8; 3])], fraction: u8) -> DamageView {
        DamageView {
            fraction,
            marks: marks
                .iter()
                .map(|&(face, point)| HitMark { face, point })
                .collect(),
        }
    }

    dwell_world::both_faces!(
        /// A struck face is carved (BLOCK_DAMAGE.md §6.2): more triangles than the cube's, every
        /// vertex inside the cell (no decal off the face), the top's vertices below the face by
        /// exactly the height field, as deep as the data at full damage; for a soil's bowl and a
        /// stone's facets, face-locally on both faces.
        fn a_carved_face_lies_inside_the_cell_below_the_face_by_its_height_field(f: FaceFrame) {
            let local = [3, 5, 4];
            // The point is world-oriented (as the server records it): face B's turned over.
            let py = |y: u8| if f == FaceFrame::B { 255 - y } else { y };
            for block in ["dwell:dirt", "dwell:stone"] {
                let look = look_of(block).expect("its damage look is data");
                let marks = [(2u8, [128, py(255), 128])];
                let (pts, tris) = mesh_one(f, block, local, &marks, 255);
                assert!(tris.len() > 12, "{block}: carved, {} triangles", tris.len());
                for p in &pts {
                    for a in 0..3 {
                        let c = f64::from(local[a]);
                        assert!(
                            p[a] >= c - 1e-5 && p[a] <= c + 1.0 + 1e-5,
                            "{block}: inside the cell: {p:?}"
                        );
                    }
                }
                // The field, face-locally (the mark's point turned back over).
                let (_, v) = face_local(
                    world_cell(f, local),
                    &local_view(&[(world_face(f, 2), [128, py(255), 128])], 255),
                    f == FaceFrame::B,
                );
                let ms: Vec<_> = v.marks.iter().collect();
                let carve = carve_of(2, &ms, &look, 1.0);
                let top = f64::from(local[1] + 1);
                let mut deepest: f64 = 0.0;
                for p in pts
                    .iter()
                    .filter(|p| p[1] > top - 0.5 * look.depth / CELL_H - 0.5)
                {
                    // A top vertex (y near the top, not a side's lower ones).
                    if p[1] < top - 0.45 {
                        continue;
                    }
                    let below = (top - p[1]) * CELL_H;
                    // The top face is the (u, v) = (z, x) plane of axis y.
                    let h = carve.height([p[2] - f64::from(local[2]), p[0] - f64::from(local[0])]);
                    assert!(below >= -1e-5, "{block}: not above the face: {p:?}");
                    assert!(
                        (below + h).abs() < 1e-4,
                        "{block}: below by the field: {below} vs {}",
                        -h
                    );
                    deepest = deepest.max(below);
                }
                assert!(
                    deepest > 0.8 * look.depth && deepest <= look.depth + 1e-6,
                    "{block}: as deep as its data: {deepest} of {}",
                    look.depth
                );
            }
        }
    );

    /// The edges of a triangle list keyed by their end points (quantized, either order).
    fn edges(pts: &[[f64; 3]], tris: &[[u32; 3]]) -> HashMap<([i64; 3], [i64; 3]), usize> {
        let q = |p: [f64; 3]| p.map(|x| (x * 1e6).round() as i64);
        let mut out = HashMap::new();
        for t in tris {
            for k in 0..3 {
                let (a, b) = (q(pts[t[k] as usize]), q(pts[t[(k + 1) % 3] as usize]));
                *out.entry(if a < b { (a, b) } else { (b, a) }).or_insert(0) += 1;
            }
        }
        out
    }

    dwell_world::both_faces!(
        /// No cracks (BLOCK_DAMAGE.md §6.2): a carved face's triangles share every inner edge in
        /// pairs, and its open edges are exactly the face's own outline on the face's plane, which
        /// the cell's other faces and the neighbours' meet; on the top and a side (½ m tall), for
        /// one hit, two merged and a mark at the cell's edge.
        fn a_carved_face_is_watertight_and_keeps_its_outline(f: FaceFrame) {
            let mirror = f == FaceFrame::B;
            let world = world_cell(f, [2, 7, 9]);
            for block in ["dwell:dirt", "dwell:stone"] {
                let look = look_of(block).unwrap();
                for face in [2u8, 0, 5] {
                    for marks in [
                        vec![[128u8, 128, 128]],
                        vec![[90, 100, 170], [160, 150, 100]],
                        vec![[2, 250, 3]],
                    ] {
                        let view = local_view(
                            &marks
                                .iter()
                                .map(|&p| (world_face(f, face), p))
                                .collect::<Vec<_>>(),
                            200,
                        );
                        let (cell, v) = face_local(world, &view, mirror);
                        let ms: Vec<_> = v.marks.iter().collect();
                        let carve = carve_of(usize::from(face), &ms, &look, 200.0 / 255.0);
                        let sf = StruckFace {
                            cell,
                            face: usize::from(face),
                            color: 0xffffff,
                            tile: crate::textures::tile_rect("plain"),
                            light: [1.0; 4],
                            flood: [[1.0, 0.0]; 4],
                        };
                        let mut m = MeshArrays::default();
                        carve_face(&mut m, &sf, &carve, &look, grid_for(&look, 1));
                        let pts: Vec<[f64; 3]> = m
                            .positions
                            .chunks_exact(3)
                            .map(|p| [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])])
                            .collect();
                        let tris: Vec<[u32; 3]> = m
                            .indices
                            .chunks_exact(3)
                            .map(|t| [t[0], t[1], t[2]])
                            .collect();
                        let axis = usize::from(face / 2);
                        let plane = f64::from(cell[axis]) + if face % 2 == 0 { 1.0 } else { 0.0 };
                        let mut outline = 0.0;
                        for ((a, b), n) in edges(&pts, &tris) {
                            assert!(n <= 2, "an edge in {n} triangles");
                            if n == 2 {
                                continue;
                            }
                            // An open edge: on the face's plane, along the cell's outline.
                            let (a, b) = (a.map(|x| x as f64 / 1e6), b.map(|x| x as f64 / 1e6));
                            assert!(
                                (a[axis] - plane).abs() < 1e-5 && (b[axis] - plane).abs() < 1e-5,
                                "{block} face {face}: an open edge off the face: {a:?} {b:?}"
                            );
                            let on_edge = (0..3).filter(|&k| k != axis).any(|k| {
                                let c = f64::from(cell[k]);
                                let at = |p: [f64; 3]| {
                                    (p[k] - c).abs() < 1e-5 || (p[k] - c - 1.0).abs() < 1e-5
                                };
                                at(a) && at(b) && (a[k] - b[k]).abs() < 1e-5
                            });
                            assert!(
                                on_edge,
                                "{block} face {face}: an open edge inside the face: {a:?} {b:?}"
                            );
                            outline += ((0..3).map(|k| (a[k] - b[k]).powi(2)).sum::<f64>()).sqrt();
                        }
                        // The outline in cells: 4 cell edges (each 1 cell long in cell space).
                        assert!(
                            (outline - 4.0).abs() < 1e-4,
                            "{block} face {face}: outline {outline}"
                        );
                    }
                }
            }
        }
    );

    #[test]
    fn a_cells_struck_faces_keep_to_its_triangle_budget() {
        // both-faces: counts only, face-local by construction.
        for block in ["dwell:dirt", "dwell:stone"] {
            let mut look = look_of(block).unwrap();
            for budget in [128, 400, 2048] {
                look.triangles = budget;
                for faces in 1..=4usize {
                    let g = grid_for(&look, faces);
                    let mut total = 0;
                    for face in [0usize, 2, 4, 5].into_iter().take(faces) {
                        let mark = HitMark {
                            face: face as u8,
                            point: [100, 140, 120],
                        };
                        let carve = carve_of(face, &[&mark], &look, 1.0);
                        let sf = StruckFace {
                            cell: [1, 1, 1],
                            face,
                            color: 0xffffff,
                            tile: crate::textures::tile_rect("plain"),
                            light: [1.0; 4],
                            flood: [[1.0, 0.0]; 4],
                        };
                        total += carve_face(&mut MeshArrays::default(), &sf, &carve, &look, g);
                    }
                    assert!(
                        total <= budget as usize,
                        "{block}: {faces} faces, {total} triangles over {budget}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_fixture_material_by_data_alone_gets_its_facets() {
        // both-faces: one face-local face, no up.
        let look = DamageLook {
            kind: DamageLookKind::Chips,
            size: 0.4,
            color: [200, 180, 150],
            crack: [80, 60, 40],
            depth: 0.12,
            rim: 0.0,
            parallax: 0,
            darken: 0.2,
            profile: CarveProfile::Facets,
            facets: 7,
            grid: 12,
            triangles: 400,
        };
        let mark = HitMark {
            face: 2,
            point: [128, 255, 128],
        };
        let carve = carve_of(2, &[&mark], &look, 1.0);
        let Carve::Facets(fs) = &carve else {
            panic!("facets by its data");
        };
        assert_eq!(fs.shards[0].corners.len(), 7, "seven facets by its data");
        let sf = StruckFace {
            cell: [0, 0, 0],
            face: 2,
            color: 0xffffff,
            tile: crate::textures::tile_rect("plain"),
            light: [1.0; 4],
            flood: [[1.0, 0.0]; 4],
        };
        let mut m = MeshArrays::default();
        carve_face(&mut m, &sf, &carve, &look, grid_for(&look, 1));
        // Flat-lit facets: each triangle's three normals equal, and several slants among them.
        let mut slants = std::collections::HashSet::new();
        for t in m.normals.chunks_exact(9) {
            assert!(
                t[0..3] == t[3..6] && t[3..6] == t[6..9],
                "a facet is lit flat"
            );
            if t[1] < 0.999 {
                slants.insert(
                    t[0..3]
                        .iter()
                        .map(|x| (x * 100.0).round() as i32)
                        .collect::<Vec<_>>(),
                );
            }
        }
        assert!(slants.len() >= 5, "angular facets: {} slants", slants.len());
    }

    #[test]
    fn soils_carve_bowls_and_stone_facets_by_data() {
        // both-faces: data only, no up.
        assert_eq!(look_of("dwell:dirt").unwrap().kind, DamageLookKind::Scoops);
        assert_eq!(look_of("dwell:dirt").unwrap().profile, CarveProfile::Bowl);
        assert_eq!(look_of("dwell:sand").unwrap().profile, CarveProfile::Bowl);
        assert_eq!(look_of("dwell:stone").unwrap().kind, DamageLookKind::Shards);
        assert_eq!(
            look_of("dwell:stone").unwrap().profile,
            CarveProfile::Facets
        );
        assert_eq!(look_of("dwell:leaves"), None);
    }

    /// Stone's shard reads from standing height (browser check, 2026-11-11): at 0.34 m across,
    /// 0.10 m deep and a 0.12 darker floor, three pickaxe hits were invisible on the top face from
    /// 2 m; the carve is the stone look's `size`, `depth` and `darken` (`shared/blocks/dwell.json`).
    #[test]
    fn a_stone_shard_is_big_and_dark_enough_to_read_from_standing_height() {
        // both-faces: data only, no up.
        let carve = |b: &str| {
            let l = look_of(b).unwrap();
            (l.size, l.depth, l.darken)
        };
        let (size, depth, darken) = carve("dwell:stone");
        assert!(size >= 0.4, "a shard {size} m across");
        assert!(depth >= 0.15, "a shard {depth} m deep");
        assert!(darken >= 0.35, "its floor {darken} darker");
    }

    /// The carved mesh's cost, measured natively (BLOCK_DAMAGE.md §6.2): a chunk of stone and dirt
    /// with 50 damaged cells (one to four struck faces each), against the same chunk undamaged.
    #[test]
    fn fifty_damaged_cells_cost() {
        // face-a-only: a cost measurement; the carve is face-local (the tests above cover face B).
        let (stone, dirt) = (
            dwell_blocks::require("dwell:stone"),
            dwell_blocks::require("dwell:dirt"),
        );
        let mut padded = vec![0u16; PADDED_VOLUME];
        // A 10 × 5 grid of pillars 2 high on a floor, so each damaged cell shows several faces.
        for x in 0..N {
            for z in 0..N {
                padded[padded_index(x, 0, z)] = stone;
            }
        }
        let mut damage = Vec::new();
        for k in 0..50 {
            let (x, z) = (1 + 3 * (k % 10), 2 + 3 * (k / 10));
            let block = if k % 2 == 0 { stone } else { dirt };
            padded[padded_index(x, 1, z)] = block;
            let faces: Vec<u8> = [2u8, 0, 4, 1]
                .into_iter()
                .take(1 + k as usize % 4)
                .collect();
            damage.push((
                [x, 1, z],
                DamageView {
                    fraction: (60 + 4 * k) as u8,
                    marks: faces
                        .iter()
                        .map(|&f| HitMark {
                            face: f,
                            point: [(70 + 3 * k) as u8, 140, (200 - 2 * k) as u8],
                        })
                        .collect(),
                },
            ));
        }
        let run = |d: &[([i32; 3], DamageView)]| {
            let t = std::time::Instant::now();
            let mut tris = 0;
            for _ in 0..20 {
                let m = mesh_chunk_damaged(
                    &padded,
                    &[],
                    false,
                    [0, 0, 0],
                    &[],
                    None,
                    ChunkFlags::default(),
                    d,
                )
                .unwrap();
                tris = m.opaque.indices.len() / 3;
            }
            (tris, t.elapsed().as_secs_f64() * 1000.0 / 20.0)
        };
        let (plain, plain_ms) = run(&[]);
        let (carved, carved_ms) = run(&damage);
        println!(
            "50 damaged cells: {carved} opaque triangles ({plain} undamaged, +{} = {:.0} a cell), meshed in {carved_ms:.2} ms ({plain_ms:.2} ms undamaged)",
            carved - plain,
            (carved - plain) as f64 / 50.0
        );
        assert!(carved > plain);
        let budget: usize = 50
            * look_of("dwell:dirt")
                .unwrap()
                .triangles
                .max(look_of("dwell:stone").unwrap().triangles) as usize;
        assert!(carved - plain <= budget, "within the cells' budgets");
    }
}
