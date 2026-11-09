//! The revealed big map (ADR 0068; owner, 2026-11-08: "big map should be fully explored on
//! creative"). In a mode with the `mapRevealed` ability the big map shows the whole world: ground
//! the minimap has not explored is coloured from the terrain generator the client already runs
//! from the seed, one tile of `revealed.tileCells`² explored cells at a time, as a job in the
//! generation workers (the LOD colour map's raster, [`crate::lod_map::raster`]: each cell its
//! column's top block's LOD colour, the colour the minimap's explored cells average, water
//! darkened by depth, rivers drawn from their polylines). The shell asks for the tiles the map's
//! view needs and caches them (`client/src/ui/minimap.ts`); the main thread does no generation.

use crate::jobs::Job;
use dwell_game::minimap::MapTunables;
use dwell_world::FaceFrame;

/// A tile's side in explored cells.
pub fn tile_cells(t: &MapTunables) -> i32 {
    t.revealed_tile_cells as i32
}

/// The job generating tile (`ti`, `tk`) of face `f`: explored cells `ti·n .. (ti+1)·n` along
/// face-local x (and `tk` along z), one texel each, a cell's texel its column at the cell's centre.
pub fn reveal_job(f: FaceFrame, ti: i32, tk: i32, t: &MapTunables) -> Job {
    let n = tile_cells(t);
    Job::LodMap {
        face: u8::from(f.is_b()),
        texel_m: t.explored_cell_m as i32,
        x0: ti * n,
        z0: tk * n,
        w: n,
        h: n,
    }
}

/// A finished tile for the shell: face (0 A, 1 B), `ti` and `tk` (i32 LE), then the tile's RGBA
/// rows of x from its least z (none without a terrain: a world the client does not generate).
pub fn encode_tile(face: u8, ti: i32, tk: i32, rgba: Option<&[u8]>) -> Vec<u8> {
    let mut out = Vec::with_capacity(9 + rgba.map_or(0, <[u8]>::len));
    out.push(face);
    out.extend_from_slice(&ti.to_le_bytes());
    out.extend_from_slice(&tk.to_le_bytes());
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
            let Output::LodMap(Some(tile)) = r.run(reveal_job(f, ti, tk, &MAP)) else {
                panic!("a terrain world reveals its tiles")
            };
            assert_eq!(tile.len(), (n * n * 4) as usize);
            // Explored cell (i, k) is texel (i − ti·n, k − tk·n): the same column one texel
            // generated alone at that cell gives.
            for (i, k) in [(-n, 0), (-1, n - 1), (-n / 2, 3)] {
                let Output::LodMap(Some(one)) = r.run(Job::LodMap {
                    face: u8::from(f.is_b()),
                    texel_m: cell,
                    x0: i,
                    z0: k,
                    w: 1,
                    h: 1,
                }) else {
                    panic!()
                };
                let o = (((k - tk * n) * n + (i - ti * n)) * 4) as usize;
                assert_eq!(&tile[o..o + 4], &one[..], "cell ({i}, {k})");
            }
            // The ground round the origin is inside the world: every texel has a colour.
            assert!(tile.chunks(4).all(|p| p[3] == 255));
        }
    );

    #[test]
    fn a_world_without_a_terrain_reveals_nothing_and_the_tile_says_where_it_is() {
        // face-a-only: no generation is involved, only the encoding.
        let Output::LodMap(None) = Runner::world(0, 1).run(reveal_job(FaceFrame::A, 0, 0, &MAP))
        else {
            panic!("a flat world has no terrain to reveal from")
        };
        let e = encode_tile(1, -3, 7, None);
        assert_eq!(e, [1, 0xfd, 0xff, 0xff, 0xff, 7, 0, 0, 0]);
        assert_eq!(encode_tile(0, 0, 0, Some(&[1, 2, 3, 4])).len(), 13);
    }
}
