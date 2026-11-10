//! The heightfield's tile fill, corners, grid and atlas (proposals/WATER_GPU_SURFACE.md §3, C6a).

use super::*;
use crate::{ChunkFlags, MeshArrays, PADDED_VOLUME, mesh_chunk_flags, mirror_padded, padded_index};
use dwell_blocks::{liquid_at_fill, require};
use dwell_world::CELL_H;

fn water() -> u16 {
    require("dwell:water")
}
fn stone() -> u16 {
    require("dwell:stone")
}

fn voxels(cells: &[(i32, i32, i32, u16)]) -> Vec<u16> {
    let mut out = vec![0u16; PADDED_VOLUME];
    for &(x, y, z, m) in cells {
        out[padded_index(x, y, z)] = m;
    }
    out
}

fn level(levels: &[f32], x: i32, z: i32) -> f32 {
    levels[(x + 1) as usize + TILE * (z + 1) as usize]
}

/// A stone basin over the whole padded floor (y 0..=2 stone, walls at the apron) with water at
/// y 3 filled to `fill` (one column at `odd` instead, if given).
fn basin(fill: f64, odd: Option<(i32, i32, f64)>) -> Vec<(i32, i32, i32, u16)> {
    let mut cells = Vec::new();
    let n = COLUMNS as i32;
    for z in -1..=n {
        for x in -1..=n {
            for y in 0..3 {
                cells.push((x, y, z, stone()));
            }
            let wall = x == -1 || z == -1 || x == n || z == n;
            let f = match odd {
                Some((ox, oz, f)) if (ox, oz) == (x, z) => f,
                _ => fill,
            };
            let m = if wall {
                stone()
            } else {
                liquid_at_fill(water(), f)
            };
            cells.push((x, 3, z, m));
            if wall {
                cells.push((x, 4, z, stone()));
            }
        }
    }
    cells
}

#[test]
fn levels_come_from_the_eighths() {
    let w = water();
    let half = liquid_at_fill(w, 0.5);
    let full_top = water_levels(&voxels(&[(2, 3, 2, w)]), false);
    let half_top = water_levels(&voxels(&[(2, 3, 2, half)]), false);
    let (full, half_h) = (level(&full_top, 2, 2), level(&half_top, 2, 2));
    assert!(wet(full) && wet(half_h));
    assert!(
        full > (3.0 * CELL_H) as f32 && full <= (4.0 * CELL_H) as f32,
        "{full}"
    );
    assert!(
        half_h > (3.0 * CELL_H) as f32 && half_h < full,
        "{half_h} < {full}"
    );
    // A deeper column's level is its top cell's.
    let deep = water_levels(&voxels(&[(2, 2, 2, w), (2, 3, 2, w)]), false);
    assert_eq!(level(&deep, 2, 2), full);
    // Dry, covered by stone, or going on up into the chunk above: no surface in this tile.
    assert!(!wet(level(&full_top, 5, 5)));
    let covered = water_levels(&voxels(&[(2, 3, 2, w), (2, 4, 2, stone())]), false);
    assert!(!wet(level(&covered, 2, 2)));
    let n = COLUMNS as i32;
    let up = water_levels(&voxels(&[(2, n - 1, 2, w), (2, n, 2, w)]), false);
    assert!(!wet(level(&up, 2, 2)));
    // The apron reads the neighbours' edge columns.
    let edge = water_levels(&voxels(&[(-1, 3, 4, w), (n, 3, 4, w)]), false);
    assert_eq!(level(&edge, -1, 4), full);
    assert_eq!(level(&edge, n, 4), full);
}

// both-faces: face B's chunk (the fixture mirrored into world rows) gives face A's tile.
#[test]
fn a_face_b_tile_is_its_face_a_twins() {
    let cells = basin(1.0, Some((4, 7, 0.5)));
    let local = voxels(&cells);
    let a = water_levels(&local, false);
    let b = water_levels(&mirror_padded(&local), true);
    assert_eq!(a, b);
    assert!(level(&a, 4, 7) < level(&a, 5, 7));
}

#[test]
fn corners_join_close_levels_and_keep_drops() {
    assert_eq!(corner_height([DRY; 4], 0.2), DRY);
    assert_eq!(corner_height([1.0, DRY, DRY, DRY], 0.2), 1.0);
    assert!((corner_height([1.0, 1.1, DRY, 0.9], 0.25) - 1.0).abs() < 1e-6);
    // A column half a metre down is a drop: the corner keeps the upper water's level.
    assert_eq!(corner_height([1.0, 1.0, 0.5, 0.5], 0.2), 1.0);
    let flat = basin(1.0, None);
    let corners = corner_heights(&water_levels(&voxels(&flat), false), 0.2);
    assert_eq!(corners.len(), CORNERS * CORNERS);
    let top = corners[3 + CORNERS * 3];
    assert!(corners.iter().all(|h| *h == top), "a calm basin is flat");
}

#[test]
fn the_grid_covers_every_column_facing_up() {
    let idx = grid_indices();
    assert_eq!(idx.len(), COLUMNS * COLUMNS * 6);
    // Corner i at (x, z); a triangle faces +y when (b − a) × (c − a) has a positive y.
    let at = |i: u32| [(i as usize % CORNERS) as f64, (i as usize / CORNERS) as f64];
    for t in idx.chunks_exact(3) {
        let [a, b, c] = [at(t[0]), at(t[1]), at(t[2])];
        let cross_y = (b[1] - a[1]) * (c[0] - a[0]) - (b[0] - a[0]) * (c[1] - a[1]);
        assert!(cross_y > 0.0, "{t:?}");
    }
}

fn up_quads(m: &MeshArrays, rise: f32) -> usize {
    (0..m.positions.len() / 12)
        .filter(|q| m.normals[q * 12 + 1] == rise)
        .count()
}

// both-faces: the same fixture meshed as either face's chunk (world rows mirrored for face B).
#[test]
fn the_flag_drops_only_the_waters_tops() {
    for mirror in [false, true] {
        let cells = voxels(&[(2, 3, 2, water()), (3, 3, 2, water()), (2, 2, 2, stone())]);
        let cells = if mirror { mirror_padded(&cells) } else { cells };
        let mesh = |water_tops| {
            let flags = ChunkFlags { water_tops };
            mesh_chunk_flags(&cells, &[], mirror, [0; 3], &[], None, flags).expect("padded")
        };
        let (on, off) = (mesh(true), mesh(false));
        // Face B's mesh is turned back into world rows: its up is −y.
        let up = if mirror { -1.0 } else { 1.0 };
        assert!(up_quads(&on.transparent, up) > 0, "a water top meshed");
        assert_eq!(up_quads(&off.transparent, up), 0, "no water top");
        let quads = |m: &MeshArrays| m.positions.len() / 12;
        assert_eq!(
            quads(&off.transparent),
            quads(&on.transparent) - up_quads(&on.transparent, up),
            "the sides stay"
        );
        assert_eq!(
            on.opaque.positions, off.opaque.positions,
            "the ground is untouched"
        );
    }
}

#[test]
fn a_rising_pool_changes_the_tile_not_the_mesh() {
    let flags = ChunkFlags { water_tops: false };
    let mesh =
        |cells: &[u16]| mesh_chunk_flags(cells, &[], false, [0; 3], &[], None, flags).unwrap();
    let low = voxels(&basin(0.5, None));
    let high = voxels(&basin(1.0, None));
    let mut atlas = HeightAtlas::new();
    assert!(atlas.set([0, 0, 0], water_levels(&low, false)));
    assert_eq!(atlas.take_dirty().len(), 1);
    assert!(
        !atlas.set([0, 0, 0], water_levels(&low, false)),
        "unchanged: nothing to upload"
    );
    assert!(
        atlas.set([0, 0, 0], water_levels(&high, false)),
        "the level rose"
    );
    let slot = atlas.slot([0, 0, 0]).expect("a slot");
    let dirty = atlas.take_dirty();
    assert_eq!(dirty.len(), 1);
    assert_eq!(dirty[0].0, slot);
    let (a, b) = (mesh(&low), mesh(&high));
    assert_eq!(
        a.transparent.positions, b.transparent.positions,
        "no re-mesh needed"
    );
    assert_eq!(a.opaque.positions, b.opaque.positions);
    // A chunk that dries frees its slot for the next.
    assert!(atlas.set([0, 0, 0], water_levels(&voxels(&[]), false)));
    assert_eq!(atlas.slot([0, 0, 0]), None);
    atlas.set([1, 0, 0], water_levels(&high, false));
    assert_eq!(atlas.capacity(), 1);
}

// both-faces: the tile rides with either face's chunk meshes, its face named.
#[test]
fn the_meshes_carry_the_tile_only_without_tops() {
    for mirror in [false, true] {
        let cells = voxels(&basin(1.0, None));
        let cells = if mirror { mirror_padded(&cells) } else { cells };
        let mesh = |water_tops| {
            let flags = ChunkFlags { water_tops };
            mesh_chunk_flags(&cells, &[], mirror, [0; 3], &[], None, flags).expect("padded")
        };
        assert_eq!(mesh(true).water, None);
        let tile = mesh(false).water.expect("a tile");
        assert_eq!(tile.face_b, mirror);
        assert_eq!(tile.levels, water_levels(&cells, mirror));
        assert_ne!(tile.fluid, crate::NO_FLUID);
        // Open sky: the packed light is full sky.
        assert!(tile.lights.iter().all(|l| *l == pack_light([1.0, 0.0])));
    }
}

// both-faces: each face's winding is checked.
#[test]
fn dry_columns_draw_nothing_and_wet_ones_end_at_their_edge() {
    // One wet column at (5, 7) in an otherwise dry tile: its two triangles are drawn, flat at its
    // level and within its own square; every other column's six vertices collapse (no pixels,
    // so no fragment of a dry column is shaded, and the surface ends at the shoreline).
    for face_b in [false, true] {
        let mut levels = vec![DRY; TILE * TILE];
        levels[6 + TILE * 8] = 2.25;
        let drawn: Vec<(usize, [f32; 3])> = (0..GRID_VERTICES)
            .filter_map(|v| grid_vertex(&levels, 0.2, v, face_b).map(|p| (v, p)))
            .collect();
        assert_eq!(drawn.len(), COLUMN_VERTICES, "face_b {face_b}");
        for (v, [x, h, z]) in &drawn {
            assert_eq!(v / COLUMN_VERTICES, 5 + COLUMNS * 7);
            assert_eq!(*h, 2.25);
            assert!(
                (5.0..=6.0).contains(x) && (7.0..=8.0).contains(z),
                "{x} {z}"
            );
        }
    }
}

// both-faces: face B's column vertices are the re-wound grid's.
#[test]
fn a_columns_vertices_follow_the_grids_winding_on_both_faces() {
    let idx = grid_indices_both();
    let c = CORNERS as u32;
    for (face_b, first) in [(false, 0), (true, COLUMNS * COLUMNS * 6)] {
        for (k, [ox, oz]) in column_corners(face_b).iter().enumerate() {
            // Column (0, 0)'s corners are (0, 0) .. (1, 1): corner i + CORNERS × j.
            assert_eq!(idx[first + k], *ox as u32 + c * *oz as u32);
        }
    }
}
