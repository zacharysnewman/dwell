//! Block damage drawn on the struck faces (docs/BLOCK_DAMAGE.md, D2; look only, collision
//! unchanged): at each hit mark a shard knocked off (stone: an angular facet in a fresher, lighter
//! tone), a scoop (soils: a round, darker bowl) or chips, and cracks that grow with the damage, as
//! decals just off the face in the chunk's opaque mesh. Per block, from its `damage.look` data.

use super::{Kinds, MeshArrays, N, ground_mesh::push_polygon, surface};
use crate::flood::Flood;
use dwell_protocol::damage::{DamageLook, DamageLookKind, DamageView, HitMark, look_of};
use dwell_world::CELL_H;

/// How far the decals stand off the face (cells): over the face, under anything in front.
const LIFT: f64 = 0.004;

/// A damaged cell turned face-local: a face-B chunk is meshed with its rows turned over, so the
/// cell's row, its marks' up and down faces and their points' heights turn over too.
pub(crate) fn face_local(cell: [i32; 3], view: &DamageView, mirror: bool) -> ([i32; 3], DamageView) {
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
            let sign = if mark.face % 2 == 0 { 1 } else { -1 };
            let mut out = *cell;
            out[axis] += sign;
            let lit = flood.cell(out);
            for poly in decal(*cell, mark, &look, fraction, i) {
                push_polygon(m, &poly.0, poly.1, &tile, lit, 1.0);
            }
        }
    }
}

/// A decal's polygons (cell space, rows up) and their colours (linear multipliers of the tile).
fn decal(
    cell: [i32; 3],
    mark: &HitMark,
    look: &DamageLook,
    fraction: f64,
    index: usize,
) -> Vec<(Vec<[f64; 3]>, [f64; 3])> {
    let axis = usize::from(mark.face / 2);
    let positive = mark.face % 2 == 0;
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
    centre[axis] = f64::from(cell[axis])
        + if positive { 1.0 + LIFT } else { -LIFT };
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
    let mut out = Vec::new();
    match look.kind {
        DamageLookKind::Shards => {
            // An angular facet: five uneven corners, a fresher tone, a darker inner facet.
            let pts: Vec<_> = (0..5)
                .map(|k| {
                    let th = turn + f64::from(k) * std::f64::consts::TAU / 5.0;
                    at(r * (0.65 + 0.45 * jitter(k)), th, 0.0)
                })
                .collect();
            out.push((pts, srgb(look.color, 1.35)));
            let inner: Vec<_> = (0..3)
                .map(|k| {
                    let th = turn + 0.6 + f64::from(k) * std::f64::consts::TAU / 3.0;
                    at(r * (0.35 + 0.2 * jitter(10 + k)), th, 0.001)
                })
                .collect();
            out.push((inner, srgb(look.color, 0.95)));
        }
        DamageLookKind::Scoops => {
            // A round, shallow bowl: a damp rim, its darker middle.
            let ring = |radius: f64, lift: f64| -> Vec<[f64; 3]> {
                (0..12)
                    .map(|k| {
                        let th = turn + f64::from(k) * std::f64::consts::TAU / 12.0;
                        at(radius * (0.92 + 0.12 * jitter(20 + k)), th, lift)
                    })
                    .collect()
            };
            out.push((ring(r, 0.0), srgb(look.color, 1.2)));
            out.push((ring(r * 0.6, 0.001), srgb(look.color, 0.8)));
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
                out.push((tri, srgb(look.color, 1.2)));
            }
        }
    }
    // Cracks that grow with the damage: thin wedges out from the mark.
    if fraction > 0.2 {
        let len = r * (0.8 + 1.6 * fraction);
        for k in 0..3 {
            let th = turn + 0.3 + f64::from(k) * (2.0 + 0.4 * jitter(40 + k));
            let w = 0.018 + 0.012 * fraction;
            let a = at(r * 0.5, th - w / (r * 0.5).max(0.05), 0.002);
            let b = at(len, th, 0.002);
            let c = at(r * 0.5, th + w / (r * 0.5).max(0.05), 0.002);
            out.push((vec![a, b, c], srgb(look.crack, 1.0)));
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
            for (poly, _) in &heavy {
                for p in poly {
                    assert!(p[1] > 6.0 && p[1] < 6.02, "just over the top: {p:?}");
                    assert!((3.0..=4.0).contains(&p[0]) && (4.0..=5.0).contains(&p[2]));
                }
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
