//! A damaged cell's faces and their cracks (docs/BLOCK_DAMAGE.md §6.3, §15): a damaged cube's
//! visible faces leave the greedy merge as [`StruckFace`]s, drawn as the pieces its damage mask
//! keeps (`damage_mask`); every damaged cell's faces carry the crack overlay once staged
//! ([`CrackLayer`]). Look only: collision stays the full block.

use super::N;
use crate::textures::TileRect;
use dwell_protocol::damage::{DamageLook, DamageView, HitMark};
use dwell_world::CELL_H;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesher::MeshArrays;
    use crate::mesher::{ChunkFlags, PADDED_VOLUME, mesh_chunk_damaged, padded_index};
    use dwell_protocol::damage::look_of;
    use dwell_world::FaceFrame;

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

    fn local_view(marks: &[(u8, [u8; 3])], fraction: u8) -> DamageView {
        DamageView {
            fraction,
            marks: marks
                .iter()
                .map(|&(face, point)| HitMark::at(face, point))
                .collect(),
        }
    }

    #[test]
    fn the_bite_rules_cell_is_the_worlds() {
        // face-a-only: a volume.
        assert_eq!(dwell_protocol::damage::CELL_M3, CELL_H);
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
                    let face = 2 * a + usize::from(n[a] < 0.0);
                    // The bottom lies on the stone below: not drawn (a wall of the chunks taken
                    // out may look down, inside the cell).
                    assert!(
                        face != 3 || p[1] > 6.0 * CELL_H + 1e-4,
                        "the hidden bottom is not drawn: {p:?}"
                    );
                    faces.insert(face);
                }
            }
            // +X, −X, +Y (struck), +Z, −Z.
            for want in [0, 1, 2, 4, 5] {
                assert!(
                    faces.contains(&want),
                    "every exposed face: {want} of {faces:?}"
                );
            }
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
}
