//! Ground items baked into a chunk's opaque mesh (docs/adr/0071-ground-items.md): the scatter
//! (`dwell_world::ground`) read from the job's sky volume, each item a few small polygons in the
//! face-local frame, so drawing hundreds costs no draw call and no instance buffer of its own.

use super::{Kinds, MeshArrays, N, surface};
use crate::flood::Flood;
use dwell_blocks::find_block;
use dwell_world::ground::{GroundShape, Placed, Rules};
use dwell_world::{CELL_H, SKY_APRON, SKY_EXTENT, SKY_VOLUME};

/// A mesh job's ground items: the world seed and the chunk's gone cells (chunk-local, world-oriented).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GroundJob {
    pub seed: u64,
    pub gone: Vec<[i32; 3]>,
    /// The sky volume as generated (empty: as the job's): the rules read the terrain as
    /// generated, so the leaves cleared over a stick leave it (owner, 2026-11-09).
    pub generated: Vec<u16>,
}

/// The ground items of the chunk at `origin` (world cells), read from its world-oriented sky
/// volume `padded` (a smaller apron: none, as the rules look up to the sky apron's reach).
pub fn scatter(padded: &[u16], origin: [i32; 3], job: &GroundJob) -> Vec<Placed> {
    if padded.len() != SKY_VOLUME {
        return Vec::new();
    }
    let [ex, ey, ez] = SKY_EXTENT;
    let [ax, ay, az] = SKY_APRON;
    let generated = if job.generated.len() == SKY_VOLUME {
        &job.generated[..]
    } else {
        padded
    };
    let mut get = |c: [i32; 3], gen_: bool| {
        let (x, y, z) = (
            c[0] - origin[0] + ax,
            c[1] - origin[1] + ay,
            c[2] - origin[2] + az,
        );
        let v = if gen_ { generated } else { padded };
        ((0..ex).contains(&x) && (0..ey).contains(&y) && (0..ez).contains(&z))
            .then(|| v[(x + ex * (z + ez * y)) as usize])
    };
    let c = dwell_world::ChunkCoord::from_array(origin.map(|v| v.div_euclid(N)));
    let mut out = Rules::shipped().scatter_over(&mut get, job.seed, c);
    if !job.gone.is_empty() {
        out.retain(|p| {
            let l = [
                p.cell[0] - origin[0],
                p.cell[1] - origin[1],
                p.cell[2] - origin[2],
            ];
            !job.gone.contains(&l)
        });
    }
    out
}

/// Appends `items` to `m` (chunk-local cells, face-local: y up, rows), `mirror` for face B's chunk.
pub(super) fn append(
    m: &mut MeshArrays,
    kinds: &Kinds,
    flood: &Flood,
    items: &[Placed],
    origin: [i32; 3],
    mirror: bool,
) {
    let rules = Rules::shipped();
    for it in items {
        let Some(k) = rules.kind(it.kind) else {
            continue;
        };
        let Some(b) = find_block(k.texture) else {
            continue;
        };
        let style = &kinds.of(b.first_state).style;
        let (_, tile) = surface(style, 0, 1);
        let mut l = [
            it.cell[0] - origin[0],
            it.cell[1] - origin[1],
            it.cell[2] - origin[2],
        ];
        if mirror {
            l[1] = N - 1 - l[1];
        }
        let lit = flood.cell(l);
        let base = [
            f64::from(l[0]) + 0.5 + it.offset[0],
            f64::from(l[1]) * CELL_H,
            f64::from(l[2]) + 0.5 + it.offset[1],
        ];
        let size = k.size.map(|v| v * it.scale);
        let (s, c) = it.yaw.sin_cos();
        let place = |p: [f64; 3]| -> [f64; 3] {
            let (x, z) = (p[0] * c - p[2] * s, p[0] * s + p[2] * c);
            [base[0] + x, base[1] + p[1], base[2] + z]
        };
        let color = k.color.map(|v| f64::from(v) / 255.0);
        for face in polygons(k.shape, size, false) {
            let pts: Vec<[f64; 3]> = face.iter().map(|&p| place(p)).collect();
            push_polygon(m, &pts, color, &tile, lit, CELL_H);
        }
    }
}

/// An item's look (`shared/inventory` `look`) as a mesh in metres, item-local (its base's middle at
/// the origin, +y up), every side drawn (the underside too: a loose item spins in the air): the
/// same shape, tile and tint a ground item of it is drawn with, for the loose item.
pub fn look_mesh(look: &dwell_protocol::inventory::ItemLook) -> MeshArrays {
    let tile = crate::textures::tile_rect(look_tile(look));
    let color = look.color.map(|v| f64::from(v) / 255.0);
    let mut m = MeshArrays::default();
    for (face, c) in look_faces(look) {
        push_polygon(&mut m, &face, c.unwrap_or(color), &tile, [1.0, 0.0], 1.0);
    }
    m
}

/// A face: its points, counter-clockwise from outside.
type Polygon = Vec<[f64; 3]>;

/// A look's faces, each with its own colour where it has one (a model look's palette colours).
fn look_faces(look: &dwell_protocol::inventory::ItemLook) -> Vec<(Polygon, Option<[f64; 3]>)> {
    if look.shape == GroundShape::Model {
        return model_faces(look.model)
            .into_iter()
            .map(|(f, c)| (f, Some(c)))
            .collect();
    }
    polygons(look.shape, look.size, true)
        .into_iter()
        .map(|f| (f, None))
        .collect()
}

/// A prop's triangles as a lying look (ADR 0070 §8): its long axis (the GLB's +y) turned along +x,
/// its base on y = 0 and its middle over the origin, each with its face's colour as an sRGB tint
/// (0..1, as a look's `color`); empty for a prop not built in.
pub fn model_faces(name: &str) -> Vec<(Vec<[f64; 3]>, [f64; 3])> {
    let Some((_, m)) = dwell_models::prop(name) else {
        return Vec::new();
    };
    // Turned -90° about z: the GLB's +y (its length) to +x, its +x to -y.
    let lie = |p: [f32; 3]| [f64::from(p[1]), -f64::from(p[0]), f64::from(p[2])];
    let floor = m
        .positions
        .iter()
        .map(|&p| lie(p)[1])
        .fold(f64::MAX, f64::min);
    let srgb = |c: f32| f64::from(c.clamp(0.0, 1.0)).powf(1.0 / 2.2);
    m.indices
        .chunks_exact(3)
        .map(|t| {
            let pts = t
                .iter()
                .map(|&i| {
                    let [x, y, z] = lie(m.positions[i as usize]);
                    [x, y - floor, z]
                })
                .collect();
            let c = m.colors[t[0] as usize];
            (pts, [srgb(c[0]), srgb(c[1]), srgb(c[2])])
        })
        .collect()
}

/// The tile a look is drawn with: its texture block's side tile.
fn look_tile(look: &dwell_protocol::inventory::ItemLook) -> &'static str {
    find_block(look.texture)
        .and_then(|b| crate::style::material_style(b.first_state).textures)
        .map_or("plain", |t| t.side)
}

/// An item look's picture for the hotbar and the backpack (`TILE` × `TILE` RGBA8, sRGB, top row
/// first): the look's shape seen from above and the side at a slant, filled with its tile tinted
/// its colour and shaded by each face's slope, as the world draws it; clear around it.
pub fn look_swatch(look: &dwell_protocol::inventory::ItemLook) -> Vec<u8> {
    use crate::textures::TILE;
    let tile = look_tile(look);
    let (faces, colours): (Vec<_>, Vec<_>) = look_faces(look).into_iter().unzip();
    // The view: turned 35° about up (a rod lies across the picture's diagonal), tilted 30° down.
    let (ys, yc) = (35f64.to_radians().sin(), 35f64.to_radians().cos());
    let (ts, tc) = (30f64.to_radians().sin(), 30f64.to_radians().cos());
    let view = |p: [f64; 3]| -> [f64; 3] {
        let (x, z) = (p[0] * yc - p[2] * ys, p[0] * ys + p[2] * yc);
        // Screen x right, y up, depth toward the eye.
        [x, p[1] * tc + z * ts, z * tc - p[1] * ts]
    };
    let pts: Vec<Vec<[f64; 3]>> = faces
        .iter()
        .map(|f| f.iter().map(|&p| view(p)).collect())
        .collect();
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for p in pts.iter().flatten() {
        for k in 0..2 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let n = TILE as f64;
    let span = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1e-9);
    let scale = n * 0.86 / span;
    let mid = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    let to_px = |p: [f64; 3]| {
        [
            (p[0] - mid[0]) * scale + n / 2.0,
            n / 2.0 - (p[1] - mid[1]) * scale,
            p[2],
        ]
    };
    let mut px = vec![0u8; TILE * TILE * 4];
    let mut depth = vec![f64::MIN; TILE * TILE];
    let tint = look.color.map(|c| f64::from(c) / 255.0);
    for ((face, world), colour) in pts.iter().zip(&faces).zip(&colours) {
        let tint = colour.unwrap_or(tint);
        let n3 = newell(world);
        // Faces away from the eye (in view space) are hidden.
        let nv = view(n3);
        if nv[2] <= 0.0 {
            continue;
        }
        let shade = dwell_look::normal_tint(n3[0], n3[1], n3[2]);
        let q: Vec<[f64; 3]> = face.iter().map(|&p| to_px(p)).collect();
        for i in 1..q.len() - 1 {
            let (a, b, c) = (q[0], q[i], q[i + 1]);
            let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
            if area.abs() < 1e-12 {
                continue;
            }
            for y in 0..TILE {
                for x in 0..TILE {
                    let (fx, fy) = (x as f64 + 0.5, y as f64 + 0.5);
                    let w0 = ((b[0] - fx) * (c[1] - fy) - (b[1] - fy) * (c[0] - fx)) / area;
                    let w1 = ((c[0] - fx) * (a[1] - fy) - (c[1] - fy) * (a[0] - fx)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                        continue;
                    }
                    let d = w0 * a[2] + w1 * b[2] + w2 * c[2];
                    let at = y * TILE + x;
                    if d <= depth[at] {
                        continue;
                    }
                    depth[at] = d;
                    // The tile's grain at the item's scale (a few texels across, as on the ground).
                    let t = crate::textures::texel_of(
                        tile,
                        ((x * 3 + y) % TILE) as i64,
                        ((TILE - 1 - y) * 2 % TILE) as i64,
                    );
                    let o = at * 4;
                    for k in 0..3 {
                        let lin = crate::textures::srgb_to_linear(t[k]) * tint[k] * shade[k];
                        px[o + k] = crate::textures::linear_to_srgb_byte(lin);
                    }
                    px[o + 3] = 255;
                }
            }
        }
    }
    px
}

/// A polygon's unit normal from Newell's sum.
fn newell(pts: &[[f64; 3]]) -> [f64; 3] {
    let mut n = [0.0f64; 3];
    for i in 0..pts.len() {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
    n.map(|v| v / len)
}

/// The shape's faces (convex, counter-clockwise from outside), item-local metres: its base's
/// middle at the origin, +y up. The underside only with `base` (lying on the ground it is hidden).
fn polygons(shape: GroundShape, [sx, sy, sz]: [f64; 3], base: bool) -> Vec<Vec<[f64; 3]>> {
    match shape {
        // A model look's faces are its prop's (`model_faces`), not a shape's.
        GroundShape::Model => Vec::new(),
        GroundShape::Cluster => {
            // Tiny lumps heaped in the footprint (fractions of it): a ring of five on the
            // ground, a sixth on top in the middle; each a lump a third of the footprint across.
            const SEEDS: [[f64; 3]; 6] = [
                [-0.3, 0.0, -0.22],
                [0.28, 0.0, -0.26],
                [0.34, 0.0, 0.2],
                [-0.06, 0.0, 0.32],
                [-0.34, 0.0, 0.12],
                [0.0, 0.45, 0.0],
            ];
            let one = [sx * 0.34, sy * 0.55, sz * 0.3];
            let mut out = Vec::new();
            for (i, [ox, oy, oz]) in SEEDS.into_iter().enumerate() {
                // Each turned its own way, so they don't line up.
                let a = i as f64 * 1.1;
                let (s, c) = a.sin_cos();
                // The top one rests on the ring: its base always drawn (it is seen from below
                // between them).
                for face in polygons(GroundShape::Lump, one, base || oy > 0.0) {
                    out.push(
                        face.into_iter()
                            .map(|[x, y, z]| {
                                [
                                    ox * sx + x * c - z * s,
                                    oy * sy + y,
                                    oz * sz + x * s + z * c,
                                ]
                            })
                            .collect(),
                    );
                }
            }
            out
        }
        GroundShape::Rod => {
            let (x, z) = (sx / 2.0, sz / 2.0);
            let c = |i: usize| -> [f64; 3] {
                [
                    if i & 1 != 0 { x } else { -x },
                    if i & 2 != 0 { sy } else { 0.0 },
                    if i & 4 != 0 { z } else { -z },
                ]
            };
            let mut out = vec![
                vec![c(2), c(6), c(7), c(3)], // top
                vec![c(1), c(3), c(7), c(5)], // +x
                vec![c(0), c(4), c(6), c(2)], // −x
                vec![c(4), c(5), c(7), c(6)], // +z
                vec![c(0), c(2), c(3), c(1)], // −z
            ];
            if base {
                out.push(vec![c(0), c(1), c(5), c(4)]); // −y
            }
            out
        }
        GroundShape::Lump => {
            // Three rings of a squat hexagon: a lump wider at its middle, rounded at the top.
            let ring = |y: f64, r: f64| -> Vec<[f64; 3]> {
                (0..6)
                    .map(|i| {
                        let a = f64::from(i) * std::f64::consts::TAU / 6.0 + 0.3;
                        [a.cos() * sx / 2.0 * r, y, a.sin() * sz / 2.0 * r]
                    })
                    .collect()
            };
            let (lo, mid, hi) = (ring(0.0, 0.8), ring(sy * 0.5, 1.0), ring(sy, 0.55));
            let mut out = Vec::new();
            for i in 0..6 {
                let j = (i + 1) % 6;
                out.push(vec![lo[i], mid[i], mid[j], lo[j]]);
                out.push(vec![mid[i], hi[i], hi[j], mid[j]]);
            }
            out.push(hi.iter().rev().copied().collect());
            if base {
                out.push(lo.clone());
            }
            out
        }
    }
}

pub(super) fn push_polygon(
    m: &mut MeshArrays,
    pts: &[[f64; 3]],
    color: [f64; 3],
    tile: &crate::textures::TileRect,
    lit: [f32; 2],
    row_m: f64,
) {
    // Its normal in metres (the points are).
    let n = newell(pts);
    let tint = dwell_look::normal_tint(n[0], n[1], n[2]);
    let base = (m.positions.len() / 3) as u32;
    for p in pts {
        // A chunk's cell space: rows up (the mesh is turned into metres after); `row_m` 1: metres.
        m.positions
            .extend([p[0] as f32, (p[1] / row_m) as f32, p[2] as f32]);
        m.normals.extend(n.map(|v| v as f32));
        m.colors.extend([
            (color[0] * tint[0]) as f32,
            (color[1] * tint[1]) as f32,
            (color[2] * tint[2]) as f32,
        ]);
        // In blocks, projected along the dominant axis; a few times the tile's density, so the
        // grain shows at their size.
        let (u, v) = if n[1].abs() > 0.6 {
            (p[0], p[2])
        } else if n[0].abs() > n[2].abs() {
            (p[2], p[1])
        } else {
            (p[0], p[1])
        };
        m.uvs.extend([(u * 3.0) as f32, (v * 3.0) as f32]);
        m.tiles.extend([
            tile.u0 as f32,
            tile.v0 as f32,
            (tile.u1 - tile.u0) as f32,
            (tile.v1 - tile.v0) as f32,
        ]);
        m.light.push(1.0);
        m.flood.extend(lit);
    }
    for i in 1..pts.len().saturating_sub(1) as u32 {
        m.indices.extend([base, base + i, base + i + 1]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_face_of_both_shapes_faces_outward() {
        // both-faces: item-local geometry, mirrored with the chunk's mesh.
        for shape in [GroundShape::Rod, GroundShape::Lump] {
            // (The cluster is lumps; a lump's faces are checked here.)
            let size = [0.4, 0.1, 0.2];
            let faces = polygons(shape, size, true);
            let mut m = MeshArrays::default();
            for f in &faces {
                push_polygon(
                    &mut m,
                    f,
                    [1.0; 3],
                    &crate::textures::tile_rect("plain"),
                    [1.0, 0.0],
                    1.0,
                );
            }
            // Each face's normal points away from the shape's middle.
            let mut at = 0;
            for f in &faces {
                let c: [f64; 3] =
                    std::array::from_fn(|k| f.iter().map(|p| p[k]).sum::<f64>() / f.len() as f64);
                let n = &m.normals[at * 3..at * 3 + 3];
                let out = [c[0], c[1] - size[1] * 0.4, c[2]];
                let dot: f64 = (0..3).map(|k| f64::from(n[k]) * out[k]).sum();
                assert!(dot > 0.0, "{shape:?} face {f:?} normal {n:?}");
                at += f.len();
            }
        }
    }

    fn look(item: &str) -> dwell_protocol::inventory::ItemLook {
        dwell_protocol::inventory::item(item)
            .and_then(|d| d.look)
            .expect(item)
    }

    #[test]
    fn a_model_look_is_its_props_mesh_lying_with_its_colours() {
        // both-faces: item-local geometry; the loose item mirrors it with its face.
        let l = look("dwell:stone_pickaxe");
        assert_eq!(l.shape, GroundShape::Model);
        let (data, prop) = dwell_models::prop(l.model).expect("built in");
        let m = look_mesh(&l);
        assert_eq!(
            m.indices.len(),
            prop.indices.len(),
            "every triangle of the prop"
        );
        // Lying: its length along x, its base on y = 0.
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in m.positions.chunks_exact(3) {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        assert!((hi[0] - lo[0] - data.length).abs() < 0.02, "{lo:?} {hi:?}");
        assert!(lo[1].abs() < 1e-4 && hi[1] < data.length / 2.0);
        // More than one colour: the prop's palette colours, not one tint.
        let mut cs: Vec<[u8; 3]> = m
            .colors
            .chunks_exact(3)
            .map(|c| [c[0], c[1], c[2]].map(|v| (v * 64.0) as u8))
            .collect();
        cs.sort_unstable();
        cs.dedup();
        assert!(cs.len() > 2, "{cs:?}");
    }

    #[test]
    fn a_model_looks_swatch_is_drawn_in_its_colours() {
        let px = look_swatch(&look("dwell:stone_pickaxe"));
        let mut cs: Vec<[u8; 3]> = px
            .chunks_exact(4)
            .filter(|p| p[3] == 255)
            .map(|p| [p[0] >> 4, p[1] >> 4, p[2] >> 4])
            .collect();
        let drawn = cs.len();
        cs.sort_unstable();
        cs.dedup();
        assert!(drawn > px.len() / 4 / 20, "{drawn} pixels drawn");
        assert!(cs.len() > 3, "{} colours", cs.len());
    }

    #[test]
    fn an_items_look_mesh_is_its_shape_at_its_size_closed_all_round() {
        // both-faces: item-local geometry; the loose item mirrors it with its face.
        for (item, shape, faces) in [
            ("dwell:stick", GroundShape::Rod, 6),
            ("dwell:rock", GroundShape::Lump, 14),
        ] {
            let l = look(item);
            assert_eq!(l.shape, shape, "{item}");
            let m = look_mesh(&l);
            // Every side, the underside too (a loose item spins in the air).
            let triangles = m.indices.len() / 3;
            let quads = if shape == GroundShape::Rod {
                faces * 2
            } else {
                12 * 2 + 4 * 2
            };
            assert_eq!(triangles, quads, "{item}");
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for p in m.positions.chunks_exact(3) {
                for k in 0..3 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
            // In metres, its base at 0: as tall as its size, as long as its size (a rod).
            assert!(
                lo[1].abs() < 1e-6 && (f64::from(hi[1]) - l.size[1]).abs() < 1e-6,
                "{item}"
            );
            if shape == GroundShape::Rod {
                assert!(
                    (f64::from(hi[0] - lo[0]) - l.size[0]).abs() < 1e-6,
                    "{item}"
                );
            }
            // Tinted its look's colour (times the face shading), on its texture's tile.
            let tile = crate::textures::tile_rect(look_tile(&l));
            assert!((f64::from(m.tiles[0]) - tile.u0).abs() < 1e-6, "{item}");
        }
    }

    #[test]
    fn the_grass_seed_draws_a_cluster_of_seeds_not_a_cube() {
        // both-faces: item-local geometry; the loose item and the ground mirror it with its face.
        let l = look("dwell:grass_seed");
        assert_eq!(l.shape, GroundShape::Cluster);
        let m = look_mesh(&l);
        // Six lumps, each closed all round (12 side quads, a hexagon top and base: 32 triangles).
        assert_eq!(m.indices.len() / 3, 6 * 32);
        // Its base at 0, no taller than its size, and in its footprint.
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in m.positions.chunks_exact(3) {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        assert!(lo[1].abs() < 1e-6 && f64::from(hi[1]) <= l.size[1] + 1e-6);
        for k in [0, 2] {
            assert!(f64::from(hi[k] - lo[k]) <= l.size[k] + 1e-6, "axis {k}");
        }
        // The picture is the heap: drawn, with gaps (not a filled square's outline).
        let px = look_swatch(&l);
        let opaque = px.chunks_exact(4).filter(|p| p[3] == 255).count();
        let t = crate::textures::TILE;
        assert!(opaque > t * 2 && opaque < t * t * 3 / 4, "{opaque}");
    }

    #[test]
    fn a_cluster_look_given_by_data_alone_draws_a_heap() {
        // both-faces: item-local geometry. A fixture look, not the shipped data.
        let l = dwell_protocol::inventory::ItemLook {
            shape: GroundShape::Cluster,
            size: [0.2, 0.06, 0.2],
            texture: "dwell:sand",
            color: [200, 190, 90],
            model: "",
        };
        let faces = polygons(l.shape, l.size, false);
        // Five on the ground with no base, the top one with its base: 5 × 13 + 14.
        assert_eq!(faces.len(), 5 * 13 + 14);
        assert!(!look_mesh(&l).indices.is_empty());
    }

    #[test]
    fn an_items_picture_draws_its_shape_not_a_square() {
        // face-a-only: a picture, no world geometry.
        use crate::textures::TILE;
        for item in ["dwell:stick", "dwell:rock"] {
            let px = look_swatch(&look(item));
            assert_eq!(px.len(), TILE * TILE * 4);
            let opaque = px.chunks_exact(4).filter(|p| p[3] == 255).count();
            // Clear around it (the corners), and drawn: a rod a thin diagonal, a rock a lump.
            for (x, y) in [(0, 0), (TILE - 1, 0), (0, TILE - 1), (TILE - 1, TILE - 1)] {
                assert_eq!(px[(y * TILE + x) * 4 + 3], 0, "{item} corner {x},{y}");
            }
            assert!(
                opaque > TILE * 2 && opaque < TILE * TILE * 3 / 4,
                "{item}: {opaque}"
            );
        }
        // The stick's grain is its log's bark, tinted: brown, red above blue.
        let px = look_swatch(&look("dwell:stick"));
        let lit: Vec<&[u8]> = px.chunks_exact(4).filter(|p| p[3] == 255).collect();
        let r: u32 = lit.iter().map(|p| u32::from(p[0])).sum();
        let b: u32 = lit.iter().map(|p| u32::from(p[2])).sum();
        assert!(r > b, "a stick reads brown");
    }
}
