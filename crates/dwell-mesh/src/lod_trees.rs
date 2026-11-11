//! Trees at LOD (docs/TREE_GROWTH.md §10, WORK_QUEUE item 5: "the ones directly beyond high detail
//! look nothing like the main trees"): a section's leaf cells (shared/look/leaves.json `blocks`) and
//! log cells (block data `crossSection`) are drawn whole, as the chunk mesher draws them at their
//! coarser cell: leaves chamfered where exposed, their normals and face shading leaning out of the
//! crown (`mesher::leaves`, the same `Crowns` over the section's grid), logs as octagonal columns.
//! The colours are the LOD's flat tile averages, a leaf's tinted by its tree (ADR 0079). Face-local, like the rest of the section mesher.

use std::sync::OnceLock;

use dwell_blocks::{STATES, StateId};
use dwell_look::leaves::LEAVES;
use dwell_look::normal_tint;
use dwell_world::tree_tint::Tint;
use dwell_world::{CELL_H, normal_to_m};

use crate::mesher::leaves::{Crowns, Piece, canopy_normal, column_pieces, pieces};
use crate::style::material_style;

const N: i32 = dwell_world::lod::SECTION_CELLS;
/// An octagon's corner cut as a share of its side (ADR 0072's regular octagon).
const OCTAGON_CUT: f64 = 0.292_893_218_813_452_5; // 1 / (2 + √2)

/// What a state is to the tree pass: 0 neither, 1 a leaf, 2 a log.
pub(crate) fn tree_kind(m: StateId) -> u8 {
    static KINDS: OnceLock<Vec<u8>> = OnceLock::new();
    let of = |m: StateId| {
        let s = material_style(m);
        if s.leafy {
            1
        } else if s.octagon && s.look == crate::style::Look::Cube {
            2
        } else {
            0
        }
    };
    let t = KINDS.get_or_init(|| (0..STATES.len()).map(|i| of(i as StateId)).collect());
    t.get(usize::from(m)).copied().unwrap_or_else(|| of(m))
}

/// Whether `cells` (a padded chunk or a section) hold any leaf: the mesh jobs ask the generator
/// for the trees' tints (ADR 0079) only then.
pub fn has_leaves(cells: &[u16]) -> bool {
    cells.iter().any(|&m| tree_kind(m) == 1)
}

const FACES: [(usize, i32); 6] = [(0, 1), (0, -1), (1, 1), (1, -1), (2, 1), (2, -1)];

/// Draws the section's tree cells into `out` (positions in cells, normals in cell space, linear
/// colours), skipping columns' surface cells (`skip`), which the surface pass draws.
pub(crate) fn tree_cells(
    cells: &[u16],
    skip: impl Fn(i32, i32, i32) -> bool,
    color: impl Fn(StateId, usize) -> u32,
    out: &mut crate::lod_mesher::FlatMesh,
    tint: &dyn Fn(i32, i32, i32) -> Tint,
) {
    let at = |x: i32, y: i32, z: i32| cells[dwell_world::lod::cell_index(x, y, z)];
    let mut any = false;
    for y in 0..N {
        for z in 0..N {
            for x in 0..N {
                if tree_kind(at(x, y, z)) == 1 {
                    any = true;
                }
            }
        }
    }
    let crowns = if any {
        Crowns::from_fn(|x, y, z| tree_kind(at(x, y, z)) == 1)
    } else {
        None
    };
    let inside = |c: [i32; 3]| c.iter().all(|&v| (-1..=N).contains(&v));
    let nb = |i: usize, c: [i32; 3]| {
        let (a, s) = FACES[i];
        let mut o = c;
        o[a] += s;
        o
    };
    let exposed_at = |c: [i32; 3]| -> [bool; 6] {
        std::array::from_fn(|i| {
            let o = nb(i, c);
            if !inside(o) {
                return false;
            }
            let n = at(o[0], o[1], o[2]);
            let k = tree_kind(n);
            // Open beside air, a liquid, or a log (whose corners are open); a leaf, or any other
            // solid (a full cube at LOD), closes it.
            n == 0 || k == 2 || (k != 1 && crate::lod_mesher::is_liquid(n))
        })
    };
    for y in 0..N {
        for z in 0..N {
            for x in 0..N {
                let m = at(x, y, z);
                let kind = tree_kind(m);
                if kind == 0 || skip(x, y, z) {
                    continue;
                }
                let c = [x, y, z];
                let ex = exposed_at(c);
                let list = if kind == 1 {
                    let leafy: [bool; 6] = std::array::from_fn(|i| {
                        let o = nb(i, c);
                        inside(o) && tree_kind(at(o[0], o[1], o[2])) == 1
                    });
                    let nb_ex: [[bool; 6]; 6] = std::array::from_fn(|i| {
                        if leafy[i] {
                            exposed_at(nb(i, c))
                        } else {
                            [false; 6]
                        }
                    });
                    pieces(ex, leafy, nb_ex, LEAVES.chamfer)
                } else {
                    // A log's ends are closed by the log above and below; it is open on its sides
                    // where nothing solid stands, and drawn square where a log joins it.
                    let mut ex = ex;
                    for (i, e) in ex.iter_mut().enumerate() {
                        let o = nb(i, c);
                        if inside(o) && tree_kind(at(o[0], o[1], o[2])) == 2 {
                            *e = false;
                        }
                    }
                    column_pieces(ex, OCTAGON_CUT)
                };
                let canopy = if kind == 1 { crowns.as_ref() } else { None };
                // A leaf takes its tree's tint (ADR 0079), as the chunk mesher's leaves do.
                let t = if kind == 1 { tint(x, y, z) } else { Tint::NONE };
                let color = |m: StateId, g: usize| t.apply_rgb8(color(m, g));
                for p in &list {
                    emit(out, c, m, p, canopy, &color);
                }
            }
        }
    }
}

fn emit(
    out: &mut crate::lod_mesher::FlatMesh,
    c: [i32; 3],
    m: StateId,
    piece: &Piece,
    crowns: Option<&Crowns>,
    color: &impl Fn(StateId, usize) -> u32,
) {
    let n_m = normal_to_m(piece.normal);
    let dom = (0..3)
        .max_by(|&i, &j| n_m[i].abs().total_cmp(&n_m[j].abs()))
        .unwrap_or(1);
    let group = if dom == 1 {
        if n_m[1] >= 0.0 { 0 } else { 2 }
    } else {
        1
    };
    let col = color(m, group);
    let base = (out.positions.len() / 3) as u32;
    for &q in &piece.pts {
        let p = [
            f64::from(c[0]) + 0.5 + q[0],
            f64::from(c[1]) + 0.5 + q[1],
            f64::from(c[2]) + 0.5 + q[2],
        ];
        let n = match crowns {
            Some(cr) => canopy_normal(n_m, cr.outward(p), LEAVES.canopy_blend),
            None => n_m,
        };
        let tint = normal_tint(n[0], n[1], n[2]);
        out.positions.extend(p.map(|v| v as f32));
        // Stored in cell space: the section mesher turns normals into metres afterwards.
        let cell_n = {
            let v = [n[0], n[1] * CELL_H, n[2]];
            let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-12);
            v.map(|x| x / l)
        };
        out.normals.extend(cell_n.map(|v| v as f32));
        out.colors.extend(crate::lod_mesher::linear_rgb(col, tint));
    }
    for i in 1..piece.pts.len().saturating_sub(1) as u32 {
        out.indices.extend([base, base + i, base + i + 1]);
    }
}

#[cfg(test)]
mod tests {
    use crate::lod_mesher::{MeshSectionOptions, mesh_section, mesh_section_tinted};
    use crate::mesher::{ChunkFlags, mesh_chunk_flags};
    use dwell_blocks::require;
    use dwell_world::FaceFrame;
    use dwell_world::tree_tint::{Tint, TreeTint};

    const N: i32 = dwell_protocol::CHUNK_SIZE;

    /// A tree fixture in the 34³ padded layout (chunks' and sections' alike): a log column 8 cells
    /// tall and a 5 × 6 × 5 crown round its top, corners cut.
    fn tree() -> Vec<u16> {
        let (log, leaf) = (require("dwell:log"), require("dwell:leaves"));
        let mut cells = vec![0u16; dwell_world::lod::VOLUME];
        for y in 4..12 {
            cells[dwell_world::lod::cell_index(16, y, 16)] = log;
        }
        for y in 10..16 {
            for z in 14..19 {
                for x in 14..19 {
                    let corner = (x == 14 || x == 18) && (z == 14 || z == 18);
                    let at = dwell_world::lod::cell_index(x, y, z);
                    if !corner && cells[at] == 0 {
                        cells[at] = leaf;
                    }
                }
            }
        }
        cells
    }

    fn bounds(p: &[f32]) -> ([f32; 3], [f32; 3]) {
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for v in p.chunks_exact(3) {
            for i in 0..3 {
                lo[i] = lo[i].min(v[i]);
                hi[i] = hi[i].max(v[i]);
            }
        }
        (lo, hi)
    }

    dwell_world::both_faces!(
        /// At the LOD's finest cells a tree has the near tree's silhouette (its bounds, within
        /// 0.05 m: the chamfered crown, the octagon trunk), its leaf normals lean out of the crown,
        /// and its trunk has the octagon's diagonal sides, not a square's.
        fn a_lod_tree_has_the_near_trees_silhouette(f: FaceFrame) {
            let cells = tree();
            let near = mesh_chunk_flags(
                &cells,
                &[],
                f.is_b(),
                [0; 3],
                &[],
                None,
                ChunkFlags {
                    water_tops: true,
                    cards: false,
                },
            )
            .expect("padded");
            let far = mesh_section(
                &cells,
                &MeshSectionOptions {
                    mirror: f.is_b(),
                    ..Default::default()
                },
            )
            .expect("cells");
            let mut near_p = near.opaque.positions.clone();
            near_p.extend(&near.cutout.positions);
            let (nlo, nhi) = bounds(&near_p);
            let (flo, fhi) = bounds(&far.opaque.positions);
            for i in 0..3 {
                assert!((nlo[i] - flo[i]).abs() < 0.05, "lo {nlo:?} vs {flo:?}");
                assert!((nhi[i] - fhi[i]).abs() < 0.05, "hi {nhi:?} vs {fhi:?}");
            }
            let normals: Vec<&[f32]> = far.opaque.normals.chunks_exact(3).collect();
            let leaning = normals
                .iter()
                .filter(|n| n.iter().all(|c| c.abs() < 0.95))
                .count();
            assert!(leaning > 20, "{leaning}");
            let diagonal = normals.iter().any(|n| {
                n[1].abs() < 1e-3
                    && (n[0].abs() - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.01
                    && (n[2].abs() - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.01
            });
            assert!(diagonal, "the trunk is an octagon at LOD");
        }
    );

    /// The tree fixture with a lone placed leaf beside it, as face `f`'s world cells (face B's rows
    /// reversed: the meshers mirror them back), and the leaf's column.
    fn tree_and_placed_leaf(f: FaceFrame) -> (Vec<u16>, i32) {
        let mut local = tree();
        local[dwell_world::lod::cell_index(4, 20, 4)] = require("dwell:leaves");
        let cells = if f.is_b() {
            crate::lod_mesher::mirror_rows(&local)
        } else {
            local
        };
        (cells, 4)
    }

    const DARKER: Tint = Tint {
        hue: 0.0,
        lightness: 0.8,
    };

    /// Per vertex: the tinted mesh's colours against the plain one's (same geometry), and whether
    /// any vertex in the placed leaf's column `x0` (positions in metres) changed.
    fn compare(plain: &[f32], tinted: &[f32], pos: &[f32], x0: i32) -> (usize, usize) {
        assert_eq!(plain.len(), tinted.len(), "the tint changes no geometry");
        let (mut darker, mut placed) = (0, 0);
        for (i, (a, b)) in plain
            .chunks_exact(3)
            .zip(tinted.chunks_exact(3))
            .enumerate()
        {
            let x = pos[i * 3];
            if a == b {
                continue;
            }
            assert!(
                (0..3).all(|c| b[c] <= a[c]) && b.iter().sum::<f32>() < a.iter().sum::<f32>(),
                "a tinted vertex is darker: {a:?} → {b:?}"
            );
            darker += 1;
            if x >= x0 as f32 - 0.01 && x <= x0 as f32 + 1.01 {
                placed += 1;
            }
        }
        (darker, placed)
    }

    dwell_world::both_faces!(
        /// Per-tree leaf tints (ADR 0079): a tree's leaves and cards take its tint near (the chunk
        /// mesher, the tree listed face-local) and far (the section mesher, its cells listed in
        /// the section's rows), its trunk does not, and a placed leaf no tree owns keeps the
        /// plain colour.
        fn a_trees_tint_darkens_its_leaves_near_and_far_but_not_a_placed_leaf(f: FaceFrame) {
            use crate::mesher::mesh_chunk_trees;
            let (cells, x0) = tree_and_placed_leaf(f);
            // The chunk meshed is face-local chunk (0, 3, 0): the tree's anchor and bounds there.
            let local = dwell_world::ChunkCoord::new(0, 3, 0);
            let world = f.chunk(local);
            let origin = [world.x * N, world.y * N, world.z * N];
            let y0 = local.y * N;
            let oak = TreeTint {
                anchor: [16, y0 + 4, 16],
                min: [14, y0 + 4, 14],
                max: [18, y0 + 15, 18],
                tint: DARKER,
            };
            let flags = ChunkFlags {
                water_tops: true,
                cards: true,
            };
            let near = |trees: &[TreeTint]| {
                mesh_chunk_trees(&cells, &[], f.is_b(), origin, &[], None, flags, &[], trees)
                    .expect("padded")
            };
            let (plain, tinted) = (near(&[]), near(&[oak]));
            assert_eq!(plain.opaque, tinted.opaque, "the trunk keeps its colour");
            let (darker, placed) = compare(
                &plain.cutout.colors,
                &tinted.cutout.colors,
                &plain.cutout.positions,
                x0,
            );
            assert!(darker > 100, "the crown and its cards are tinted: {darker}");
            assert_eq!(placed, 0, "the placed leaf is not");
            // The cards too: their stream's vertices (bare 1) are among the darker.
            let cards_darker = plain
                .cutout
                .bare
                .iter()
                .enumerate()
                .filter(|&(i, &b)| {
                    b == 1.0
                        && plain.cutout.colors[i * 3..i * 3 + 3]
                            != tinted.cutout.colors[i * 3..i * 3 + 3]
                })
                .count();
            assert!(cards_darker > 0, "the cards take the tint");

            // Far: every leaf of the crown listed in the section's own (world) rows.
            let leaf = require("dwell:leaves");
            let tints: Vec<(u32, Tint)> = (0..cells.len())
                .filter(|&i| cells[i] == leaf && (i % 34) as i32 - 1 != x0)
                .map(|i| (i as u32, DARKER))
                .collect();
            let options = MeshSectionOptions {
                mirror: f.is_b(),
                ..Default::default()
            };
            let plain = mesh_section(&cells, &options).expect("cells");
            let tinted = mesh_section_tinted(&cells, &options, &tints).expect("cells");
            let (darker, placed) = compare(
                &plain.opaque.colors,
                &tinted.opaque.colors,
                &plain.opaque.positions,
                x0,
            );
            assert!(darker > 50, "the far crown is tinted: {darker}");
            assert_eq!(placed, 0, "the far placed leaf is not");
        }
    );
}
