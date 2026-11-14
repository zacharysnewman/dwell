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
use relief::{Carve, Dent, Facets, Flake, Shard, Spall};

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
            chunk: m.chunk,
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
pub(crate) fn plane_axes(axis: usize) -> (usize, usize) {
    ((axis + 1) % 3, (axis + 2) % 3)
}

/// The height field of a face's marks at damage `fraction`, the look's own size grown with it
/// (`relief::Carve`; the shape alone: [`carve_for`] sizes it to the bite rule's volume).
#[cfg(test)]
pub(crate) fn carve_of(face: usize, marks: &[&HitMark], look: &DamageLook, fraction: f64) -> Carve {
    let grow = 0.55 + 0.45 * fraction;
    carve_sized(face, marks, look, grow, grow, 0.45)
}

/// How deep a carve may go, as a share of the cell along the face's normal.
pub(crate) const DEEPEST: f64 = 0.9;

/// The volume (m³) a face's carve takes out: its height field integrated over the face.
pub(crate) fn carve_volume(face: usize, carve: &Carve) -> f64 {
    const S: usize = 64;
    let (u, v) = plane_axes(face / 2);
    let (eu, ev) = (ext(u), ext(v));
    let (du, dv) = (eu / S as f64, ev / S as f64);
    let mut sum = 0.0;
    for i in 0..S {
        for j in 0..S {
            sum -= carve.height([(i as f64 + 0.5) * du, (j as f64 + 0.5) * dv]);
        }
    }
    sum * du * dv
}

/// The height field of a face's marks carving exactly `target_m3` (the bite rule: its share of
/// [`dwell_protocol::damage::carved_m3`]; WORK_QUEUE item 18, BLOCK_DAMAGE.md §10): the look's
/// shape (its `size`, `depth`, profile), deepened to the volume, and once as deep as
/// [`DEEPEST`] allows, widened.
pub(crate) fn carve_for(
    face: usize,
    marks: &[&HitMark],
    look: &DamageLook,
    target_m3: f64,
) -> Carve {
    carve_fit(
        target_m3,
        DEEPEST * ext(face / 2),
        look,
        |r, d| carve_sized(face, marks, look, r, d, DEEPEST),
        |c| carve_volume(face, c),
    )
}

/// The carve `build(radius ×, depth ×)` sized so `volume` of it is `target_m3`: deepened, and
/// once its depth reaches `cap` (m), widened (a face's worth at most). Shared by a cube's faces
/// and a ramp's (`ramp_damage`).
pub(crate) fn carve_fit(
    target_m3: f64,
    cap: f64,
    look: &DamageLook,
    build: impl Fn(f64, f64) -> Carve,
    volume: impl Fn(&Carve) -> f64,
) -> Carve {
    let (mut r, mut d) = (1.0, 1.0);
    if target_m3 <= 0.0 {
        return build(r, 0.0);
    }
    let mut carve = build(r, d);
    for _ in 0..24 {
        let got = volume(&carve);
        if got <= 0.0 {
            break;
        }
        let ratio = target_m3 / got;
        if (ratio - 1.0).abs() < 0.01 {
            break;
        }
        if ratio > 1.0 && carve.depth() >= cap * 0.999 {
            // As deep as it goes: wider (a face's worth at most).
            r = (r * ratio.sqrt()).min(4.0 / look.size);
        } else {
            d *= ratio;
        }
        carve = build(r, d);
    }
    carve
}

/// The height field of a face's marks with the look's radius × `rmul` and depth × `dmul`, each mark
/// weighed by its chunk (its bite against the others'), no deeper than `deepest` of the cell.
fn carve_sized(
    face: usize,
    marks: &[&HitMark],
    look: &DamageLook,
    rmul: f64,
    dmul: f64,
    deepest: f64,
) -> Carve {
    let axis = face / 2;
    let (u, v) = plane_axes(axis);
    let (eu, ev) = (ext(u), ext(v));
    let at: Vec<[f64; 2]> = marks
        .iter()
        .map(|m| {
            [
                f64::from(m.point[u]) / 255.0 * eu,
                f64::from(m.point[v]) / 255.0 * ev,
            ]
        })
        .collect();
    carve_shape([eu, ev], &at, marks, look, rmul, dmul, deepest * ext(axis))
}

/// The height field of `marks` struck at `at` (face metres) on a face `dims` (m) across, with the
/// look's radius × `rmul` and depth × `dmul`, each mark weighed by its chunk, no deeper than
/// `deepest` (m): a cube's face's ([`carve_sized`]) or a ramp polygon's (`ramp_damage`).
pub(crate) fn carve_shape(
    dims: [f64; 2],
    at: &[[f64; 2]],
    marks: &[&HitMark],
    look: &DamageLook,
    rmul: f64,
    dmul: f64,
    deepest: f64,
) -> Carve {
    let [eu, ev] = dims;
    let radius = 0.5 * look.size * rmul;
    let depth = look.depth * dmul;
    // Each mark scaled by its chunk (its bite against the others', `HitMark::chunk`), its depth
    // kept inside the cell.
    let mean =
        marks.iter().take(4).map(|m| m.scale()).sum::<f64>() / marks.len().clamp(1, 4) as f64;
    let rel = |m: &HitMark| m.scale() / mean.max(1e-6);
    let sized = |m: &HitMark| (radius * rel(m), (depth * rel(m)).min(deepest));
    // The soils' merged hollow has one radius: its biggest mark's.
    let big = marks
        .iter()
        .take(4)
        .map(|m| rel(m))
        .fold(0.0, f64::max)
        .max(1.0);
    let (bowl_r, bowl_d) = (radius * big, (depth * big).min(deepest));
    // Mark `i`'s centre on the face (m), kept far enough in for its dent to fit the face.
    let centre = |i: usize, keep: f64| -> [f64; 2] {
        let p = at.get(i).copied().unwrap_or([0.5 * eu, 0.5 * ev]);
        let at = |x: f64, e: f64| {
            let k = keep.min(0.5 * e);
            x.clamp(k, e - k)
        };
        [at(p[0], eu), at(p[1], ev)]
    };
    let (lo, hi) = ([0.0, 0.0], [eu, ev]);
    match look.profile {
        CarveProfile::Bowl => Carve::Bowl(Dent {
            centres: (0..marks.len().min(4)).map(|i| centre(i, bowl_r)).collect(),
            lo,
            hi,
            radius: bowl_r,
            depth: bowl_d,
            // A carve only cuts: the loose rim was the shader relief's.
            rim: 0.0,
        }),
        CarveProfile::Facets => Carve::Facets(Facets {
            shards: marks
                .iter()
                .enumerate()
                .take(4)
                .map(|(i, m)| {
                    let (r, d) = sized(m);
                    shard(m, i, look.facets.max(3), r, d, centre(i, r))
                })
                .collect(),
            lo,
            hi,
        }),
        CarveProfile::Spall => Carve::Spall(Spall {
            flakes: marks
                .iter()
                .enumerate()
                .take(4)
                .flat_map(|(i, m)| {
                    let (r, d) = sized(m);
                    flakes(m, i, look.flakes.max(1), r, d, centre(i, r))
                })
                .collect(),
            lo,
            hi,
            fresh: 1.0,
        }),
    }
}

/// A mark's stable jitter in [0, 1) by its point and order, so a remesh carves it the same.
fn jitter_of(m: &HitMark, index: usize) -> impl Fn(u32) -> f64 {
    let seed = (u32::from(m.point[0]).wrapping_mul(73_856_093))
        ^ (u32::from(m.point[1]).wrapping_mul(19_349_663))
        ^ (u32::from(m.point[2]).wrapping_mul(83_492_791))
        ^ (index as u32).wrapping_mul(2_654_435_761);
    move |k: u32| {
        let h = (seed ^ k.wrapping_mul(0x9E37_79B9)).wrapping_mul(0x85EB_CA6B);
        let h = (h ^ (h >> 13)).wrapping_mul(0xC2B2_AE35);
        f64::from(h >> 8) / f64::from(1u32 << 24)
    }
}

/// The convex hull of `pts`, counter-clockwise (Andrew's monotone chain).
pub(crate) fn hull(mut pts: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    pts.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    let cross = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    let mut out: Vec<[f64; 2]> = Vec::new();
    for pass in 0..2 {
        let start = out.len();
        let it: Box<dyn Iterator<Item = &[f64; 2]>> = if pass == 0 {
            Box::new(pts.iter())
        } else {
            Box::new(pts.iter().rev())
        };
        for &p in it {
            while out.len() >= start + 2
                && cross(out[out.len() - 2], out[out.len() - 1], p) <= 1e-12
            {
                out.pop();
            }
            out.push(p);
        }
        out.pop();
    }
    out
}

/// A hit's spalled flakes round its strike `c` (face metres; WORK_QUEUE item 18): `count` flat
/// scars, the first the widest and shallowest, each next smaller, turned a little and deeper (a
/// stepped scar), all elongated about 3:1 along a direction of the hit's own and running out from
/// the strike (deepest there, feathering toward the far end).
fn flakes(m: &HitMark, index: usize, count: u32, r: f64, depth: f64, c: [f64; 2]) -> Vec<Flake> {
    let jitter = jitter_of(m, index);
    let dir = jitter(99) * std::f64::consts::TAU;
    (0..count)
        .map(|k| {
            let kf = f64::from(k);
            let scale = 0.7f64.powi(k as i32);
            let th = dir
                + if k == 0 {
                    0.0
                } else {
                    (jitter(40 + k) - 0.5) * 0.7
                };
            let (s, co) = th.sin_cos();
            let a = r * scale * (0.85 + 0.15 * jitter(10 + k));
            let b = a / 2.8;
            // The flake's middle lies out from the strike along its length.
            let mid = [c[0] + co * 0.45 * a, c[1] + s * 0.45 * a];
            let corners = hull(
                (0..7u32)
                    .map(|j| {
                        let t = f64::from(j) * std::f64::consts::TAU / 7.0
                            + (jitter(100 + 10 * k + j) - 0.5) * 0.5;
                        let rr = 0.8 + 0.2 * jitter(200 + 10 * k + j);
                        let (lx, ly) = (a * rr * t.cos(), b * rr * t.sin());
                        [mid[0] + lx * co - ly * s, mid[1] + lx * s + ly * co]
                    })
                    .collect(),
            );
            // Deep at the strike, shallower toward the far end (1.45 a out).
            // The last (deepest) flake reaches the full depth; a single flake is it.
            let step = if count == 1 {
                1.0
            } else {
                kf / f64::from(count - 1)
            };
            // Behind the strike the floor runs 0.55 a on, deepening ×1.27: the deepest is `depth`.
            let near = depth * (0.55 + 0.45 * step) / 1.27;
            let far = near * 0.3;
            let g = (near - far) / (1.45 * a);
            Flake {
                corners,
                origin: c,
                depth: near,
                grad: [-co * g, -s * g],
                bevel: 0.004,
            }
        })
        .collect()
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

/// A damaged cell's crack overlay on one face (BLOCK_DAMAGE.md §6.3): the crack stage's atlas
/// tile centred on the hit nearest the face, mirrored per cell so neighbours differ. Each vertex
/// carries it packed in four bytes (`MeshArrays::cracks`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CrackLayer {
    /// The stage's atlas slot + 1 (0: none; `TILE_ORDER` index + 1).
    pub slot: u8,
    /// How far it darkens the face (0–255).
    pub strength: u8,
    /// The hit the cracks spread from, in the cell (m, face-local axes).
    pub centre: [f64; 3],
    /// The width the tile covers round the hit (m).
    pub span: f64,
    /// The tile mirrored across its s and t, per cell.
    pub flip: [bool; 2],
}

/// The crack tile's (s, t) axes on a face: a side's t runs up (the grain of a standing log), a
/// top's along z; s across.
pub(crate) fn crack_axes(face: usize) -> (usize, usize) {
    match face / 2 {
        0 => (2, 1),
        1 => (0, 2),
        _ => (0, 1),
    }
}

/// The crack tile's coordinates are packed as unorm bytes over [−½, 1½] of the tile (a vertex at
/// a cell's far corner from a hit lies past the tile's edge; the shader draws no crack there).
pub const CRACK_UV_MIN: f64 = -0.5;
pub const CRACK_UV_RANGE: f64 = 2.0;

impl CrackLayer {
    /// The cell's crack layer at its damage (`view`, face-local), or none below the first stage or
    /// for a look without cracks; the centre is chosen per face by [`CrackLayer::for_face`].
    pub(crate) fn of(look: &DamageLook, view: &DamageView, cell: [i32; 3]) -> Option<CrackLayer> {
        Self::of_cracks(&look.cracks?, f64::from(view.fraction) / 255.0, cell)
    }

    /// The layer of crack data `cracks` at damage `fraction` (0–1) in `cell`, centred on the
    /// cell's middle until [`CrackLayer::for_face`] or a caller moves it (a cut wood's: its newest
    /// bite, WORK_QUEUE item 26).
    pub(crate) fn of_cracks(
        cracks: &dwell_protocol::damage::Cracks,
        fraction: f64,
        cell: [i32; 3],
    ) -> Option<CrackLayer> {
        let stage = cracks.stage(fraction)?;
        let tile = cracks.tiles[stage];
        let slot = crate::textures::TILE_ORDER
            .iter()
            .position(|t| *t == tile)?
            + 1;
        let h = (cell[0].wrapping_mul(73_856_093)
            ^ cell[1].wrapping_mul(19_349_663)
            ^ cell[2].wrapping_mul(83_492_791)) as u32;
        let h = h.wrapping_mul(0x9E37_79B9) >> 16;
        Some(CrackLayer {
            slot: u8::try_from(slot).ok()?,
            strength: (cracks.strength.clamp(0.0, 1.0) * 255.0).round() as u8,
            centre: [0.5, 0.5 * CELL_H, 0.5],
            span: cracks.span,
            flip: [h & 1 == 1, h & 2 == 2],
        })
    }

    /// The layer on `face`: centred on the newest hit on it, else the hit nearest its plane (the
    /// cracks run round the corner from the struck face onto this one).
    pub(crate) fn for_face(mut self, face: usize, marks: &[HitMark]) -> CrackLayer {
        let metres = |m: &HitMark| -> [f64; 3] {
            std::array::from_fn(|a| f64::from(m.point[a]) / 255.0 * ext(a))
        };
        let axis = face / 2;
        let plane = if face.is_multiple_of(2) {
            ext(axis)
        } else {
            0.0
        };
        let best = marks
            .iter()
            .rev()
            .find(|m| usize::from(m.face) == face)
            .or_else(|| {
                marks.iter().rev().min_by(|a, b| {
                    let d = |m: &HitMark| (metres(m)[axis] - plane).abs();
                    d(a).total_cmp(&d(b))
                })
            });
        if let Some(m) = best {
            self.centre = metres(m);
        }
        self
    }

    /// The crack tile's (s, t) at a point of `face` (face metres, its `plane_axes`), before packing.
    pub(crate) fn uv(&self, face: usize, q: [f64; 2]) -> [f64; 2] {
        let (u, v) = plane_axes(face / 2);
        let mut p = self.centre;
        p[u] = q[0];
        p[v] = q[1];
        let (s, t) = crack_axes(face);
        let at = |a: usize, flip: bool| {
            let d = (p[a] - self.centre[a]) / self.span;
            0.5 + if flip { -d } else { d }
        };
        [at(s, self.flip[0]), at(t, self.flip[1])]
    }

    /// The four bytes a vertex carries (s, t, slot, strength; little-endian, the GPU's unorm8x4).
    pub(crate) fn pack(&self, face: usize, q: [f64; 2]) -> u32 {
        let [s, t] = self.uv(face, q);
        let byte = |x: f64| {
            ((x - CRACK_UV_MIN) / CRACK_UV_RANGE * 255.0)
                .round()
                .clamp(0.0, 255.0) as u32
        };
        byte(s) | byte(t) << 8 | u32::from(self.slot) << 16 | u32::from(self.strength) << 24
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
///
/// `carve` none: the face is flat (an unstruck face of a cracked cell, one quad); `crack`: the
/// crack overlay each vertex carries (BLOCK_DAMAGE.md §6.3), following the carve.
pub(crate) fn carve_face(
    m: &mut MeshArrays,
    sf: &StruckFace,
    carve: Option<&Carve>,
    look: &DamageLook,
    grid: usize,
    crack: Option<&CrackLayer>,
) -> usize {
    let axis = sf.face / 2;
    let sign = if sf.face.is_multiple_of(2) { 1.0 } else { -1.0 };
    let (u, v) = plane_axes(axis);
    let (eu, ev) = (ext(u), ext(v));
    let (us, vs) = match carve {
        Some(c) => {
            let (lo, hi) = c.reach();
            (lines(lo[0], hi[0], eu, grid), lines(lo[1], hi[1], ev, grid))
        }
        None => (vec![0.0, eu], vec![0.0, ev]),
    };
    let height = |q: [f64; 2]| carve.map_or(0.0, |c| c.height(q));
    let plane = f64::from(sf.cell[axis]) + if sign > 0.0 { 1.0 } else { 0.0 };
    let deepest = carve.map_or(1.0, Carve::depth).max(1e-6);
    // A spall's scar is fresh rock: lifted toward the look's `color`, barely occluded.
    let fresh = carve.map_or(0.0, Carve::fresh);
    let lift = look.color.map(|c| 1.8 * f64::from(c) / 255.0);
    let occlude = if fresh > 0.0 { 0.15 } else { 0.4 };
    // The crack overlay's packed bytes at a point of the face (face metres); before this face's
    // vertices, the stream is filled with "none" for the mesh's earlier ones.
    if crack.is_some() {
        m.cracks.resize(m.positions.len() / 3, 0);
    }
    let crack_at = |pu: f64, pv: f64| crack.map(|c| c.pack(sf.face, [pu, pv]));
    // A point of the face (face metres) carved: its cell-space position and its depth share.
    let place = |pu: f64, pv: f64| -> ([f64; 3], f64) {
        let h = height([pu, pv]);
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
        let gu = (height([pu + E, pv]) - height([pu - E, pv])) / (2.0 * E);
        let gv = (height([pu, pv + E]) - height([pu, pv - E])) / (2.0 * E);
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
        crack: Option<u32>,
        n: [f64; 3],
        light: f32,
        flood: [f32; 2],
    }
    let smooth = carve.is_none_or(Carve::smooth);
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
                crack: crack_at(pu, pv),
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
        let t = fresh * (g.s * 8.0).min(1.0);
        let dim = if fresh > 0.0 {
            1.0
        } else {
            1.0 - look.darken * g.s
        };
        m.positions.extend(g.p.map(|x| x as f32));
        m.normals.extend(n.map(|x| x as f32));
        let mut k = 0;
        m.colors.extend(rgb.map(|c| {
            let l = 1.0 + t * (lift[k] - 1.0);
            k += 1;
            (c * dim * l) as f32
        }));
        m.uvs.extend(uv(g.p));
        m.tiles.extend(tile);
        // The floor of the dent is occluded by its walls.
        m.light.push(g.light * (1.0 - occlude * g.s as f32));
        m.flood.extend(g.flood);
        if let Some(c) = g.crack {
            m.cracks.push(c);
        }
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
        mesh_sized(f, block, local, marks, fraction, HitMark::ONE)
    }

    /// [`mesh_one`] with every mark's piece `chunk` (`HitMark::chunk`, sixteenths).
    fn mesh_sized(
        f: FaceFrame,
        block: &str,
        local: [i32; 3],
        marks: &[(u8, [u8; 3])],
        fraction: u8,
        chunk: u8,
    ) -> (Vec<[f64; 3]>, Vec<[u32; 3]>) {
        let cell = world_cell(f, local);
        let mut padded = vec![0u16; PADDED_VOLUME];
        padded[padded_index(cell[0], cell[1], cell[2])] = dwell_blocks::require(block);
        let view = DamageView {
            fraction,
            marks: marks
                .iter()
                .map(|&(face, point)| HitMark {
                    chunk,
                    ..HitMark::at(world_face(f, face), point)
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
                .map(|&(face, point)| HitMark::at(face, point))
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
                // The bite rule's volume at the break (one mark: all of it on the top).
                let carve = carve_for(2, &ms, &look, dwell_protocol::damage::carved_m3(block, 1.0));
                let top = f64::from(local[1] + 1);
                let mut deepest: f64 = 0.0;
                for p in &pts {
                    // A top vertex (y near the top, not the bottom's).
                    if p[1] < top - 0.95 {
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
                    deepest > 0.05 && deepest <= DEEPEST * CELL_H + 1e-6,
                    "{block}: as deep as its bite, inside the cell: {deepest} of {}",
                    carve.depth()
                );
            }
        }
    );

    #[test]
    fn the_bite_rules_cell_is_the_worlds() {
        // face-a-only: a volume.
        assert_eq!(dwell_protocol::damage::CELL_M3, CELL_H);
    }

    dwell_world::both_faces!(
        /// The bite rule (owner, 2026-11-12: "the chunks that fly off and show chunked off the
        /// blocks should be equivalent to 50% of the block's volume / number of swings required
        /// to break it"): after k of a block's n hits with a tool, the meshed cell has lost
        /// k × 0.5 × V ÷ n (its closed mesh's volume, within 8 %: the mesh's grid against the field), half at the break;
        /// a metal pickaxe's 3 hits bite 5/3 of the stone one's 5; on both faces.
        fn the_carve_removes_half_the_block_over_its_hits_k_bites_after_k_hits(f: FaceFrame) {
            let local = [3, 5, 4];
            let py = |y: u8| if f == FaceFrame::B { 255 - y } else { y };
            let spots = [
                [128, 255, 128],
                [70, 255, 90],
                [180, 255, 170],
                [100, 255, 190],
            ];
            let cell = CELL_H;
            let carved = |block: &str, k: u32, n: u32| -> f64 {
                let marks: Vec<_> = spots
                    .iter()
                    .cycle()
                    .take(k.min(4) as usize)
                    .map(|p| (2u8, [p[0], py(p[1]), p[2]]))
                    .collect();
                let frac = (255.0 * f64::from(k) / f64::from(n)).round() as u8;
                let (pts, tris) = mesh_sized(f, block, local, &marks, frac, HitMark::ONE);
                // The closed mesh's volume (divergence theorem), y in cells.
                let v: f64 = tris
                    .iter()
                    .map(|t| {
                        let [a, b, c] = t.map(|i| pts[i as usize]);
                        let bc = [
                            b[1] * c[2] - b[2] * c[1],
                            b[2] * c[0] - b[0] * c[2],
                            b[0] * c[1] - b[1] * c[0],
                        ];
                        (a[0] * bc[0] + a[1] * bc[1] + a[2] * bc[2]) / 6.0
                    })
                    .sum::<f64>()
                    .abs()
                    * cell;
                cell - v
            };
            for (block, tool, n) in [
                ("dwell:dirt", "dwell:wooden_shovel", 5),
                ("dwell:dirt", "dwell:metal_shovel", 3),
                ("dwell:stone", "dwell:stone_pickaxe", 5),
                ("dwell:stone", "dwell:metal_pickaxe", 3),
            ] {
                assert_eq!(
                    dwell_protocol::damage::hits_to_break(block, Some(tool)),
                    Some(n)
                );
                let bite = dwell_protocol::damage::bite_m3_of(block, Some(tool)).unwrap();
                for k in 1..n {
                    let got = carved(block, k, n);
                    let want = f64::from(k) * bite;
                    assert!(
                        (got - want).abs() < 0.08 * want,
                        "{block} {tool}: {k} of {n} hits carved {got:.4} m³, the rule {want:.4}"
                    );
                }
                let at_break = carved(block, n, n);
                assert!(
                    (at_break - 0.5 * cell).abs() < 0.08 * 0.5 * cell,
                    "{block} {tool}: half the block at the break: {at_break:.4}"
                );
            }
            let one = |tool| carved("dwell:stone", 1, if tool == "metal" { 3 } else { 5 });
            let r = one("metal") / one("stone");
            assert!(
                (r - 5.0 / 3.0).abs() < 0.12,
                "a metal bite 5/3 of a stone one: {r}"
            );
        }
    );

    dwell_world::both_faces!(
        /// A log hit by the axe through block damage (WORK_QUEUE item 51) is carved with the axe's
        /// bites at its marks, grown by the bite rule: after k of its n hits the closed mesh has
        /// lost k × ½ ÷ n of the log's own volume (the octagon's), within 10 %; undamaged, it is
        /// the plain log; on both faces.
        fn a_damaged_log_is_carved_by_its_marks_k_bites_after_k_hits(f: FaceFrame) {
            let local = [3, 5, 4];
            let py = |y: u8| if f == FaceFrame::B { 255 - y } else { y };
            // On −x (face 1), the lower and upper bands, across z.
            let spots = [[0, 64, 128], [0, 190, 90], [0, 64, 170], [0, 190, 140]];
            let volume = |k: u32, n: u32| -> (f64, usize) {
                let marks: Vec<_> = spots
                    .iter()
                    .cycle()
                    .take(k.min(4) as usize)
                    .map(|p| (1u8, [p[0], py(p[1]), p[2]]))
                    .collect();
                let frac = (255.0 * f64::from(k) / f64::from(n)).round() as u8;
                let (pts, tris) = mesh_one(f, "dwell:log", local, &marks, frac);
                let v: f64 = tris
                    .iter()
                    .map(|t| {
                        let [a, b, c] = t.map(|i| pts[i as usize]);
                        let bc = [
                            b[1] * c[2] - b[2] * c[1],
                            b[2] * c[0] - b[0] * c[2],
                            b[0] * c[1] - b[1] * c[0],
                        ];
                        (a[0] * bc[0] + a[1] * bc[1] + a[2] * bc[2]) / 6.0
                    })
                    .sum::<f64>()
                    .abs();
                (v, tris.len())
            };
            let n = dwell_protocol::damage::hits_to_break("dwell:log", Some("dwell:stone_axe"))
                .expect("an axe breaks a log");
            let (whole, _) = volume(0, n);
            for k in 1..n {
                let (v, _) = volume(k, n);

                let got = (whole - v) / whole;
                let want = 0.5 * f64::from(k) / f64::from(n);
                assert!(
                    (got - want).abs() < 0.1 * want,
                    "{k} of {n} hits carved {got:.4} of the log, the rule {want:.4}"
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
                        carve_face(&mut m, &sf, Some(&carve), &look, grid_for(&look, 1), None);
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
                        let mark = HitMark::at(face as u8, [100, 140, 120]);
                        let carve = carve_of(face, &[&mark], &look, 1.0);
                        let sf = StruckFace {
                            cell: [1, 1, 1],
                            face,
                            color: 0xffffff,
                            tile: crate::textures::tile_rect("plain"),
                            light: [1.0; 4],
                            flood: [[1.0, 0.0]; 4],
                        };
                        total += carve_face(
                            &mut MeshArrays::default(),
                            &sf,
                            Some(&carve),
                            &look,
                            g,
                            None,
                        );
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
            flakes: 2,
            grid: 12,
            triangles: 400,
            cracks: None,
        };
        let mark = HitMark::at(2, [128, 255, 128]);
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
        carve_face(&mut m, &sf, Some(&carve), &look, grid_for(&look, 1), None);
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
        assert_eq!(look_of("dwell:stone").unwrap().profile, CarveProfile::Spall);
        assert_eq!(look_of("dwell:leaves"), None);
    }

    /// Stone shatters, it doesn't dent (owner, 2026-11-12: "The stone hits should be elongated low
    /// poly shards stone doesn't dent, it shatters"): every stone look spalls (flat, shallow
    /// chip-outs a few cm deep), its scar lifted lighter, not a dark pit (`darken` unread).
    #[test]
    fn stone_spalls_shallow_and_light_not_a_dark_pit() {
        // both-faces: data only, no up.
        for b in [
            "dwell:stone",
            "dwell:basalt",
            "dwell:red_sandstone",
            "dwell:obsidian",
        ] {
            let l = look_of(b).unwrap();
            assert_eq!(l.profile, CarveProfile::Spall, "{b}");
            assert!(l.depth <= 0.1, "{b}: a scar {} m deep", l.depth);
            assert!(l.flakes >= 1, "{b}: flakes");
            // The cracks stay faint and late on a spalled face: the chip-outs, not a dark starburst.
            let c = l.cracks.expect("stone cracks");
            assert!(
                c.strength <= 0.35 && c.stages[0] >= 0.5,
                "{b}: cracks {} from {}",
                c.strength,
                c.stages[0]
            );
        }
        let look = look_of("dwell:stone").unwrap();
        let mark = HitMark::at(2, [128, 255, 128]);
        let carve = carve_of(2, &[&mark], &look, 1.0);
        let sf = StruckFace {
            cell: [0, 0, 0],
            face: 2,
            color: 0x808080,
            tile: crate::textures::tile_rect("plain"),
            light: [1.0; 4],
            flood: [[1.0, 0.0]; 4],
        };
        let mut m = MeshArrays::default();
        carve_face(&mut m, &sf, Some(&carve), &look, grid_for(&look, 1), None);
        // The deepest vertex's colour is lighter than the face's, its light barely occluded.
        let (mut low, mut at) = (f32::MAX, 0);
        for (i, p) in m.positions.chunks_exact(3).enumerate() {
            if p[1] < low {
                (low, at) = (p[1], i);
            }
        }
        let flat = m
            .positions
            .chunks_exact(3)
            .position(|p| (p[1] - 1.0).abs() < 1e-6)
            .unwrap();
        let lum = |i: usize| m.colors[i * 3..i * 3 + 3].iter().sum::<f32>();
        assert!(low < 1.0 - 0.01, "carved: {low}");
        assert!(
            lum(at) > lum(flat) * 1.35,
            "fresh rock lighter: {} vs {}",
            lum(at),
            lum(flat)
        );
        assert!(m.light[at] > 0.85, "not a dark pit: {}", m.light[at]);
    }

    /// A spall is flat chip-outs (planar facets like conchoidal fractures): inside each flake past
    /// its bevel the field is one plane; the flakes are elongated (~3:1), convex and inside the
    /// cell; more hits add flakes that merge; by data alone for a fixture material.
    #[test]
    fn a_fixture_material_by_data_alone_spalls_into_flat_elongated_flakes() {
        // both-faces: one face-local face's field, no up.
        let look = DamageLook {
            kind: DamageLookKind::Shards,
            size: 0.4,
            color: [230, 220, 200],
            crack: [80, 60, 40],
            depth: 0.06,
            rim: 0.0,
            parallax: 0,
            darken: 0.0,
            profile: CarveProfile::Spall,
            facets: 5,
            flakes: 3,
            grid: 12,
            triangles: 512,
            cracks: None,
        };
        let marks = [
            HitMark::at(2, [128, 255, 128]),
            HitMark::at(2, [150, 255, 100]),
        ];
        let one = carve_of(2, &[&marks[0]], &look, 0.3);
        let Carve::Spall(sp) = &one else {
            panic!("a spall by its data");
        };
        assert_eq!(sp.flakes.len(), 3, "three flakes by its data");
        for fl in &sp.flakes {
            // Convex, counter-clockwise.
            let n = fl.corners.len();
            assert!(n >= 4, "{n} corners");
            for k in 0..n {
                let (a, b, c) = (
                    fl.corners[k],
                    fl.corners[(k + 1) % n],
                    fl.corners[(k + 2) % n],
                );
                let cr = (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]);
                assert!(cr > 0.0, "convex");
            }
            // Elongated: its length over its width, through its own axes.
            let (mut len, mut wide): (f64, f64) = (0.0, f64::MAX);
            for th in 0..90 {
                let t = f64::from(th) * std::f64::consts::PI / 90.0;
                let (lo, hi) = fl.corners.iter().fold((f64::MAX, f64::MIN), |(lo, hi), c| {
                    let d = c[0] * t.cos() + c[1] * t.sin();
                    (lo.min(d), hi.max(d))
                });
                len = len.max(hi - lo);
                wide = wide.min(hi - lo);
            }
            assert!(len >= 2.2 * wide, "elongated: {len} by {wide}");
            // Planar: inside past the bevel, the height is the floor's plane exactly.
            let pts: Vec<[f64; 2]> = (0..14_400)
                .map(|i| {
                    let (a, b) = (f64::from(i % 120) / 120.0, f64::from(i / 120) / 120.0);
                    [0.2 + 0.6 * a, 0.2 + 0.6 * b]
                })
                .filter(|&q| fl.inside(q) > fl.bevel && fl.floor(q) > 0.0)
                .collect();
            assert!(pts.len() >= 3, "a floor");
            for q in &pts {
                let h = fl.height(*q);
                let plane = -(fl.depth
                    + fl.grad[0] * (q[0] - fl.origin[0])
                    + fl.grad[1] * (q[1] - fl.origin[1]));
                assert!((h - plane).abs() < 1e-12, "one plane: {h} vs {plane}");
                assert!(-h <= look.depth + 1e-9, "a few cm deep");
            }
        }
        // The floors step: the next flake is deeper at the strike.
        assert!(sp.flakes[1].depth > sp.flakes[0].depth, "stepped");
        // A second hit adds its flakes; where they overlap the deepest wins (merged).
        let two = carve_of(2, &[&marks[0], &marks[1]], &look, 0.6);
        let Carve::Spall(sp2) = &two else { panic!() };
        assert_eq!(sp2.flakes.len(), 6);
        // Meshed (one flake a hit, the grid coarse beside the stepped ones): flat-lit, one floor.
        let sf = StruckFace {
            cell: [0, 0, 0],
            face: 2,
            color: 0xffffff,
            tile: crate::textures::tile_rect("plain"),
            light: [1.0; 4],
            flood: [[1.0, 0.0]; 4],
        };
        let mut m = MeshArrays::default();
        let mut l1 = look;
        l1.flakes = 1;
        let tris = carve_face(
            &mut m,
            &sf,
            Some(&carve_of(2, &[&marks[0]], &l1, 1.0)),
            &look,
            grid_for(&look, 1),
            None,
        );
        assert!(tris <= look.triangles as usize);
        for t in m.normals.chunks_exact(9) {
            assert!(t[0..3] == t[3..6] && t[3..6] == t[6..9], "lit flat");
        }
        // Many triangles share one floor normal: the flat scars, not a pit's spikes.
        let mut normals: HashMap<Vec<i32>, usize> = HashMap::new();
        for t in m.normals.chunks_exact(9) {
            if t[1] < 0.99999 {
                *normals
                    .entry(t[0..3].iter().map(|x| (x * 1e4).round() as i32).collect())
                    .or_insert(0) += 1;
            }
        }
        let biggest = normals.values().copied().max().unwrap_or(0);
        assert!(biggest >= 4, "a flat scar of several triangles: {biggest}");
    }

    /// The crack stage (BLOCK_DAMAGE.md §6.3) is the last whose start the damage has reached:
    /// none below the first, each threshold inclusive, the last to full damage.
    #[test]
    fn the_crack_stage_follows_the_damage_fraction() {
        // both-faces: data only, no up.
        let c = look_of("dwell:stone")
            .unwrap()
            .cracks
            .expect("stone cracks");
        assert_eq!(c.tiles.len(), c.stages.len());
        assert_eq!(c.stage(0.0), None);
        assert_eq!(c.stage(c.stages[0] - 1e-6), None, "below the first stage");
        for (k, &at) in c.stages.iter().enumerate() {
            assert_eq!(c.stage(at), Some(k), "stage {k} starts at {at}");
            let next = c.stages.get(k + 1).copied().unwrap_or(1.0 + 1e-9);
            assert_eq!(c.stage(0.5 * (at + next)), Some(k));
        }
        assert_eq!(c.stage(1.0), Some(c.tiles.len() - 1));
        // Every damage look with a carve also cracks (stone, soils, sand, snow, ice).
        for d in dwell_protocol::chopping::BLOCK_DAMAGE {
            if let Some(l) = d.look {
                assert!(l.cracks.is_some(), "{} cracks", d.block);
            }
        }
    }

    /// The crack tiles are painted into the atlas (shared/textures/cracks.json, materials.json):
    /// each look's tiles exist, are white where uncracked and dark in their cracks, and each stage
    /// covers more of the tile than the one before (hairline to branching).
    #[test]
    fn the_crack_tiles_are_in_the_atlas_and_grow() {
        // both-faces: atlas data only, no up.
        for block in ["dwell:stone", "dwell:dirt"] {
            let c = look_of(block).unwrap().cracks.unwrap();
            let mut last = 0;
            for t in c.tiles {
                assert!(crate::textures::has_tile(t), "{t} in the atlas order");
                assert!(
                    crate::textures::PAINTERLY_TILES.contains(t),
                    "{t} is painted"
                );
                let ts = crate::textures::TILE as i64;
                let px: Vec<[u8; 4]> = (0..ts * ts)
                    .map(|k| crate::textures::texel_of(t, k % ts, k / ts))
                    .collect();
                assert_eq!(px[0][..3], [255, 255, 255], "{t}: white where uncracked");
                let dark = px.iter().filter(|p| p[0] < 200).count();
                assert!(
                    dark > last,
                    "{t}: {dark} cracked texels, not more than {last}"
                );
                last = dark;
            }
        }
    }

    /// A damaged cell for [`mesh_cracked`]: face-local cell, its marks, fraction.
    type Hit<'a> = ([i32; 3], &'a [(u8, [u8; 3])], u8);

    /// The opaque mesh of `cells` (block, face-local cell) on face `f` with `damage` (face-local
    /// cell, face-local faces, world points, fraction): positions and normals face-local.
    fn mesh_cracked(f: FaceFrame, cells: &[(&str, [i32; 3])], damage: &[Hit<'_>]) -> MeshArrays {
        let mut padded = vec![0u16; PADDED_VOLUME];
        for &(b, local) in cells {
            let c = world_cell(f, local);
            padded[padded_index(c[0], c[1], c[2])] = dwell_blocks::require(b);
        }
        let views: Vec<_> = damage
            .iter()
            .map(|&(local, marks, fraction)| {
                (
                    world_cell(f, local),
                    DamageView {
                        fraction,
                        marks: marks
                            .iter()
                            .map(|&(face, point)| HitMark::at(world_face(f, face), point))
                            .collect(),
                    },
                )
            })
            .collect();
        let mut m = mesh_chunk_damaged(
            &padded,
            &[],
            f == FaceFrame::B,
            [0, 0, 0],
            &[],
            None,
            ChunkFlags::default(),
            &views,
        )
        .expect("padded volume")
        .opaque;
        if f == FaceFrame::B {
            for p in m.positions.iter_mut().skip(1).step_by(3) {
                *p = (f64::from(N) * CELL_H - f64::from(*p)) as f32;
            }
            for n in m.normals.iter_mut().skip(1).step_by(3) {
                *n = -*n;
            }
        }
        m
    }

    dwell_world::both_faces!(
        /// Every exposed face of a cracked cell carries the crack overlay (BLOCK_DAMAGE.md §6.3),
        /// the struck one carved and the others flat, all at the stage's tile; a face against a
        /// neighbour is not drawn, and an undamaged cell beside it has none; face-locally on both
        /// faces.
        fn every_exposed_face_of_a_cracked_cell_carries_the_overlay(f: FaceFrame) {
            let local = [4, 6, 4];
            let py = |y: u8| if f == FaceFrame::B { 255 - y } else { y };
            let look = look_of("dwell:stone").unwrap();
            let c = look.cracks.unwrap();
            let fraction = 200u8;
            let stage = c.stage(200.0 / 255.0).unwrap();
            let slot = crate::textures::TILE_ORDER
                .iter()
                .position(|t| *t == c.tiles[stage])
                .unwrap() as u32
                + 1;
            let marks = [(2u8, [90u8, py(255), 160])];
            let m = mesh_cracked(
                f,
                &[
                    ("dwell:stone", local),
                    // Below it (its bottom hidden) and one cell over, undamaged.
                    ("dwell:stone", [4, 5, 4]),
                    ("dwell:stone", [7, 6, 4]),
                ],
                &[(local, &marks, fraction)],
            );
            let count = m.positions.len() / 3;
            assert_eq!(m.cracks.len(), count, "the stream covers every vertex");
            let mut faces = std::collections::HashSet::new();
            for v in 0..count {
                let p: Vec<f64> = (0..3).map(|a| f64::from(m.positions[3 * v + a])).collect();
                let n: Vec<f32> = m.normals[3 * v..3 * v + 3].to_vec();
                let cracked = (m.cracks[v] >> 16) & 0xff;
                let inside = p[0] >= 4.0 - 1e-4
                    && p[0] <= 5.0 + 1e-4
                    && p[2] >= 4.0 - 1e-4
                    && p[2] <= 5.0 + 1e-4
                    && p[1] >= 6.0 * CELL_H - 1e-4
                    && p[1] <= 7.0 * CELL_H + 1e-4;
                if p[0] > 6.5 || p[1] < 6.0 * CELL_H - 1e-4 {
                    assert_eq!(cracked, 0, "an undamaged cell has no cracks: {p:?}");
                }
                if cracked != 0 {
                    assert!(inside, "only the damaged cell cracks: {p:?}");
                    assert_eq!(cracked, slot, "the stage's tile");
                    assert_eq!(m.cracks[v] >> 24, (c.strength * 255.0).round() as u32);
                    // The face it lies on, by its main normal axis and sign.
                    let a = (0..3)
                        .max_by(|&x, &y| n[x].abs().total_cmp(&n[y].abs()))
                        .unwrap();
                    faces.insert(2 * a + usize::from(n[a] < 0.0));
                }
            }
            // +X, −X, +Y (carved), +Z, −Z; −Y lies on the stone below.
            let want: std::collections::HashSet<usize> = [0, 1, 2, 4, 5].into_iter().collect();
            assert_eq!(faces, want, "every exposed face, not the hidden one");
        }
    );

    dwell_world::both_faces!(
        /// No cracks on an undamaged chunk or below the first stage: no stream at all (the GPU
        /// buffer fills it with "none"), and the cell stays in the greedy merge.
        fn no_cracks_without_damage_or_below_the_first_stage(f: FaceFrame) {
            let local = [3, 3, 3];
            let py = |y: u8| if f == FaceFrame::B { 255 - y } else { y };
            let plain = mesh_cracked(f, &[("dwell:stone", local)], &[]);
            assert!(plain.cracks.is_empty());
            let first = look_of("dwell:stone").unwrap().cracks.unwrap().stages[0];
            let below = ((first * 255.0).floor() as u8).saturating_sub(1);
            let light = mesh_cracked(
                f,
                &[("dwell:stone", local)],
                &[(local, &[(2, [128, py(255), 128])], below)],
            );
            assert!(light.cracks.is_empty(), "below the first stage");
        }
    );

    /// The cracks spread from the hit and round the corner (BLOCK_DAMAGE.md §6.3): on the struck
    /// face the tile's middle lies at the hit; on a side the hit nearest it centres the tile, so
    /// the crack meets the edge where it reached it from the top.
    #[test]
    fn cracks_centre_on_the_hit_and_run_round_the_corner() {
        // both-faces: face-local by construction (a face-B cell's marks are turned over first).
        let look = look_of("dwell:stone").unwrap();
        let view = local_view(&[(2, [77, 255, 179])], 255);
        let layer = CrackLayer::of(&look, &view, [1, 2, 3]).expect("full damage cracks");
        let (hx, hz) = (77.0 / 255.0, 179.0 / 255.0);
        // The top (axis 1: (u, v) = (z, x)).
        let top = layer.for_face(2, &view.marks);
        let at = top.uv(2, [hz, hx]);
        assert!(
            (at[0] - 0.5).abs() < 1e-9 && (at[1] - 0.5).abs() < 1e-9,
            "{at:?}"
        );
        // The +X side (axis 0: (u, v) = (y, z)), at its top edge level with the hit.
        let side = layer.for_face(0, &view.marks);
        let edge = side.uv(0, [CELL_H, hz]);
        assert!(
            (edge[0] - 0.5).abs() < 1e-9 && (edge[1] - 0.5).abs() < 1e-9,
            "{edge:?}"
        );
        // Further down the side, further from the tile's middle (along t, its up).
        let down = side.uv(0, [0.0, hz]);
        assert!((down[1] - 0.5).abs() > 0.3 * CELL_H / look.cracks.unwrap().span);
        // Packed: four bytes, the tile's s and t over [−½, 1½].
        let b = top.pack(2, [hz, hx]);
        let s = f64::from(b & 0xff) / 255.0 * CRACK_UV_RANGE + CRACK_UV_MIN;
        assert!((s - 0.5).abs() < CRACK_UV_RANGE / 255.0);
        assert_eq!((b >> 16) & 0xff, u32::from(top.slot));
    }

    /// A fixture material's cracks by data alone (BLOCK_DAMAGE.md §6.3): its own set (the wood
    /// tiles), stages and strength pick the tile and the darkening.
    #[test]
    fn a_fixture_material_by_data_alone_gets_its_cracks() {
        // both-faces: data only, no up.
        let mut look = look_of("dwell:dirt").unwrap();
        look.cracks = Some(dwell_protocol::damage::Cracks {
            tiles: &["crackWood1", "crackWood3"],
            stages: &[0.3, 0.7],
            strength: 0.5,
            span: 1.0,
        });
        let at = |fraction: u8| CrackLayer::of(&look, &local_view(&[], fraction), [0, 0, 0]);
        assert_eq!(at(70), None, "below 0.3");
        let slot = |t: &str| {
            crate::textures::TILE_ORDER
                .iter()
                .position(|x| *x == t)
                .unwrap()
                + 1
        };
        assert_eq!(at(100).unwrap().slot as usize, slot("crackWood1"));
        assert_eq!(at(200).unwrap().slot as usize, slot("crackWood3"));
        assert_eq!(at(200).unwrap().strength, 128);
        assert_eq!(at(200).unwrap().span, 1.0);
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
                        .map(|&f| HitMark::at(f, [(70 + 3 * k) as u8, 140, (200 - 2 * k) as u8]))
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
