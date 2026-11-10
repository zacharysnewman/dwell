//! The big map's tiles (ADR 0068; owner, 2026-11-08: "big map should be fully explored on
//! creative"; 2026-11-09: five zoom levels, each its own detail). Each zoom level's ground is
//! coloured from the terrain generator the client already runs from the seed, one tile of
//! `revealed.tileCells`² cells of that level's `cellM` at a time, as a job in the generation
//! workers; the shell keeps them per world (IndexedDB) and fogs what is unexplored outside a mode
//! with the `mapRevealed` ability (the LOD colour map's raster, [`crate::lod_map::raster`]: each cell its
//! column's top block's LOD colour, the colour the minimap's explored cells average, water
//! darkened by depth, rivers drawn from their polylines), and after the colours each cell's height
//! code (`dwell_game::minimap::height_code`, i16 LE: the big map's height tint,
//! `shadeMap`). The shell asks for the tiles the map's
//! view needs and caches them (`client/src/ui/minimap.ts`); the main thread does no generation.

use crate::jobs::Job;
use dwell_game::minimap::MapTunables;
use dwell_world::FaceFrame;

/// A tile's side in explored cells.
pub fn tile_cells(t: &MapTunables) -> i32 {
    t.revealed_tile_cells as i32
}

/// Whether `cell_m` is a zoom level's cell side (`bigMap.levels`) or the explored cells'.
pub fn is_level(cell_m: i32, t: &MapTunables) -> bool {
    cell_m == t.explored_cell_m as i32 || t.big_map_levels.iter().any(|&(c, _)| c as i32 == cell_m)
}

/// The job generating tile (`ti`, `tk`) of face `f` at cells of `cell_m`: cells `ti·n .. (ti+1)·n`
/// along face-local x (and `tk` along z), one texel each, a cell's texel its column at the cell's
/// centre.
pub fn reveal_job(f: FaceFrame, ti: i32, tk: i32, cell_m: i32, t: &MapTunables) -> Job {
    let n = tile_cells(t);
    Job::LodMap {
        face: u8::from(f.is_b()),
        heights: true,
        texel_m: cell_m,
        x0: ti * n,
        z0: tk * n,
        w: n,
        h: n,
    }
}

/// A finished tile for the shell: face (0 A, 1 B), `ti`, `tk` and its level's cell side (i32 LE),
/// then the tile's RGBA rows of x from its least z, then its i16 LE height codes (none without a
/// terrain: a world the client does not generate).
pub fn encode_tile(face: u8, ti: i32, tk: i32, cell_m: i32, rgba: Option<&[u8]>) -> Vec<u8> {
    let mut out = Vec::with_capacity(13 + rgba.map_or(0, <[u8]>::len));
    out.push(face);
    out.extend_from_slice(&ti.to_le_bytes());
    out.extend_from_slice(&tk.to_le_bytes());
    out.extend_from_slice(&cell_m.to_le_bytes());
    if let Some(p) = rgba {
        out.extend_from_slice(p);
    }
    out
}

/// Whether the level of `cell_m` cells is drawn from the loaded terrain where it is loaded
/// (`revealed.liveMaxCellM`; owner, 2026-11-09: "big map closest zooms should use 1m").
pub fn is_live(cell_m: i32, t: &MapTunables) -> bool {
    cell_m >= 1 && f64::from(cell_m) <= t.revealed_live_max_cell_m && is_level(cell_m, t)
}

/// Tile (`ti`, `tk`) of face `f` at cells of `cell_m` read from the loaded terrain (the minimap's
/// read, [`dwell_game::minimap::scan_tile`], each cell its column's top block's LOD colour; a
/// placed or broken block shows), round face-local eye height `eye_y`, in [`encode_tile`]'s
/// bytes: alpha 0 where nothing is loaded. None for a level not [`is_live`] or a tile with
/// nothing loaded (the generator's tile shows).
pub fn live_tile(
    world: &dwell_world::VoxelWorld,
    f: FaceFrame,
    ti: i32,
    tk: i32,
    cell_m: i32,
    eye_y: f64,
    t: &MapTunables,
) -> Option<Vec<u8>> {
    if !is_live(cell_m, t) {
        return None;
    }
    let n = tile_cells(t);
    let px = dwell_game::minimap::scan_tile(
        world,
        f,
        ti * n * cell_m,
        tk * n * cell_m,
        n as usize,
        cell_m,
        eye_y,
        t,
        &|m| (m != dwell_world::M.air).then(|| dwell_mesh::lod_color(m, 0)),
    )?;
    Some(encode_tile(u8::from(f.is_b()), ti, tk, cell_m, Some(&px)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::{Output, Runner};
    use dwell_game::minimap::MAP;
    use dwell_world::{GENERATOR_TERRAIN, both_faces};

    both_faces!(
        fn a_tile_holds_each_explored_cells_column_on_its_face(f: FaceFrame) {
            let r = Runner::world(GENERATOR_TERRAIN, 1);
            let n = tile_cells(&MAP);
            let cell = MAP.explored_cell_m as i32;
            let (ti, tk) = (-1, 0);
            let Output::LodMap(Some(tile)) = r.run(reveal_job(f, ti, tk, cell, &MAP)) else {
                panic!("a terrain world reveals its tiles")
            };
            // The colours, then a height code a cell.
            assert_eq!(tile.len(), (n * n * 6) as usize);
            // Explored cell (i, k) is texel (i − ti·n, k − tk·n): the same column one texel
            // generated alone at that cell gives.
            for (i, k) in [(-n, 0), (-1, n - 1), (-n / 2, 3)] {
                let Output::LodMap(Some(one)) = r.run(Job::LodMap {
                    face: u8::from(f.is_b()),
                    heights: true,
                    texel_m: cell,
                    x0: i,
                    z0: k,
                    w: 1,
                    h: 1,
                }) else {
                    panic!()
                };
                let j = ((k - tk * n) * n + (i - ti * n)) as usize;
                assert_eq!(&tile[j * 4..j * 4 + 4], &one[..4], "cell ({i}, {k})");
                let h = (n * n * 4) as usize + j * 2;
                assert_eq!(&tile[h..h + 2], &one[4..6], "cell ({i}, {k})'s height");
            }
            // The heights are the generator's column tops on this face, face-local.
            let terrain = dwell_worldgen::BifacialTerrain::for_generator(GENERATOR_TERRAIN, 1)
                .expect("a terrain world");
            let local = terrain.local(if f.is_b() {
                dwell_world::Face::B
            } else {
                dwell_world::Face::A
            });
            let (i, k) = (-n / 2, 3);
            let col = local.column_lod(
                i64::from(i * cell + cell / 2),
                i64::from(k * cell + cell / 2),
                i64::from(cell),
            );
            let j = ((k - tk * n) * n + (i - ti * n)) as usize;
            let (_, code) = dwell_game::minimap::tile_cell(&tile, (n * n) as usize, j).unwrap();
            let (hm, water) = dwell_game::minimap::decode_height(code).expect("a height");
            // Water's is its bed's (a river's channel under the column's own ground).
            let top = f64::from(col.height);
            if water {
                assert!(hm <= top + 0.25, "{hm} vs {top}");
            } else {
                assert!((hm - top).abs() <= 0.25, "{hm} vs {top}");
            }
            // The ground round the origin is inside the world: every texel has a colour.
            assert!(tile[..(n * n * 4) as usize].chunks(4).all(|p| p[3] == 255));
        }
    );

    both_faces!(
        fn explored_cells_the_terrain_has_not_shown_take_the_generators_colours(f: FaceFrame) {
            // Explored from high over a valley: the disc's tops saw nothing (an empty grid).
            use dwell_game::minimap::{Explored, TopGrid};
            let r = Runner::world(GENERATOR_TERRAIN, 1);
            let mut e = Explored::default();
            // One tile's worth: its centre, the disc inside it (a generated tile is slow in debug).
            let none = TopGrid {
                texel_m: MAP.minimap_texel_m,
                ..TopGrid::default()
            };
            e.mark(f, &none, [128.5, 128.5], &MAP);
            let first = e.take();
            assert!(first.iter().all(|&(_, c)| c == MAP.explored_pending_color));
            let mut tiles = Vec::new();
            loop {
                let asked = e.ask(&MAP);
                if asked.is_empty() {
                    break;
                }
                for key in asked {
                    let (face, ti, tk) = key;
                    assert_eq!(face, u8::from(f.is_b()));
                    let Output::LodMap(rgba) =
                        r.run(reveal_job(f, ti, tk, MAP.explored_cell_m as i32, &MAP))
                    else {
                        panic!()
                    };
                    tiles.push((key, rgba.clone().expect("a terrain world")));
                    e.fill(key, rgba, &MAP);
                }
            }
            assert_eq!(tiles.len(), 1);
            let fresh = e.take();
            assert_eq!(fresh.len(), first.len(), "every explored cell coloured");
            let n = tile_cells(&MAP);
            for ((face, i, k), c) in fresh {
                let ((_, ti, tk), rgba) = tiles
                    .iter()
                    .find(|((fc, ti, tk), _)| {
                        *fc == face && i.div_euclid(n) == *ti && k.div_euclid(n) == *tk
                    })
                    .expect("its tile");
                let o = (((k - tk * n) * n + (i - ti * n)) * 4) as usize;
                let p = &rgba[o..o + 4];
                let want = (u32::from(p[0]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[2]);
                assert_eq!(c, want, "cell ({i}, {k}) is its column's generated top");
            }
        }
    );

    both_faces!(
        fn each_zoom_levels_tile_is_its_own_cell_size(f: FaceFrame) {
            // The closest level and the widest (owner, 2026-11-09: each level its own detail):
            // a tile's cell (i, k) is the column at the centre of that level's cell, as one cell
            // generated alone at that size gives.
            let r = Runner::world(GENERATOR_TERRAIN, 1);
            let n = tile_cells(&MAP);
            for &(cell, _) in [
                MAP.big_map_levels[0],
                MAP.big_map_levels[MAP.big_map_levels.len() - 1],
            ]
            .iter()
            {
                let cell = cell as i32;
                let Output::LodMap(Some(tile)) = r.run(reveal_job(f, 0, -1, cell, &MAP)) else {
                    panic!("a terrain world builds its tiles")
                };
                assert_eq!(tile.len(), (n * n * 6) as usize);
                for (i, k) in [(0, -1), (n - 1, -n), (n / 2, -3)] {
                    let Output::LodMap(Some(one)) = r.run(Job::LodMap {
                        face: u8::from(f.is_b()),
                        heights: true,
                        texel_m: cell,
                        x0: i,
                        z0: k,
                        w: 1,
                        h: 1,
                    }) else {
                        panic!()
                    };
                    let j = ((k + n) * n + i) as usize;
                    assert_eq!(
                        &tile[j * 4..j * 4 + 4],
                        &one[..4],
                        "{cell} m cell ({i}, {k})"
                    );
                }
            }
        }
    );

    #[test]
    fn a_world_without_a_terrain_reveals_nothing_and_the_tile_says_where_it_is() {
        // face-a-only: no generation is involved, only the encoding.
        let Output::LodMap(None) = Runner::world(0, 1).run(reveal_job(FaceFrame::A, 0, 0, 8, &MAP))
        else {
            panic!("a flat world has no terrain to reveal from")
        };
        let e = encode_tile(1, -3, 7, 32, None);
        assert_eq!(e, [1, 0xfd, 0xff, 0xff, 0xff, 7, 0, 0, 0, 32, 0, 0, 0]);
        assert_eq!(encode_tile(0, 0, 0, 1, Some(&[1, 2, 3, 4])).len(), 17);
        // The zoom levels' cell sizes are asked for; others are not.
        for &(c, _) in MAP.big_map_levels {
            assert!(is_level(c as i32, &MAP));
        }
        assert!(!is_level(3, &MAP));
    }
    /// A grass floor at face-local row `ground − 1` over columns 0..`span` a side, loaded in a
    /// world that generates nothing.
    fn loaded_floor(f: FaceFrame, ground: i32, span: i32) -> dwell_world::VoxelWorld {
        let grass = dwell_blocks::parse_state("dwell:grass").expect("grass");
        let mut w = dwell_world::VoxelWorld::new(dwell_world::empty_generator(), None);
        for x in 0..span {
            for z in 0..span {
                let [x, y, z] = f.cell([x, ground - 1, z]);
                w.set_voxel(x, y, z, grass);
            }
        }
        w
    }

    fn rgb(tile: &[u8], n: i32, i: i32, k: i32) -> Option<[u8; 3]> {
        let o = (13 + (k * n + i) * 4) as usize;
        (tile[o + 3] == 255).then(|| [tile[o], tile[o + 1], tile[o + 2]])
    }

    fn lod_rgb(m: dwell_blocks::StateId) -> [u8; 3] {
        let c = dwell_mesh::lod_color(m, 0);
        [(c >> 16) as u8, (c >> 8) as u8, c as u8]
    }

    both_faces!(
        fn the_closest_levels_tile_shows_the_loaded_blocks_and_an_edit(f: FaceFrame) {
            // Owner, 2026-11-09: "big map closest zooms should use 1m", the real top blocks.
            let ground = dwell_world::FLAT_TOP_Y + 20;
            let eye = dwell_world::m_of_row(ground) + 1.6;
            let n = tile_cells(&MAP);
            let mut w = loaded_floor(f, ground, n);
            let cell = MAP.big_map_levels[0].0 as i32;
            assert_eq!(cell, 1);
            assert!(is_live(cell, &MAP));
            let tile = live_tile(&w, f, 0, 0, cell, eye, &MAP).expect("loaded ground");
            assert_eq!(tile.len(), 13 + (n * n * 6) as usize);
            assert_eq!(tile[0], u8::from(f.is_b()));
            let grass = dwell_blocks::parse_state("dwell:grass").expect("grass");
            let sand = dwell_blocks::parse_state("dwell:sand").expect("sand");
            let g = lod_rgb(grass);
            assert!((0..n).all(|k| (0..n).all(|i| rgb(&tile, n, i, k) == Some(g))));
            // A block placed on the ground at (5, 7) shows in its own pixel, and only there.
            let [x, y, z] = f.cell([5, ground, 7]);
            w.set_voxel(x, y, z, sand);
            let again = live_tile(&w, f, 0, 0, cell, eye, &MAP).expect("loaded ground");
            assert_eq!(rgb(&again, n, 5, 7), Some(lod_rgb(sand)));
            assert_eq!(rgb(&again, n, 6, 7), Some(g));
            assert_eq!(rgb(&again, n, 5, 6), Some(g));
            // Its height code is the block's top, a row over the floor.
            let cells = (n * n) as usize;
            let code = dwell_game::minimap::tile_cell(&again[13..], cells, (7 * n + 5) as usize)
                .expect("a cell")
                .1;
            let floor = dwell_game::minimap::tile_cell(&again[13..], cells, 0)
                .expect("a cell")
                .1;
            assert!(code > floor, "{code} over {floor}");
        }
    );

    both_faces!(
        fn the_second_level_reads_a_column_every_two_metres(f: FaceFrame) {
            let ground = dwell_world::FLAT_TOP_Y + 20;
            let eye = dwell_world::m_of_row(ground) + 1.6;
            let n = tile_cells(&MAP);
            // A fixture map by data alone (the shipped one's coarse levels are never live): a
            // 2 m level read from the terrain.
            let mut t = MAP;
            t.big_map_levels = &[(1.0, 128.0), (2.0, 512.0), (256.0, 100_000.0)];
            t.revealed_live_max_cell_m = 2.0;
            let cell = t.big_map_levels[1].0 as i32;
            assert!(is_live(cell, &t));
            let mut w = loaded_floor(f, ground, n * cell);
            let sand = dwell_blocks::parse_state("dwell:sand").expect("sand");
            // Cell (3, 4) of 2 m is columns 6–7 × 8–9; its centre column (7, 9) is the one read.
            let [x, y, z] = f.cell([7, ground, 9]);
            w.set_voxel(x, y, z, sand);
            let tile = live_tile(&w, f, 0, 0, cell, eye, &t).expect("loaded ground");
            assert_eq!(rgb(&tile, n, 3, 4), Some(lod_rgb(sand)));
            assert_ne!(rgb(&tile, n, 3, 3), rgb(&tile, n, 3, 4));
            // The whole tile covers n × 2 m: its far corner is still the floor.
            assert!(rgb(&tile, n, n - 1, n - 1).is_some());
        }
    );

    both_faces!(
        fn unloaded_ground_falls_back_to_the_generators_tile(f: FaceFrame) {
            let ground = dwell_world::FLAT_TOP_Y + 20;
            let eye = dwell_world::m_of_row(ground) + 1.6;
            let n = tile_cells(&MAP);
            // Half the tile loaded: the other half's cells are alpha 0 (the generator's show).
            let w = loaded_floor(f, ground, n / 2);
            let tile = live_tile(&w, f, 0, 0, 1, eye, &MAP).expect("some loaded");
            assert!(rgb(&tile, n, 0, 0).is_some());
            assert!(rgb(&tile, n, n - 1, 0).is_none());
            assert!(rgb(&tile, n, 0, n - 1).is_none());
            // Nothing loaded: no tile at all.
            assert!(live_tile(&w, f, 5, 5, 1, eye, &MAP).is_none());
            // A coarser level is never read from the terrain.
            let coarse = MAP.big_map_levels[2].0 as i32;
            assert!(!is_live(coarse, &MAP));
            assert!(live_tile(&w, f, 0, 0, coarse, eye, &MAP).is_none());
        }
    );
    both_faces!(
        fn ground_up_to_an_unloaded_chunk_is_unknown_not_its_stone(f: FaceFrame) {
            // 0.50.76's browser check: a hill whose grass was in a chunk not yet streamed showed
            // as the stone under it. Such a column falls back to the generator's tile.
            let ground = dwell_world::FLAT_TOP_Y + 20;
            let eye = dwell_world::m_of_row(ground) + 1.6;
            let n = tile_cells(&MAP);
            let mut w = loaded_floor(f, ground, n);
            let stone = dwell_blocks::parse_state("dwell:stone").expect("stone");
            let chunk = |r: i32| {
                let [x, y, z] = f.cell([3, r, 3]);
                dwell_world::chunk_of(x, y, z)
            };
            let mut r = ground;
            loop {
                let [x, y, z] = f.cell([3, r, 3]);
                w.set_voxel(x, y, z, stone);
                if chunk(r + 1) != chunk(r) {
                    break;
                }
                r += 1;
            }
            assert!(w.find(chunk(r + 1)).is_none());
            let tile = live_tile(&w, f, 0, 0, 1, eye, &MAP).expect("loaded ground");
            assert_eq!(rgb(&tile, n, 3, 3), None);
            assert!(rgb(&tile, n, 4, 3).is_some());
        }
    );
}
