//! Block damage drawn on the struck faces (docs/BLOCK_DAMAGE.md, D2; look only, collision
//! unchanged): at each hit mark a shard knocked off (stone: an angular facet in a fresher, lighter
//! tone), a scoop (soils: a round, darker bowl) or chips, and cracks that grow with the damage, as
//! decals just off the face in the chunk's opaque mesh. Per block, from its `damage.look` data.
//!
//! A scoop and a shard are modelled cut into the face, `depth` deep (a bowl of rings, a fan of
//! angular facets), and drawn flat just off it, each piece lit by its modelled normal: the face's
//! own quad stays whole (greedy-meshed, collision unchanged), and the dent reads by its shading.

use super::{
    Kinds, MeshArrays, N,
    ground_mesh::{newell, push_polygon, push_polygon_shaded},
    surface,
};
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
        DamageLookKind::Scoops => {
            // A round bowl, a spherical cap `depth` deep: rings of quads from its damp rim down to
            // its floor, darker at the rim, each lit at its slant, so the far wall catches the
            // light and the near one falls into shadow.
            const SEG: u32 = 12;
            let rings = [1.0, 0.72, 0.42];
            let ring = |t: f64, k: u32| {
                let th = turn + f64::from(k % SEG) * std::f64::consts::TAU / f64::from(SEG);
                let wobble = 0.92 + 0.12 * jitter(20 + k % SEG);
                at(r * t * wobble, th, 0.0)
            };
            let cap = |t: f64| depth * (1.0 - t * t);
            // The look's colour as a hue (its brightest channel 1), so the bowl keeps the tile's
            // grain and reads by its shading: a dark damp rim, lighter walls and floor.
            let hue = |mul: f64| {
                let top = look.color.iter().copied().max().unwrap_or(1).max(1);
                look.color.map(|c| f64::from(c) / f64::from(top) * mul)
            };
            let tones = [0.5, 0.95];
            for (i, w) in rings.windows(2).enumerate() {
                for k in 0..SEG {
                    let pts = vec![
                        (ring(w[0], k), cap(w[0])),
                        (ring(w[0], k + 1), cap(w[0])),
                        (ring(w[1], k + 1), cap(w[1])),
                        (ring(w[1], k), cap(w[1])),
                    ];
                    out.push(cut(pts, hue(tones[i])));
                }
            }
            // The floor: a fan to the deepest point.
            let last = rings[rings.len() - 1];
            for k in 0..SEG {
                let pts = vec![
                    (at(0.0, 0.0, 0.0), depth),
                    (ring(last, k), cap(last)),
                    (ring(last, k + 1), cap(last)),
                ];
                out.push(cut(pts, hue(0.85)));
            }
        }
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
    // Cracks that grow with the damage: thin wedges out from the mark.
    if fraction > 0.2 {
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
        /// A shovel's scoop is a bowl cut into the face (BLOCK_DAMAGE.md §6): drawn flat on the
        /// face, modelled behind it, deepest at its middle, and each wall lit as it slopes, tilted
        /// toward the middle (concave); deeper as the damage grows, as deep as its data says. On the
        /// top and on a side, face-locally on both faces.
        fn a_scoop_is_a_bowl_recessed_into_the_face(f: dwell_world::FaceFrame) {
            let mirror = f == dwell_world::FaceFrame::B;
            let look = look_of("dwell:dirt").expect("dirt's damage look is data");
            assert!(look.depth > 0.0, "a scoop's depth is data");
            // The up face (face-locally 2) and the +x side (0), struck mid-face.
            for (local, world_face) in [(2u8, if mirror { 3 } else { 2 }), (0, 0)] {
                let world = [3, if mirror { N - 1 - 5 } else { 5 }, 4];
                let mark = DamageView {
                    fraction: 0,
                    marks: vec![HitMark {
                        face: world_face,
                        point: [128, 128, 128],
                    }],
                };
                let (cell, v) = face_local(world, &mark, mirror);
                assert_eq!(v.marks[0].face, local);
                let axis = usize::from(local / 2);
                // The face's plane in metres (positive faces: the cell's far side).
                let plane = (f64::from(cell[axis]) + 1.0) * if axis == 1 { CELL_H } else { 1.0 };
                let deepest = |fraction: f64| {
                    let ps = decal(cell, &v.marks[0], &look, fraction, 0);
                    let cut: Vec<_> = ps.iter().filter(|p| p.normal.is_some()).collect();
                    assert!(
                        cut.len() >= 24,
                        "a bowl of rings and a floor: {}",
                        cut.len()
                    );
                    let mut deepest: f64 = 0.0;
                    for p in &cut {
                        for q in &p.model {
                            let d = plane - q[axis];
                            assert!(d >= -1e-9, "modelled behind the face, never proud: {d}");
                            deepest = deepest.max(d);
                        }
                        // Lit as it slopes, toward the bowl's middle (concave).
                        let n = p.normal.unwrap();
                        assert!(
                            n[axis] > 0.5 && n[axis] < 1.0 - 1e-6,
                            "a slanted wall: {n:?}"
                        );
                        let mid: [f64; 3] = std::array::from_fn(|a| {
                            p.model.iter().map(|q| q[a]).sum::<f64>() / p.model.len() as f64
                        });
                        let centre = [3.5, 5.5 * CELL_H, 4.5];
                        let inward: f64 = (0..3)
                            .filter(|&a| a != axis)
                            .map(|a| n[a] * (centre[a] - mid[a]))
                            .sum();
                        assert!(
                            inward > 0.0,
                            "a wall faces the bowl's middle: {n:?} at {mid:?}"
                        );
                        for q in &p.pts {
                            let off = q[axis] - (f64::from(cell[axis]) + 1.0);
                            assert!(off > 0.0 && off < 0.01, "drawn just off the face: {off}");
                        }
                    }
                    deepest
                };
                let (light, heavy) = (deepest(0.0), deepest(1.0));
                assert!(
                    (heavy - look.depth).abs() < 1e-9,
                    "full damage: the data's depth"
                );
                assert!(light > 0.0 && light < heavy, "deeper as the damage grows");
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
