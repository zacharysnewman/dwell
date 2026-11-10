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
}
