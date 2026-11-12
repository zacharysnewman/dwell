//! Block damage on a stored ramp (BUGS.md #110, BLOCK_DAMAGE.md §6.5): the ramp's struck
//! polygons carved as a cube's struck faces are (§6.2), its exposed polygons cracked (§6.3), the
//! bite rule's volume (§10, a ramp's ½) taken out over its hits. Owner, 2026-11-12: "Slopes aren't
//! affected visually by tools when hit, though they do break."
//!
//! Each polygon of the ramp's shape (the slope, the high wall, the triangular sides, the floor)
//! gets its own plane frame in metres: the marks are given to the polygon their hit point lies on,
//! and a struck polygon is meshed as a grid over its corners pushed in along its inward normal by
//! the look's height field (`relief::Carve`, a soil's bowls or a stone's facets and flakes),
//! faded to 0 at the polygon's outline (so its edges, which the ramp's other polygons and the
//! neighbours meet, stay put: watertight) and kept inside the solid (no deeper than
//! [`DEEPEST`] of the ramp's thickness under each point). Face-local: the mesher turns a face-B
//! chunk over first, so this is face A's.

use super::damage_mesh::{CrackLayer, DEEPEST, carve_fit, carve_shape, grid_for};
use super::relief::{self, Carve};
use super::{MeshArrays, SideTop, vertex_rgb};
use crate::textures::TileRect;
use dwell_blocks::{ShapeInfo, shape::faces_of};
use dwell_protocol::damage::{DamageLook, DamageView, HitMark};
use dwell_world::CELL_H;

type V3 = [f64; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale(a: V3, k: f64) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn unit(a: V3) -> V3 {
    let l = dot(a, a).sqrt().max(1e-12);
    scale(a, 1.0 / l)
}
/// Cell units (y in cells) to metres within the cell.
fn metres(p: V3) -> V3 {
    [p[0], p[1] * CELL_H, p[2]]
}

/// One polygon of a ramp's shape in its own plane frame (metres within the cell).
#[derive(Clone, Debug)]
pub(crate) struct Plane {
    /// Its corners (metres), counter-clockwise from outside; a triangle's last repeated, so every
    /// polygon is gridded as a quad (the repeated corner's row collapses to a point).
    pub quad: [V3; 4],
    /// The outward unit normal (metres).
    pub n: V3,
    /// The frame's origin and in-plane axes: a point's (u, v) are its offsets along them (m).
    pub o: V3,
    pub eu: V3,
    pub ev: V3,
    /// The polygon's (u, v) corners, counter-clockwise, and its extent (m).
    pub outline: Vec<[f64; 2]>,
    pub dims: [f64; 2],
}

impl Plane {
    /// A polygon of `pts` (cell units within the cell, counter-clockwise from outside).
    pub(crate) fn of(pts: &[V3]) -> Option<Plane> {
        if pts.len() < 3 {
            return None;
        }
        let m: Vec<V3> = pts.iter().map(|&p| metres(p)).collect();
        let mut area = [0.0; 3];
        for i in 1..m.len() - 1 {
            area = add(area, cross(sub(m[i], m[0]), sub(m[i + 1], m[0])));
        }
        if dot(area, area) < 1e-12 {
            return None;
        }
        let n = unit(area);
        let eu = unit(sub(m[1], m[0]));
        let ev = cross(n, eu);
        let raw: Vec<[f64; 2]> = m
            .iter()
            .map(|&p| [dot(sub(p, m[0]), eu), dot(sub(p, m[0]), ev)])
            .collect();
        let lo = [
            raw.iter().map(|q| q[0]).fold(f64::MAX, f64::min),
            raw.iter().map(|q| q[1]).fold(f64::MAX, f64::min),
        ];
        let hi = [
            raw.iter().map(|q| q[0]).fold(f64::MIN, f64::max),
            raw.iter().map(|q| q[1]).fold(f64::MIN, f64::max),
        ];
        let o = add(m[0], add(scale(eu, lo[0]), scale(ev, lo[1])));
        let quad = [m[0], m[1], m[2], *m.get(3).unwrap_or(&m[2])];
        Some(Plane {
            quad,
            n,
            o,
            eu,
            ev,
            outline: raw.iter().map(|q| [q[0] - lo[0], q[1] - lo[1]]).collect(),
            dims: [hi[0] - lo[0], hi[1] - lo[1]],
        })
    }

    /// A point's (u, v) on the plane (m).
    pub(crate) fn uv(&self, p: V3) -> [f64; 2] {
        let d = sub(p, self.o);
        [dot(d, self.eu), dot(d, self.ev)]
    }

    /// The point at (u, v) on the plane (m).
    pub(crate) fn at(&self, q: [f64; 2]) -> V3 {
        add(self.o, add(scale(self.eu, q[0]), scale(self.ev, q[1])))
    }

    /// The point at grid parameters (s, t) ∈ [0, 1]² of its corners (bilinear).
    fn grid_point(&self, s: f64, t: f64) -> V3 {
        let [a, b, c, d] = self.quad;
        let w = [(1.0 - s) * (1.0 - t), s * (1.0 - t), s * t, (1.0 - s) * t];
        let mut p = [0.0; 3];
        for (k, q) in [a, b, c, d].iter().enumerate() {
            p = add(p, scale(*q, w[k]));
        }
        p
    }

    /// How far (m) a point (u, v) lies inside the outline (negative: outside).
    pub(crate) fn inside(&self, q: [f64; 2]) -> f64 {
        let k = self.outline.len();
        (0..k)
            .map(|i| {
                let a = self.outline[i];
                let b = self.outline[(i + 1) % k];
                let e = [b[0] - a[0], b[1] - a[1]];
                let l = e[0].hypot(e[1]).max(1e-12);
                (e[0] * (q[1] - a[1]) - e[1] * (q[0] - a[0])) / l
            })
            .fold(f64::MAX, f64::min)
    }
}

/// A damaged ramp cell: its shape's planes (coplanar polygons of one face merged: a slope's two
/// triangles are one plane, carved across the diagonal), which marks struck which, and its look.
pub(crate) struct RampDamage<'a> {
    pub look: DamageLook,
    pub view: &'a DamageView,
    /// Every plane of the shape, visible or not.
    pub planes: Vec<Option<Plane>>,
    /// The plane each polygon (`faces_of` order) lies in, and each plane's first polygon (which
    /// draws it).
    pub group_of: Vec<usize>,
    pub first: Vec<usize>,
    /// The marks on each plane (indices into `view.marks`).
    pub on: Vec<Vec<usize>>,
    /// The cell's carved volume (m³, the bite rule) and its crack layer, if staged.
    pub carved_m3: f64,
    pub crack: Option<CrackLayer>,
    /// How many visible planes are struck (they share the cell's triangle budget).
    pub struck_visible: usize,
    /// Each plane's carve, sized by the bite rule (none: not struck).
    carves: Vec<Option<Carve>>,
}

/// A mark's hit point in the cell (metres, face-local).
fn mark_point(m: &HitMark) -> V3 {
    let e = [1.0, CELL_H, 1.0];
    std::array::from_fn(|a| f64::from(m.point[a]) / 255.0 * e[a])
}

/// The planes of a shape's polygons (`tags` and points in cell units, counter-clockwise from
/// outside): polygons of one tag in one plane merged into their convex hull (a convex shape's
/// coplanar faces are one convex polygon) when it has at most four corners; and the plane each
/// polygon lies in.
pub(crate) fn merged(polys: &[(u8, Vec<V3>)]) -> (Vec<Option<Plane>>, Vec<usize>) {
    let mut planes: Vec<Option<Plane>> = Vec::new();
    let mut members: Vec<(u8, Vec<V3>)> = Vec::new();
    let mut group_of = Vec::with_capacity(polys.len());
    for (tag, pts) in polys {
        let own = Plane::of(pts);
        let same = own.as_ref().and_then(|pl| {
            (0..members.len()).find(|&g| {
                members[g].0 == *tag
                    && planes[g].as_ref().is_some_and(|o| {
                        dot(o.n, pl.n) > 1.0 - 1e-6 && dot(sub(pl.o, o.o), o.n).abs() < 1e-6
                    })
            })
        });
        let joined = same.and_then(|g| {
            let o = planes[g].clone()?;
            let mut all = members[g].1.clone();
            all.extend(pts.iter().copied());
            let hull = super::damage_mesh::hull(all.iter().map(|&p| o.uv(metres(p))).collect());
            if hull.len() > 4 {
                return None;
            }
            let back: Vec<V3> = hull
                .iter()
                .map(|&q| {
                    let m = o.at(q);
                    [m[0], m[1] / CELL_H, m[2]]
                })
                .collect();
            Some((g, Plane::of(&back)?, all))
        });
        match joined {
            Some((g, pl, all)) => {
                planes[g] = Some(pl);
                members[g].1 = all;
                group_of.push(g);
            }
            None => {
                group_of.push(planes.len());
                planes.push(own);
                members.push((*tag, pts.clone()));
            }
        }
    }
    (planes, group_of)
}

impl<'a> RampDamage<'a> {
    /// The damage of a ramp of shape `def` (face-local) with `look`, its carved volume
    /// `carved_m3` (the bite rule's at its damage), struck as `view` (face-local) in `cell`;
    /// `visible` says which of its polygons are drawn. None for a shape that is not convex (its
    /// thickness under a point is not one span).
    pub(crate) fn new(
        def: &ShapeInfo,
        look: DamageLook,
        carved_m3: f64,
        view: &'a DamageView,
        cell: [i32; 3],
        visible: &[bool],
    ) -> Option<RampDamage<'a>> {
        if !def.convex {
            return None;
        }
        let polys: Vec<(u8, Vec<V3>)> = faces_of(def)
            .iter()
            .map(|f| {
                let pts: Vec<V3> = f.v[..usize::from(f.count)]
                    .iter()
                    .map(|p| p.map(f64::from))
                    .collect();
                (f.tag, pts)
            })
            .collect();
        Some(Self::of_polygons(
            &polys, look, carved_m3, view, cell, visible,
        ))
    }

    /// [`RampDamage::new`] over given polygons (a fixture's, in the tests).
    pub(crate) fn of_polygons(
        polys: &[(u8, Vec<V3>)],
        look: DamageLook,
        carved_m3: f64,
        view: &'a DamageView,
        cell: [i32; 3],
        visible: &[bool],
    ) -> RampDamage<'a> {
        let (planes, group_of) = merged(polys);
        let first: Vec<usize> = (0..planes.len())
            .map(|g| group_of.iter().position(|&x| x == g).unwrap_or(0))
            .collect();
        let mut on = vec![Vec::new(); planes.len()];
        for (i, m) in view.marks.iter().enumerate() {
            let p = mark_point(m);
            // The plane the hit lies on: nearest it, within its outline (a point past an edge is
            // as far again).
            let best = planes
                .iter()
                .enumerate()
                .filter_map(|(k, pl)| pl.as_ref().map(|pl| (k, pl)))
                .map(|(k, pl)| {
                    let off = dot(sub(p, pl.o), pl.n).abs();
                    let out = (-pl.inside(pl.uv(p))).max(0.0);
                    (k, off + out)
                })
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((k, _)) = best {
                on[k].push(i);
            }
        }
        let struck_visible = on
            .iter()
            .enumerate()
            .filter(|(g, ms)| !ms.is_empty() && visible.get(first[*g]).copied().unwrap_or(false))
            .count();
        let crack = CrackLayer::of(&look, view, cell);
        let mut d = RampDamage {
            look,
            view,
            planes,
            group_of,
            first,
            on,
            carved_m3,
            crack,
            struck_visible,
            carves: Vec::new(),
        };
        d.carves = (0..d.planes.len()).map(|g| d.fit(g)).collect();
        d
    }

    /// Plane `k`'s carve (sized by the bite rule; none when nothing struck it).
    pub(crate) fn carve(&self, k: usize) -> Option<&Carve> {
        self.carves.get(k)?.as_ref()
    }

    /// How deep (m) the solid runs under a point `p` of polygon `k` along its inward normal: to
    /// the first other plane it leaves through (the shape is convex).
    fn thickness(&self, k: usize, p: V3) -> f64 {
        let Some(own) = &self.planes[k] else {
            return 0.0;
        };
        self.planes
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != k)
            .filter_map(|(_, pl)| pl.as_ref())
            .filter_map(|pl| {
                let c = dot(own.n, pl.n);
                (c < -1e-6).then(|| dot(sub(p, pl.o), pl.n) / c)
            })
            .fold(f64::MAX, f64::min)
            .max(0.0)
    }

    /// The carve of polygon `k` at (u, v): the look's field, faded at the outline, clamped inside
    /// the solid (m, ≤ 0).
    fn height(&self, k: usize, carve: &Carve, q: [f64; 2]) -> f64 {
        let Some(pl) = &self.planes[k] else {
            return 0.0;
        };
        let d = pl.inside(q);
        if d <= 0.0 {
            return 0.0;
        }
        let s = (d / relief::EDGE_FADE_M).clamp(0.0, 1.0);
        let fade = s * s * (3.0 - 2.0 * s);
        (carve.height(q) * fade).max(-DEEPEST * self.thickness(k, pl.at(q)))
    }

    /// The volume (m³) a carve of polygon `k` takes out: its height integrated over the outline.
    fn volume(&self, k: usize, carve: &Carve) -> f64 {
        const S: usize = 48;
        let Some(pl) = &self.planes[k] else {
            return 0.0;
        };
        let (du, dv) = (pl.dims[0] / S as f64, pl.dims[1] / S as f64);
        let mut sum = 0.0;
        for i in 0..S {
            for j in 0..S {
                sum -= self.height(k, carve, [(i as f64 + 0.5) * du, (j as f64 + 0.5) * dv]);
            }
        }
        sum * du * dv
    }

    /// Plane `k`'s carve, sized to its share of the cell's carved volume (its marks' bites over
    /// all of them); none when nothing struck it.
    fn fit(&self, k: usize) -> Option<Carve> {
        let pl = self.planes[k].as_ref()?;
        let ids = &self.on[k];
        if ids.is_empty() {
            return None;
        }
        let marks: Vec<&HitMark> = ids.iter().map(|&i| &self.view.marks[i]).collect();
        let at: Vec<[f64; 2]> = marks.iter().map(|m| pl.uv(mark_point(m))).collect();
        let all: f64 = self.view.marks.iter().map(HitMark::scale).sum();
        let mine: f64 = marks.iter().map(|m| m.scale()).sum();
        let target = if all > 0.0 {
            self.carved_m3 * mine / all
        } else {
            0.0
        };
        // The deepest the solid allows anywhere on the polygon.
        let thick = (0..=16)
            .flat_map(|i| (0..=16).map(move |j| (i, j)))
            .map(|(i, j)| {
                let q = [
                    pl.dims[0] * f64::from(i) / 16.0,
                    pl.dims[1] * f64::from(j) / 16.0,
                ];
                if pl.inside(q) > 0.0 {
                    self.thickness(k, pl.at(q))
                } else {
                    0.0
                }
            })
            .fold(0.0, f64::max);
        let cap = DEEPEST * thick;
        Some(carve_fit(
            target,
            cap,
            &self.look,
            |r, d| carve_shape(pl.dims, &at, &marks, &self.look, r, d, cap),
            |c| self.volume(k, c),
        ))
    }

    /// The cube face a polygon is cracked as (its normal's main axis and sign): the crack tile's
    /// axes and centre follow it.
    fn crack_face(pl: &Plane) -> usize {
        let a = (0..3)
            .max_by(|&x, &y| pl.n[x].abs().total_cmp(&pl.n[y].abs()))
            .unwrap_or(1);
        2 * a + usize::from(pl.n[a] < 0.0)
    }

    /// Draws polygon `fi` (`faces_of` order) damaged into `m`: its plane carved where struck,
    /// with the crack overlay once staged, drawn whole by the plane's first polygon. Returns
    /// whether the polygon is drawn so (false: neither struck nor cracked, the plain polygon).
    pub(crate) fn draw(&self, m: &mut MeshArrays, fi: usize, face: &RampFace<'_>) -> bool {
        let Some(&k) = self.group_of.get(fi) else {
            return false;
        };
        let Some(pl) = &self.planes[k] else {
            return false;
        };
        let carve = self.carve(k);
        if carve.is_none() && self.crack.is_none() {
            return false;
        }
        if self.first[k] != fi {
            return true;
        }
        let f = Self::crack_face(pl);
        let crack = self.crack.map(|c| c.for_face(f, &self.view.marks));
        let grid = grid_for(&self.look, self.struck_visible.max(1));
        draw_polygon(
            m,
            pl,
            face,
            carve.map(|c| (|q: [f64; 2]| self.height(k, c, q), c)),
            &self.look,
            grid,
            crack.as_ref().map(|c| (c, f)),
        );
        true
    }
}

/// What the mesher knows of a ramp polygon as it would have drawn it whole.
pub(crate) struct RampFace<'a> {
    /// The cell (chunk-local, face-local cells).
    pub cell: [i32; 3],
    pub color: u32,
    /// The polygon's own tint (its flat parts keep it).
    pub tint: [f64; 3],
    pub tile: &'a TileRect,
    pub side_top: Option<SideTop>,
    /// Sky light at a point of the plane (cell units within the cell) and its flood light.
    pub light: &'a dyn Fn(V3) -> f32,
    pub flood: [f32; 2],
}

/// Grid lines (parameters in [0, 1]): the ends and `g` divisions over [lo, hi].
fn lines(lo: f64, hi: f64, g: usize) -> Vec<f64> {
    let mut out = vec![0.0];
    for k in 0..=g {
        let x = lo + (hi - lo) * k as f64 / g as f64;
        if x > 1e-6 && x < 1.0 - 1e-6 {
            out.push(x);
        }
    }
    out.push(1.0);
    out
}

/// Meshes polygon `pl` into `m` as a grid over its corners, pushed in by `carve` (its clamped
/// height at (u, v) and the look's field) and carrying `crack` (the layer and the cube face it is
/// cracked as); returns the triangles it added.
pub(crate) fn draw_polygon<H: Fn([f64; 2]) -> f64>(
    m: &mut MeshArrays,
    pl: &Plane,
    face: &RampFace<'_>,
    carve: Option<(H, &Carve)>,
    look: &DamageLook,
    grid: usize,
    crack: Option<(&CrackLayer, usize)>,
) -> usize {
    let height = |q: [f64; 2]| carve.as_ref().map_or(0.0, |(h, _)| h(q));
    // The grid runs finer over the carve's reach (in the corners' parameters).
    let (ss, ts) = match &carve {
        Some(_) => {
            const R: usize = 32;
            let (mut lo, mut hi) = ([1.0f64, 1.0], [0.0f64, 0.0]);
            for i in 0..=R {
                for j in 0..=R {
                    let (s, t) = (i as f64 / R as f64, j as f64 / R as f64);
                    if height(pl.uv(pl.grid_point(s, t))) < -1e-7 {
                        lo = [lo[0].min(s), lo[1].min(t)];
                        hi = [hi[0].max(s), hi[1].max(t)];
                    }
                }
            }
            if lo[0] > hi[0] {
                (vec![0.0, 1.0], vec![0.0, 1.0])
            } else {
                let pad = 1.0 / R as f64;
                (
                    lines((lo[0] - pad).max(0.0), (hi[0] + pad).min(1.0), grid),
                    lines((lo[1] - pad).max(0.0), (hi[1] + pad).min(1.0), grid),
                )
            }
        }
        None => (vec![0.0, 1.0], vec![0.0, 1.0]),
    };
    let deepest = carve.as_ref().map_or(1.0, |(_, c)| c.depth()).max(1e-6);
    let fresh = carve.as_ref().map_or(0.0, |(_, c)| c.fresh());
    let smooth = carve.as_ref().is_none_or(|(_, c)| c.smooth());
    let lift = look.color.map(|c| 1.8 * f64::from(c) / 255.0);
    let occlude = if fresh > 0.0 { 0.15 } else { 0.4 };
    if crack.is_some() {
        m.cracks.resize(m.positions.len() / 3, 0);
    }
    let corner_light: [f32; 4] = pl.quad.map(|q| (face.light)([q[0], q[1] / CELL_H, q[2]]));
    struct Pt {
        /// Carved, metres within the cell.
        p: V3,
        s: f64,
        n: V3,
        light: f32,
        crack: Option<u32>,
    }
    let normal_at = |q: [f64; 2]| -> V3 {
        const E: f64 = 0.004;
        let gu = (height([q[0] + E, q[1]]) - height([q[0] - E, q[1]])) / (2.0 * E);
        let gv = (height([q[0], q[1] + E]) - height([q[0], q[1] - E])) / (2.0 * E);
        unit(sub(pl.n, add(scale(pl.eu, gu), scale(pl.ev, gv))))
    };
    let pts: Vec<Pt> = ts
        .iter()
        .flat_map(|&t| ss.iter().map(move |&s| (s, t)))
        .map(|(s, t)| {
            let flat = pl.grid_point(s, t);
            let q = pl.uv(flat);
            let h = height(q);
            let w = [(1.0 - s) * (1.0 - t), s * (1.0 - t), s * t, (1.0 - s) * t];
            let light: f64 = w
                .iter()
                .zip(corner_light)
                .map(|(w, l)| w * f64::from(l))
                .sum();
            let share = (-h / deepest).clamp(0.0, 1.0);
            Pt {
                p: add(flat, scale(pl.n, h)),
                s: share,
                n: if smooth { normal_at(q) } else { pl.n },
                light: (light as f32) * (1.0 - occlude * share as f32),
                crack: crack.map(|(c, f)| {
                    let (u, v) = super::damage_mesh::plane_axes(f / 2);
                    c.pack(f, [flat[u], flat[v]])
                }),
            }
        })
        .collect();
    let row = ss.len();
    let cell = face.cell.map(f64::from);
    let main = (0..3)
        .max_by(|&x, &y| pl.n[x].abs().total_cmp(&pl.n[y].abs()))
        .unwrap_or(1);
    let vertex = |m: &mut MeshArrays, g: &Pt, n: V3| {
        let tint = if dot(n, pl.n) > 1.0 - 1e-9 {
            face.tint
        } else {
            dwell_look::normal_tint(n[0], n[1], n[2])
        };
        let rgb = vertex_rgb(face.color, tint);
        let t = fresh * (g.s * 8.0).min(1.0);
        let dim = if fresh > 0.0 {
            1.0
        } else {
            1.0 - look.darken * g.s
        };
        let pos = [
            cell[0] + g.p[0],
            cell[1] + g.p[1] / CELL_H,
            cell[2] + g.p[2],
        ];
        m.positions.extend(pos.map(|x| x as f32));
        m.normals.extend(n.map(|x| x as f32));
        let mut k = 0;
        m.colors.extend(rgb.map(|c| {
            let l = 1.0 + t * (lift[k] - 1.0);
            k += 1;
            (c * dim * l) as f32
        }));
        // The polygon's uvs (`Builder::polygon`): projected along its main axis, in blocks; a
        // side's up the world's height in metres (a hanging tile's from the face's top edge).
        let local = [g.p[0], g.p[1] / CELL_H, g.p[2]];
        let tv = match face.side_top {
            Some(st) if face.tile.hang => (local[1] - st.at(&local)) * CELL_H + cell[1] + 1.0,
            _ => pos[1] * CELL_H,
        };
        let (su, sv) = match main {
            1 => (pos[0], pos[2]),
            0 => (pos[2], tv),
            _ => (pos[0], tv),
        };
        m.uvs.extend([su as f32, sv as f32]);
        m.tiles.extend([
            face.tile.u0 as f32,
            face.tile.v0 as f32,
            (face.tile.u1 - face.tile.u0) as f32,
            (face.tile.v1 - face.tile.v0) as f32,
        ]);
        m.light.push(g.light);
        m.flood.extend(face.flood);
        if let Some(c) = g.crack {
            m.cracks.push(c);
        }
    };
    let shared = (m.positions.len() / 3) as u32;
    if smooth {
        for g in &pts {
            vertex(m, g, g.n);
        }
    }
    // A triangle's flat corners (before the carve) coincide on a triangle's collapsed row: none.
    let flat_at = |k: usize| {
        let (i, j) = (k % row, k / row);
        pl.grid_point(ss[i], ts[j])
    };
    let degenerate = |ids: [usize; 3]| {
        let [a, b, c] = ids.map(flat_at);
        let n = cross(sub(b, a), sub(c, a));
        dot(n, n) < 1e-14
    };
    let mut tris = 0;
    for j in 0..ts.len() - 1 {
        for i in 0..row - 1 {
            let corners = [
                i + row * j,
                i + 1 + row * j,
                i + 1 + row * (j + 1),
                i + row * (j + 1),
            ];
            for t in [[0, 1, 2], [0, 2, 3]] {
                let ids = t.map(|c| corners[c]);
                if degenerate(ids) {
                    continue;
                }
                tris += 1;
                if smooth {
                    m.indices.extend(ids.map(|k| shared + k as u32));
                    continue;
                }
                let [a, b, c] = ids.map(|k| pts[k].p);
                let n = cross(sub(b, a), sub(c, a));
                let flat = if dot(n, n) < 1e-18 { pl.n } else { unit(n) };
                for k in ids {
                    vertex(m, &pts[k], flat);
                }
                let base = (m.positions.len() / 3) as u32 - 3;
                m.indices.extend([base, base + 1, base + 2]);
            }
        }
    }
    tris
}

#[cfg(test)]
#[path = "ramp_damage_tests.rs"]
mod tests;
