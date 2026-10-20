//! Meshes from parts (CHARACTERS.md §9.1): each part a rigid mesh on its bone, built at its own size
//! (a rounded box keeps round corners, a capsule round ends), placed in its bone's frame. A look in
//! one view is one mesh: the renderer moves each vertex by its bone's transform, so a character draws
//! in one call with no weights and no skinning.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use crate::math::Affine;
use crate::types::{Kind, Part, View};

/// Segments around a round part, and rings over a sphere: chunky shapes read at these counts.
const AROUND: u32 = 16;
const RINGS: u32 = 10;
const CAPSULE_RINGS: u32 = 4;

/// A mesh in bone frames: per vertex its position and normal in its bone's frame, the bone, the
/// material, a texture coordinate on its side (0–1, for `$block`) and which side it is (0 +x, 1 −x,
/// 2 +y, 3 −y, 4 +z, 5 −z, in the part's own frame: the held block's top, bottom and sides).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub bones: Vec<u8>,
    pub materials: Vec<u16>,
    pub faces: Vec<u8>,
    /// Triangles, counter-clockwise seen from outside.
    pub indices: Vec<u32>,
    /// The parts drawn, in order.
    pub parts: Vec<&'static str>,
}

impl Mesh {
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }
    /// Bytes as the renderer uploads it (`VERTEX_BYTES` a vertex, 4 an index).
    pub fn byte_len(&self) -> usize {
        self.positions.len() * VERTEX_BYTES + self.indices.len() * 4
    }
}

/// Bytes per vertex on the GPU: position, normal, uv, and bone | face | material in one u32.
pub const VERTEX_BYTES: usize = 4 * (3 + 3 + 2 + 1);

/// Whether a view draws a part: both views draw `both`; the arms view only the arm chain's bones.
pub fn drawn(p: &Part, view: View, chain: &[usize]) -> bool {
    let in_view = p.view == View::Both || p.view == view;
    in_view && (view != View::Arms || chain.contains(&p.bone))
}

/// The mesh of `parts` as `view` draws it (`chain`: the first-person arm chain, for the arms view).
pub fn build(parts: &[Part], view: View, chain: &[usize]) -> Mesh {
    let mut m = Mesh::default();
    for p in parts.iter().filter(|p| drawn(p, view, chain)) {
        add_part(&mut m, p);
    }
    m
}

/// A shape in its part's frame (centred, filling its size).
#[derive(Default)]
struct Shape {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    indices: Vec<u32>,
}

impl Shape {
    fn vertex(&mut self, p: [f32; 3], n: [f32; 3]) -> u32 {
        self.positions.push(p);
        self.normals.push(normalize(n));
        (self.positions.len() - 1) as u32
    }
    /// A grid of `rows` × `cols` vertices, joined into quads.
    fn grid(&mut self, first: u32, rows: u32, cols: u32) {
        for r in 0..rows - 1 {
            for c in 0..cols - 1 {
                let a = first + r * cols + c;
                let (b, d, e) = (a + 1, a + cols, a + cols + 1);
                self.indices.extend([a, b, e, a, e, d]);
            }
        }
    }
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l < 1e-12 {
        [0.0, 1.0, 0.0]
    } else {
        v.map(|c| c / l)
    }
}

fn add_part(m: &mut Mesh, p: &Part) {
    let mut s = match p.kind {
        Kind::Box => rounded_box(p.size, 0.0),
        Kind::RoundBox => rounded_box(p.size, p.round),
        Kind::Ellipsoid => ellipsoid(p.size),
        Kind::Cylinder => frustum(p.size, 1.0),
        Kind::Cone => frustum(p.size, p.tip),
        Kind::Capsule => capsule(p.size),
    };
    orient_outward(&mut s);
    let place = Affine::translation(p.at).then(&Affine::euler(p.rot));
    let base = m.positions.len() as u32;
    for (pos, n) in s.positions.iter().zip(&s.normals) {
        // The side (in the part's frame) and a coordinate across it.
        let a = n.map(f32::abs);
        let axis = if a[0] >= a[1] && a[0] >= a[2] {
            0
        } else if a[1] >= a[2] {
            1
        } else {
            2
        };
        let face = (axis * 2 + usize::from(n[axis] < 0.0)) as u8;
        let rel = |k: usize| pos[k] / p.size[k].max(1e-6) + 0.5;
        let uv = match axis {
            0 => [if n[0] > 0.0 { 1.0 - rel(2) } else { rel(2) }, rel(1)],
            1 => [rel(0), if n[1] > 0.0 { 1.0 - rel(2) } else { rel(2) }],
            _ => [if n[2] > 0.0 { rel(0) } else { 1.0 - rel(0) }, rel(1)],
        };
        m.positions.push(place.apply(*pos));
        m.normals.push(normalize(place.rotate(*n)));
        m.uvs.push(uv);
        m.bones.push(p.bone as u8);
        m.materials.push(p.material);
        m.faces.push(face);
    }
    m.indices.extend(s.indices.iter().map(|i| i + base));
    m.parts.push(p.id);
}

/// Every triangle turned to face along its vertices' normals (outward), whatever order a shape
/// listed its corners in.
fn orient_outward(s: &mut Shape) {
    for t in s.indices.chunks_mut(3) {
        let [a, b, c] = [t[0], t[1], t[2]].map(|i| s.positions[i as usize]);
        let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ];
        let vn = [t[0], t[1], t[2]]
            .map(|i| s.normals[i as usize])
            .into_iter()
            .fold([0.0; 3], |acc, v| {
                [acc[0] + v[0], acc[1] + v[1], acc[2] + v[2]]
            });
        if n[0] * vn[0] + n[1] * vn[1] + n[2] * vn[2] < 0.0 {
            t.swap(1, 2);
        }
    }
}

/// A box of `size`, its edges rounded by `round` of its smallest side (0: sharp). Each face is a
/// grid whose outer rows are pushed onto the rounding (the cube-sphere construction).
fn rounded_box(size: [f32; 3], round: f32) -> Shape {
    let h = size.map(|s| s / 2.0);
    let smallest = size.iter().copied().fold(f32::MAX, f32::min);
    let r = (round.clamp(0.0, 0.5) * smallest)
        .min(smallest / 2.0 - 1e-4)
        .max(0.0);
    let mut s = Shape::default();
    // Per axis, the face's sample coordinates: the corners, and with rounding two rows into the curve.
    let coords = |k: usize| -> Vec<f32> {
        if r < 1e-4 {
            vec![-h[k], h[k]]
        } else {
            let inner = h[k] - r;
            vec![
                -h[k],
                -inner - r * 0.5,
                -inner,
                inner,
                inner + r * 0.5,
                h[k],
            ]
        }
    };
    for axis in 0..3 {
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        for sign in [1.0f32, -1.0] {
            let (cu, cv) = (coords(u), coords(v));
            let first = s.positions.len() as u32;
            for &b in &cv {
                for &a in &cu {
                    let mut p = [0.0; 3];
                    p[axis] = sign * h[axis];
                    p[u] = a;
                    p[v] = b;
                    if r < 1e-4 {
                        let mut n = [0.0; 3];
                        n[axis] = sign;
                        s.vertex(p, n);
                    } else {
                        let inner: [f32; 3] =
                            std::array::from_fn(|k| p[k].clamp(-(h[k] - r), h[k] - r));
                        let d = normalize([p[0] - inner[0], p[1] - inner[1], p[2] - inner[2]]);
                        s.vertex(std::array::from_fn(|k| inner[k] + d[k] * r), d);
                    }
                }
            }
            s.grid(first, cv.len() as u32, cu.len() as u32);
        }
    }
    s
}

/// A sphere scaled to fill `size`.
fn ellipsoid(size: [f32; 3]) -> Shape {
    let h = size.map(|s| s / 2.0);
    let mut s = Shape::default();
    for ring in 0..=RINGS {
        let phi = PI * ring as f32 / RINGS as f32;
        for seg in 0..=AROUND {
            let theta = TAU * seg as f32 / AROUND as f32;
            let unit = [phi.sin() * theta.sin(), -phi.cos(), phi.sin() * theta.cos()];
            let p = std::array::from_fn(|k| unit[k] * h[k]);
            let n = std::array::from_fn(|k| unit[k] / h[k].max(1e-6));
            s.vertex(p, n);
        }
    }
    s.grid(0, RINGS + 1, AROUND + 1);
    s
}

/// A cylinder (`tip` 1) or cone (`tip` the top radius, 0 a point) filling `size`, axis along y, with
/// its caps.
fn frustum(size: [f32; 3], tip: f32) -> Shape {
    let h = size.map(|s| s / 2.0);
    let (r0, r1) = (1.0f32, tip.clamp(0.0, 1.0));
    let mut s = Shape::default();
    // The side, in unit radius then scaled: its slope gives the normal's y.
    let slope = (r0 - r1) * h[0].min(h[2]) / size[1].max(1e-6);
    let first = s.positions.len() as u32;
    for (y, r) in [(-h[1], r0), (h[1], r1)] {
        for seg in 0..=AROUND {
            let t = TAU * seg as f32 / AROUND as f32;
            let (sin, cos) = t.sin_cos();
            let n = [
                sin / h[0].max(1e-6),
                slope / h[0].min(h[2]).max(1e-6),
                cos / h[2].max(1e-6),
            ];
            s.vertex([sin * r * h[0], y, cos * r * h[2]], n);
        }
    }
    s.grid(first, 2, AROUND + 1);
    for (y, r, ny) in [(-h[1], r0, -1.0), (h[1], r1, 1.0)] {
        if r <= 1e-4 {
            continue;
        }
        let centre = s.vertex([0.0, y, 0.0], [0.0, ny, 0.0]);
        for seg in 0..=AROUND {
            let t = TAU * seg as f32 / AROUND as f32;
            let (sin, cos) = t.sin_cos();
            s.vertex([sin * r * h[0], y, cos * r * h[2]], [0.0, ny, 0.0]);
        }
        for seg in 0..AROUND {
            s.indices
                .extend([centre, centre + 1 + seg, centre + 2 + seg]);
        }
    }
    s
}

/// A capsule as the Lab builds it: radius x/2, total height y, scaled along z to z/x.
fn capsule(size: [f32; 3]) -> Shape {
    let r = size[0] / 2.0;
    let half = ((size[1] - size[0]) / 2.0).max(0.0005);
    let zs = size[2] / size[0].max(1e-6);
    let mut s = Shape::default();
    let mut profile = Vec::new();
    for i in 0..=CAPSULE_RINGS {
        let a = -FRAC_PI_2 + FRAC_PI_2 * i as f32 / CAPSULE_RINGS as f32;
        profile.push((r * a.cos(), -half + r * a.sin(), a.cos(), a.sin()));
    }
    for i in 0..=CAPSULE_RINGS {
        let a = FRAC_PI_2 * i as f32 / CAPSULE_RINGS as f32;
        profile.push((r * a.cos(), half + r * a.sin(), a.cos(), a.sin()));
    }
    for &(pr, y, nr, ny) in &profile {
        for seg in 0..=AROUND {
            let t = TAU * seg as f32 / AROUND as f32;
            let (sin, cos) = t.sin_cos();
            s.vertex([pr * sin, y, pr * cos * zs], [nr * sin, ny, nr * cos / zs]);
        }
    }
    s.grid(0, profile.len() as u32, AROUND + 1);
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::{Pose, bone_transforms};
    use crate::{DEFAULT_LOOK, FIXTURE, PLAYER, TABLES};

    fn bounds(m: &Mesh, ch: &crate::Character) -> ([f32; 3], [f32; 3]) {
        let t = bone_transforms(ch, &Pose::REST);
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for (p, &b) in m.positions.iter().zip(&m.bones) {
            let w = t[usize::from(b)].apply(*p);
            for k in 0..3 {
                lo[k] = lo[k].min(w[k]);
                hi[k] = hi[k].max(w[k]);
            }
        }
        (lo, hi)
    }

    fn wound_outward(m: &Mesh) -> bool {
        m.indices.chunks(3).all(|t| {
            let [a, b, c] = [t[0], t[1], t[2]].map(|i| m.positions[i as usize]);
            let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let n = [
                e1[1] * e2[2] - e1[2] * e2[1],
                e1[2] * e2[0] - e1[0] * e2[2],
                e1[0] * e2[1] - e1[1] * e2[0],
            ];
            let area = n.iter().map(|x| x * x).sum::<f32>();
            let vn = m.normals[t[0] as usize];
            area < 1e-14 || n[0] * vn[0] + n[1] * vn[1] + n[2] * vn[2] > -1e-9
        })
    }

    #[test]
    fn every_kind_fills_its_size_box_wound_outward() {
        for kind in [
            Kind::Box,
            Kind::RoundBox,
            Kind::Ellipsoid,
            Kind::Cylinder,
            Kind::Cone,
            Kind::Capsule,
        ] {
            let p = Part {
                id: "p",
                bone: 0,
                kind,
                size: [0.2, 0.5, 0.3],
                at: [0.0; 3],
                rot: [0.0; 3],
                round: 0.3,
                tip: 0.4,
                view: View::Both,
                material: 0,
            };
            let m = build(&[p], View::Body, &[]);
            let mut lo = [f32::MAX; 3];
            let mut hi = [f32::MIN; 3];
            for v in &m.positions {
                for k in 0..3 {
                    lo[k] = lo[k].min(v[k]);
                    hi[k] = hi[k].max(v[k]);
                }
            }
            for k in 0..3 {
                assert!(
                    (hi[k] - lo[k] - p.size[k]).abs() < 2e-3,
                    "{kind:?} {k}: {lo:?} {hi:?}"
                );
            }
            assert!(wound_outward(&m), "{kind:?}");
            assert!(m.indices.iter().all(|&i| (i as usize) < m.vertex_count()));
        }
    }

    #[test]
    fn the_players_body_mesh_stands_its_height_and_the_arms_view_only_the_arm_chain() {
        let ch = TABLES.character(PLAYER).unwrap();
        let look = ch.look(DEFAULT_LOOK).unwrap();
        let body = build(look.parts, View::Body, &[]);
        let (lo, hi) = bounds(&body, ch);
        assert!(lo[1].abs() < 0.03, "feet on the ground: {lo:?}");
        // With the wide hat on, taller than the bare body.
        assert!(hi[1] > ch.height - 0.03, "{hi:?}");
        let fp = ch.first_person.unwrap();
        let arms = build(look.parts, View::Arms, fp.bones);
        assert!(!arms.parts.is_empty());
        assert!(
            arms.bones
                .iter()
                .all(|b| fp.bones.contains(&usize::from(*b)))
        );
        assert!(arms.parts.len() < body.parts.len());
        // Every body part on the chain is in the arms view (CHARACTERS.md §5).
        for p in look
            .parts
            .iter()
            .filter(|p| fp.bones.contains(&p.bone) && p.view == View::Both)
        {
            assert!(arms.parts.contains(&p.id), "{}", p.id);
        }
    }

    /// The loader turns data into meshes: a part added to the data alone (the fixture's pauldron,
    /// shared/characters/fixtures) is in the fixture's tables, meshed on its bone (and its mirror's),
    /// and widens the body; the build's tables don't have it.
    #[test]
    fn a_part_added_by_data_alone_is_meshed_on_its_bone() {
        let build_ch = TABLES.character(PLAYER).unwrap();
        let fx_ch = FIXTURE.character(PLAYER).unwrap();
        let plain = build(build_ch.look(DEFAULT_LOOK).unwrap().parts, View::Body, &[]);
        let fx = build(fx_ch.look(DEFAULT_LOOK).unwrap().parts, View::Body, &[]);
        assert!(!plain.parts.contains(&"test_pauldron_l"));
        assert!(fx.parts.contains(&"test_pauldron_l") && fx.parts.contains(&"test_pauldron_r"));
        assert!(fx.vertex_count() > plain.vertex_count());
        let arm_l = fx_ch.bone("arm_l").unwrap() as u8;
        let on_arm = |m: &Mesh| m.bones.iter().filter(|&&b| b == arm_l).count();
        assert!(on_arm(&fx) > on_arm(&plain));
        let (lo, hi) = bounds(&fx, fx_ch);
        let (plo, phi) = bounds(&plain, build_ch);
        assert!(
            hi[0] > phi[0] + 0.02 && lo[0] < plo[0] - 0.02,
            "{lo:?} {hi:?} vs {plo:?} {phi:?}"
        );
        // The pauldron paints with its material, which the fixture's table has.
        let iron = FIXTURE
            .materials
            .iter()
            .position(|m| m.name == "metal_iron")
            .unwrap() as u16;
        assert!(fx.materials.contains(&iron));
    }

    #[test]
    fn the_held_block_is_a_block_material_box_with_top_and_side_faces() {
        let ch = crate::player();
        let item = ch.item("block").unwrap();
        let m = build(item.parts, View::Body, &[]);
        assert!(m.materials.iter().all(|&x| x == crate::BLOCK_MATERIAL));
        assert_eq!(m.vertex_count(), 24);
        for f in 0..6u8 {
            assert!(m.faces.contains(&f));
        }
        assert!(
            m.uvs
                .iter()
                .all(|uv| (0.0..=1.0).contains(&uv[0]) && (0.0..=1.0).contains(&uv[1]))
        );
    }
}
