//! Per-tree leaf tints (ADR 0079, docs/TREE_GROWTH.md §11): the generator lists the species trees
//! reaching a chunk or an LOD section with each tree's tint (`TerrainGenerator::tree_tints`,
//! `lod_tints_at`), and the meshers tint a leaf cell by its owning tree
//! (`dwell_world::tree_tint::owner`). Here, on the starter world's species trees (both faces): a cell gets
//! the same tree from either chunk at a border and from the LOD, neighbouring trees differ, and the
//! default world (hand-built pines, no species) lists none.

use std::sync::Arc;

use dwell_world::lod::{self, LodCoord};
use dwell_world::tree_tint::{TreeTint, tint_at};
use dwell_world::*;
use dwell_worldgen::bifacial::face_seed;
use dwell_worldgen::config::world_config;
use dwell_worldgen::terrain::TerrainGenerator;
use dwell_worldgen::worldmap::WorldMap;

const NAME: &str = "test:starter";
const S: i32 = CHUNK_SIZE;
/// A spot of species trees on each face of the starter world (seed 0): face A's broadleaf forest
/// (`client/bench/spots.json` `starter:broadleaf`: oaks), and face B's species pines at its own
/// map's (13900, −7100).
fn spot(f: FaceFrame) -> (i32, i32) {
    if f.is_b() {
        (13900, -7100)
    } else {
        (22300, 100)
    }
}

fn starter(f: FaceFrame) -> TerrainGenerator {
    let e = world_config(NAME).unwrap().expect("the starter's edit");
    let map = WorldMap::for_height(face_seed(0, f.face), e.height.unwrap())
        .with_worldgen(NAME)
        .unwrap();
    TerrainGenerator::with_map(face_seed(0, f.face), Arc::new(map))
        .with_biomes_and_geology(&e.biomes, e.map, e.geology)
        .with_vegetation(e.vegetation)
}

/// Chunk column (cx, cz)'s trees with their tints.
fn chunk_trees(g: &TerrainGenerator, cx: i32, cz: i32) -> Vec<TreeTint> {
    g.tree_tints((cx * S, cz * S), (cx * S + S - 1, cz * S + S - 1))
}

both_faces!(
    /// Two chunks meeting at a border each list the trees reaching their own columns, yet every
    /// cell of both border columns gets the tree a list of the whole area gives it, so the two
    /// chunks' meshes tint a crown across the border alike; and neighbouring trees differ.
    fn chunks_at_a_border_give_its_cells_the_same_tree(f: FaceFrame) {
        let g = starter(f);
        let (x, z) = spot(f);
        let (cx, cz) = (x.div_euclid(S), z.div_euclid(S));
        let all = g.tree_tints(((cx - 2) * S, (cz - 2) * S), ((cx + 3) * S, (cz + 3) * S));
        assert!(all.len() >= 4, "oaks at the spot: {}", all.len());
        let distinct = all.iter().filter(|t| t.tint != all[0].tint).count();
        assert!(distinct > 0, "neighbouring trees differ: {all:?}");
        let mut checked = 0;
        for dz in -1..=1 {
            for dx in -1..=1 {
                let (ax, az) = (cx + dx, cz + dz);
                let (west, east) = (chunk_trees(&g, ax, az), chunk_trees(&g, ax + 1, az));
                let border = (ax + 1) * S;
                for t in all
                    .iter()
                    .filter(|t| t.min[0] < border && t.max[0] >= border)
                {
                    for z in t.min[2].max(az * S)..=t.max[2].min(az * S + S - 1) {
                        for y in t.min[1]..=t.max[1] {
                            for (x, list) in [(border - 1, &west), (border, &east)] {
                                let c = [x, y, z];
                                assert_eq!(tint_at(list, c), tint_at(&all, c), "{c:?}");
                                checked += 1;
                            }
                        }
                    }
                }
            }
        }
        assert!(checked > 0, "a crown crosses a border near the spot");
    }
);

both_faces!(
    /// The LOD's finest section over a species tree lists its tree cells with the tint the chunk holding
    /// each cell gives it: no colour pop at the hand-over.
    fn lod_and_chunks_tint_a_tree_alike(f: FaceFrame) {
        let g = starter(f);
        let (x, z) = spot(f);
        let t = *g
            .tree_tints((x - 64, z - 64), (x + 64, z + 64))
            .first()
            .expect("a species tree at the spot");
        let size = lod::section_size(0);
        let o0 = lod::section_origin(LodCoord::new(0, 0, 0, 0));
        let at = |v: i32, o: i64| ((i64::from(v) - o).div_euclid(size)) as i32;
        let mid = (t.min[1] + t.max[1]) / 2;
        let c = LodCoord::new(
            0,
            at(t.anchor[0], o0.x),
            at(mid, o0.y),
            at(t.anchor[2], o0.z),
        );
        let o = lod::section_origin(c);
        let mut cells = Vec::new();
        g.generate_lod_at(c, o.y, &mut cells, None);
        let n = lod::SECTION_CELLS;
        let mut tints = Vec::new();
        g.lod_tints_at(c, o.y, -1..=n, &|r| r, &cells, &mut tints);
        assert!(!tints.is_empty(), "the section holds the oak's cells");
        assert_eq!(lod::cell_size(0), 1, "the finest cells are the chunks' own");
        let p = lod::PAD as usize;
        for (i, tint) in tints {
            let i = i as usize;
            let (x, z, y) = (i % p, (i / p) % p, i / (p * p));
            let w = [
                (o.x + x as i64 - 1) as i32,
                (o.y + y as i64 - 1) as i32,
                (o.z + z as i64 - 1) as i32,
            ];
            let near = chunk_trees(&g, w[0].div_euclid(S), w[2].div_euclid(S));
            assert_eq!(tint_at(&near, w), tint, "{w:?}");
        }
    }
);

both_faces!(
    /// The default world stands hand-built pines, no species: no tree is listed, so its leaves
    /// keep their colour (and its goldens, worldgen hashes, never see a mesh colour).
    fn the_default_world_lists_no_tinted_trees(f: FaceFrame) {
        let g = TerrainGenerator::new(face_seed(7, f.face));
        assert!(
            !g.trees_in((-200, -200), (199, 199)).is_empty(),
            "pines stand"
        );
        assert!(g.tree_tints((-200, -200), (199, 199)).is_empty());
    }
);
