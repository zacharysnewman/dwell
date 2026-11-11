//! Block damage drawn on the struck faces (docs/BLOCK_DAMAGE.md, D2; look only, collision
//! unchanged): at each hit mark a shard knocked off (stone: an angular facet in a fresher, lighter
//! tone), a scoop (soils: a round, darker bowl) or chips, and cracks that grow with the damage, as
//! decals just off the face in the chunk's opaque mesh. Per block, from its `damage.look` data.
//!
//! A shard is modelled cut into the face, `depth` deep (a fan of angular facets), and drawn flat
//! just off it, each piece lit by its modelled normal. A scoop is one flat quad per struck face
//! whose hollow the terrain shader computes per pixel (`relief`, BLOCK_DAMAGE.md §6.1): the face's
//! own quad stays whole (greedy-meshed, collision unchanged), and the dent reads by its relief.

use super::{
    Kinds, MeshArrays, N,
    ground_mesh::{newell, push_polygon, push_polygon_shaded},
    relief, surface,
};
use crate::textures::TileRect;
use crate::flood::Flood;
use dwell_protocol::damage::{DamageLook, DamageLookKind, DamageView, HitMark, look_of};
use dwell_world::CELL_H;

/// How far the decals stand off the face (cells): over the face, under anything in front.
const LIFT: f64 = 0.004;

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

/// Appends the damage decals of `cells` (chunk-local, face-local) to `m` (cells, rows up).
pub(crate) fn append(
    m: &mut MeshArrays,
    kinds: &Kinds,
    flood: &Flood,
    cells: &[([i32; 3], u16, DamageView)],
) {
    for (cell, state, view) in cells {
        let k = kinds.of(*state);
        let Some(look) = look_of(k.style.block) else {
            continue;
        };
        let fraction = f64::from(view.fraction) / 255.0;
        if look.kind == DamageLookKind::Scoops {
            // One relief quad per struck face, its marks merged into one hollow.
            for face in 0..6u8 {
                let marks: Vec<_> = view.marks.iter().filter(|m| m.face == face).collect();
                if marks.is_empty() {
                    continue;
                }
                let axis = usize::from(face / 2);
                let sign = if face.is_multiple_of(2) { 1 } else { -1 };
                let (_, tile) = surface(&k.style, axis, sign);
                let mut out = *cell;
                out[axis] += sign;
                let lit = flood.cell(out);
                push_relief(m, &relief_quad(*cell, face, &marks, &look, fraction, &tile), &tile, lit);
            }
            continue;
        }
        for (i, mark) in view.marks.iter().enumerate() {
            if mark.face > 5 {
                continue;
            }
            let (_, tile) = surface(&k.style, usize::from(mark.face), 1);
            let axis = usize::from(mark.face / 2);
            let sign = if mark.face.is_multiple_of(2) { 1 } else { -1 };
            let mut out = *cell;
            out[axis] += sign;
            let lit = flood.cell(out);
            for d in decal(*cell, mark, &look, fraction, i) {
                match d.normal {
                    Some(n) => push_polygon_shaded(m, &d.pts, n, d.color, &tile, lit, 1.0),
                    None => push_polygon(m, &d.pts, d.color, &tile, lit, 1.0),
                }
            }
        }
    }
}

/// A scoop's relief quad (BLOCK_DAMAGE.md §6.1): its corners (cell space, rows up, just off the
/// face, counter-clockwise seen from outside), their uvs (the face's own, so the texture meets the
/// face's), its normal and its parameters packed for the shader (`relief::pack`).
#[derive(Debug)]
struct ReliefQuad {
    pts: [[f64; 3]; 4],
    uvs: [[f64; 2]; 4],
    normal: [f64; 3],
    color: [f32; 3],
    light: f32,
    /// The marks' centres (cell fractions along the face's u and v), the radius and depth (m).
    #[cfg_attr(not(test), allow(dead_code))]
    centres: Vec<[f64; 2]>,
    #[cfg_attr(not(test), allow(dead_code))]
    radius: f64,
    #[cfg_attr(not(test), allow(dead_code))]
    depth: f64,
}

fn relief_quad(
    cell: [i32; 3],
    face: u8,
    marks: &[&HitMark],
    look: &DamageLook,
    fraction: f64,
    tile: &TileRect,
) -> ReliefQuad {
    let axis = usize::from(face / 2);
    let positive = face.is_multiple_of(2);
    // The face's texture axes as the face's quad lays them (mesher.rs `quad`): tops in x, z; sides
    // along then up; a turned tile swaps them.
    let mut ax = match axis {
        1 => [0, 2],
        0 => [2, 1],
        _ => [0, 1],
    };
    if tile.turn {
        ax.swap(0, 1);
    }
    // A cell's extent along an axis (m).
    let ext = |a: usize| if a == 1 { CELL_H } else { 1.0 };
    let grow = 0.55 + 0.45 * fraction;
    let radius = 0.5 * look.size * grow;
    let depth = look.depth * grow;
    let centres: Vec<[f64; 2]> = marks
        .iter()
        .take(4)
        .map(|m| {
            std::array::from_fn(|i| {
                let a = ax[i];
                let ra = (radius / ext(a)).min(0.5);
                (f64::from(m.point[a]) / 255.0).clamp(ra, 1.0 - ra)
            })
        })
        .collect();
    // The quad: the marks' reach, inside the cell.
    let reach = |i: usize| relief::EXTENT * radius / ext(ax[i]);
    let lo: [f64; 2] = std::array::from_fn(|i| {
        centres.iter().map(|c| c[i] - reach(i)).fold(1.0, f64::min).max(0.005)
    });
    let hi: [f64; 2] = std::array::from_fn(|i| {
        centres.iter().map(|c| c[i] + reach(i)).fold(0.0, f64::max).min(0.995)
    });
    // Corners counter-clockwise seen from outside: e_u × e_v = e_axis (u, v the next axes).
    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
    let plane = f64::from(cell[axis]) + if positive { 1.0 + LIFT } else { -LIFT };
    let corner = |su: f64, sv: f64| {
        // su, sv: 0 or 1 along the next axes u, v.
        let mut f = [0.0; 3];
        f[u] = su;
        f[v] = sv;
        let pick = |a: usize, i: usize| if f[a] > 0.5 { hi[i] } else { lo[i] };
        let mut p = [0.0; 3];
        p[axis] = plane;
        for a in [u, v] {
            let i = usize::from(a == ax[1]);
            p[a] = f64::from(cell[a]) + pick(a, i);
        }
        p
    };
    let order: [(f64, f64); 4] = if positive {
        [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]
    } else {
        [(0.0, 0.0), (0.0, 1.0), (1.0, 1.0), (1.0, 0.0)]
    };
    let pts = order.map(|(a, b)| corner(a, b));
    // The face's own uvs (mesher.rs `quad`): blocks across, metres up a side (a hanging tile from
    // the cell's top).
    let top = f64::from(cell[1] + 1);
    let uv_of = |a: usize, p: &[f64; 3]| {
        if a == 1 {
            if tile.hang { (p[1] - top) * CELL_H + top } else { p[1] * CELL_H }
        } else {
            p[a]
        }
    };
    let uvs = pts.map(|p| [uv_of(ax[0], &p), uv_of(ax[1], &p)]);
    let mut normal = [0.0; 3];
    normal[axis] = if positive { 1.0 } else { -1.0 };
    let (color, light) = relief::pack(
        &centres,
        [ax[0] == 1, ax[1] == 1],
        look.parallax,
        look.darken,
        look.rim,
        radius,
        depth,
    );
    ReliefQuad { pts, uvs, normal, color, light, centres, radius, depth }
}

/// Appends a relief quad: its parameters in the colour and light (the shader reads them flat).
fn push_relief(m: &mut MeshArrays, q: &ReliefQuad, tile: &TileRect, lit: [f32; 2]) {
    let base = (m.positions.len() / 3) as u32;
    for (p, uv) in q.pts.iter().zip(q.uvs) {
        m.positions.extend(p.map(|x| x as f32));
        m.normals.extend(q.normal.map(|x| x as f32));
        m.colors.extend(q.color);
        m.uvs.extend(uv.map(|x| x as f32));
        m.tiles.extend([
            tile.u0 as f32,
            tile.v0 as f32,
            (tile.u1 - tile.u0) as f32,
            (tile.v1 - tile.v0) as f32,
        ]);
        m.light.push(q.light);
        m.flood.extend(lit);
    }
    m.indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// One piece of a decal: its polygon as drawn (cell space, rows up, just off the face), its colour
/// (a linear multiplier of the tile), and the normal it is lit by (metres; `None`: its own).
#[derive(Debug)]
struct Piece {
    pts: Vec<[f64; 3]>,
    color: [f64; 3],
    normal: Option<[f64; 3]>,
    /// The piece as modelled (metres, cut into the face): `pts` before they are laid flat.
    #[cfg_attr(not(test), allow(dead_code))]
    model: Vec<[f64; 3]>,
}

/// A decal's pieces.
fn decal(
    cell: [i32; 3],
    mark: &HitMark,
    look: &DamageLook,
    fraction: f64,
    index: usize,
) -> Vec<Piece> {
    let axis = usize::from(mark.face / 2);
    let positive = mark.face.is_multiple_of(2);
    // In-face axes, (u, v) with u × v the face's outward normal.
    let (mut u, mut v) = match axis {
        0 => (1, 2),
        1 => (2, 0),
        _ => (0, 1),
    };
    if !positive {
        std::mem::swap(&mut u, &mut v);
    }
    // Metres to cell space per axis (a cell is CELL_H tall).
    let scale = |a: usize| if a == 1 { 1.0 / CELL_H } else { 1.0 };
    let grow = 0.55 + 0.45 * fraction;
    let r = 0.5 * look.size * grow;
    let mut centre = [0.0; 3];
    for a in 0..3 {
        let p = f64::from(mark.point[a]) / 255.0;
        let ra = (r * scale(a)).min(0.5);
        centre[a] = f64::from(cell[a]) + p.clamp(ra, 1.0 - ra);
    }
    centre[axis] = f64::from(cell[axis]) + if positive { 1.0 + LIFT } else { -LIFT };
    // A stable jitter per mark (its point and order), so a remesh draws it the same.
    let seed = (u32::from(mark.point[0]).wrapping_mul(73_856_093))
        ^ (u32::from(mark.point[1]).wrapping_mul(19_349_663))
        ^ (u32::from(mark.point[2]).wrapping_mul(83_492_791))
        ^ (index as u32).wrapping_mul(2_654_435_761);
    let jitter = |k: u32| {
        let h = (seed ^ k.wrapping_mul(0x9E37_79B9)).wrapping_mul(0x85EB_CA6B);
        f64::from(h >> 8) / f64::from(1u32 << 24)
    };
    let at = |radius: f64, theta: f64, lift: f64| {
        let mut p = centre;
        p[u] += radius * theta.cos() * scale(u);
        p[v] += radius * theta.sin() * scale(v);
        p[axis] += if positive { lift } else { -lift };
        // Kept on the face (a crack stops at its edge).
        for a in [u, v] {
            p[a] = p[a].clamp(f64::from(cell[a]) + 0.01, f64::from(cell[a]) + 0.99);
        }
        p
    };
    let srgb = |c: [u8; 3], mul: f64| c.map(|x| (f64::from(x) / 255.0 * mul).min(1.6));
    let turn = jitter(99) * std::f64::consts::TAU;
    let flat = |pts: Vec<[f64; 3]>, color: [f64; 3]| Piece {
        model: Vec::new(),
        pts,
        color,
        normal: None,
    };
    // A point `depth` metres into the face (negative: proud of it), modelled in metres: the drawn
    // point back on the face, off it by LIFT, the cut along the inward normal.
    let out_sign = if positive { 1.0 } else { -1.0 };
    let model = |p: [f64; 3], depth: f64| {
        let mut q = p;
        q[axis] -= out_sign * LIFT;
        q[1] *= CELL_H;
        q[axis] -= out_sign * depth;
        q
    };
    // A piece drawn flat, lit as its modelled shape (each corner its depth).
    let cut = |pts: Vec<([f64; 3], f64)>, color: [f64; 3]| {
        let model: Vec<_> = pts.iter().map(|&(p, d)| model(p, d)).collect();
        let normal = Some(newell(&model));
        Piece {
            pts: pts.into_iter().map(|(p, _)| p).collect(),
            color,
            normal,
            model,
        }
    };
    let depth = look.depth * grow;
    let mut out = Vec::new();
    match look.kind {
        DamageLookKind::Shards => {
            // Angular facets: five uneven corners round an off-centre apex knocked `depth` in (or,
            // now and then, a flake left proud), each facet lit at its own slant in a fresher tone.
            let corners: Vec<_> = (0..5)
                .map(|k| {
                    let th = turn + f64::from(k) * std::f64::consts::TAU / 5.0;
                    at(r * (0.65 + 0.45 * jitter(k)), th, 0.0)
                })
                .collect();
            let off = jitter(7) * std::f64::consts::TAU;
            let apex = at(r * 0.25 * jitter(8), off, 0.0);
            let d = if jitter(9) < 0.8 { depth } else { -0.5 * depth };
            for k in 0..5 {
                let (a, b) = (corners[k], corners[(k + 1) % 5]);
                let shade = 1.2 + 0.25 * jitter(30 + k as u32);
                out.push(cut(
                    vec![(apex, d), (a, 0.0), (b, 0.0)],
                    srgb(look.color, shade),
                ));
            }
        }
        // Drawn per pixel instead (`relief_quad`).
        DamageLookKind::Scoops => {}
        DamageLookKind::Chips => {
            for k in 0..3 {
                let th = turn + f64::from(k) * 2.1;
                let c = at(r * 0.5, th, 0.0);
                let tri = (0..3)
                    .map(|j| {
                        let a = th + f64::from(j) * std::f64::consts::TAU / 3.0;
                        let mut p = c;
                        p[u] += 0.3 * r * a.cos() * scale(u);
                        p[v] += 0.3 * r * a.sin() * scale(v);
                        p
                    })
                    .collect();
                out.push(flat(tri, srgb(look.color, 1.2)));
            }
        }
    }
    // Cracks that grow with the damage: thin wedges out from the mark (not in dug soil).
    if fraction > 0.2 && look.kind != DamageLookKind::Scoops {
        // Toned down after the first look (thinner, shorter, a softer colour: the storybook style).
        let len = r * (0.6 + 1.1 * fraction);
        for k in 0..3 {
            let th = turn + 0.3 + f64::from(k) * (2.0 + 0.4 * jitter(40 + k));
            let w = 0.008 + 0.006 * fraction;
            let a = at(r * 0.5, th - w / (r * 0.5).max(0.05), 0.002);
            let b = at(len, th, 0.002);
            let c = at(r * 0.5, th + w / (r * 0.5).max(0.05), 0.002);
            out.push(flat(vec![a, b, c], srgb(look.crack, 1.0)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(face: u8, fraction: u8) -> DamageView {
        DamageView {
            fraction,
            marks: vec![HitMark {
                face,
                point: [128, 255, 64],
            }],
        }
    }

    dwell_world::both_faces!(
        fn a_marks_decal_sits_on_its_face_and_cracks_grow(f: dwell_world::FaceFrame) {
            // A stone cell's top struck, face-local (+y up), placed through the face's frame.
            let mirror = f == dwell_world::FaceFrame::B;
            let up = if mirror { 3 } else { 2 };
            let world = [3, if mirror { N - 1 - 5 } else { 5 }, 4];
            let (cell, v) = face_local(world, &view(up, 40), mirror);
            assert_eq!(cell, [3, 5, 4], "face-local, as face A's");
            assert_eq!(v.marks[0].face, 2, "the up face, face-locally");
            let look = look_of("dwell:stone").expect("stone's damage look is data");
            let light = decal(cell, &v.marks[0], &look, 40.0 / 255.0, 0);
            let (_, heavy_v) = face_local(world, &view(up, 230), mirror);
            let heavy = decal(cell, &heavy_v.marks[0], &look, 230.0 / 255.0, 0);
            assert!(heavy.len() > light.len(), "cracks show as the damage grows");
            for piece in &heavy {
                for p in &piece.pts {
                    assert!(p[1] > 6.0 && p[1] < 6.02, "just over the top: {p:?}");
                    assert!((3.0..=4.0).contains(&p[0]) && (4.0..=5.0).contains(&p[2]));
                }
            }
        }
    );

    dwell_world::both_faces!(
        /// A shovel's scoop is one flat quad per struck face whose hollow the shader computes
        /// (BLOCK_DAMAGE.md §6.1): just off the face, inside the cell, the face's own uvs (so the
        /// texture meets the face's), its parameters packed for the shader, deeper and wider as
        /// the damage grows, as deep as its data says; two hits on a face share one quad (one
        /// hollow). On the top and on a side, face-locally on both faces.
        fn a_scoop_is_one_relief_quad_per_struck_face(f: dwell_world::FaceFrame) {
            let mirror = f == dwell_world::FaceFrame::B;
            let look = look_of("dwell:dirt").expect("dirt's damage look is data");
            let tile = crate::textures::tile_rect("plain");
            for (local, world_face) in [(2u8, if mirror { 3 } else { 2 }), (0, 0)] {
                let world = [3, if mirror { N - 1 - 5 } else { 5 }, 4];
                let mark = |p: [u8; 3]| HitMark { face: world_face, point: p };
                let view = DamageView { fraction: 0, marks: vec![mark([128, 128, 128]), mark([90, 100, 170])] };
                let (cell, v) = face_local(world, &view, mirror);
                assert!(v.marks.iter().all(|m| m.face == local));
                let axis = usize::from(local / 2);
                let marks: Vec<_> = v.marks.iter().collect();
                let quad = |fraction: f64| relief_quad(cell, local, &marks, &look, fraction, &tile);
                let (light, heavy) = (quad(0.0), quad(1.0));
                assert_eq!(heavy.centres.len(), 2, "both hits in one hollow");
                assert!((heavy.depth - look.depth).abs() < 1e-9, "full damage: the data's depth");
                assert!(light.depth < heavy.depth && light.radius < heavy.radius);
                assert!(heavy.normal[axis] == 1.0);
                for (p, uv) in heavy.pts.iter().zip(heavy.uvs) {
                    let off = p[axis] - (f64::from(cell[axis]) + 1.0);
                    assert!(off > 0.0 && off < 0.01, "just off the face: {off}");
                    for a in 0..3 {
                        if a != axis {
                            assert!(p[a] > f64::from(cell[a]) && p[a] < f64::from(cell[a] + 1), "inside the cell: {p:?}");
                        }
                    }
                    // The face's uvs: blocks across, metres up a side (mesher.rs `quad`).
                    let want = if axis == 1 { [p[0], p[2]] } else { [p[2], p[1] * CELL_H] };
                    assert!((uv[0] - want[0]).abs() < 1e-9 && (uv[1] - want[1]).abs() < 1e-9, "{uv:?} {want:?}");
                }
                let (back, ..) = relief::unpack(heavy.color, heavy.light);
                assert_eq!(back.len(), 2);
                assert!(heavy.color[2] < -0.5 && heavy.color[1] >= 0.0, "a relief quad's code");
            }
        }
    );

    #[test]
    fn soils_scoop_and_stone_shards_by_data() {
        // both-faces: data only, no up.
        assert_eq!(look_of("dwell:dirt").unwrap().kind, DamageLookKind::Scoops);
        assert_eq!(look_of("dwell:sand").unwrap().kind, DamageLookKind::Scoops);
        assert_eq!(look_of("dwell:stone").unwrap().kind, DamageLookKind::Shards);
        assert_eq!(look_of("dwell:leaves"), None);
    }
}
