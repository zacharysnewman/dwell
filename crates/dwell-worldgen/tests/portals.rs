//! The portals between the faces (`portals.rs`, `shared/worldgen/midplane.json`; owner 2026-10-21):
//! sited from the seed, as many as it asks for and spaced apart, on land on both faces; each a shaft
//! open from one face's ground to the other's with a continuous ladder up its wall and a rim round
//! both openings, the same whichever order its chunks are generated in, with nothing growing over
//! it; and a fixture of rules added by data alone.

use std::collections::HashMap;

use dwell_blocks::{StateId, info, require};
use dwell_world::lod::{self, LodCells, LodCoord};
use dwell_world::*;
use dwell_worldgen::BifacialTerrain;
use dwell_worldgen::noise::floor_div;
use dwell_worldgen::portals::{MIDPLANE, Part, PortalRules, Portals, wanted};

const S: i32 = CHUNK_SIZE;
const SEEDS: [u64; 6] = [0, 7, 3, 20_260_925, 1, 2];

/// The world's voxels, generated a chunk at a time on demand.
struct Voxels<'a> {
    g: &'a BifacialTerrain,
    chunks: HashMap<ChunkCoord, Chunk>,
}

impl<'a> Voxels<'a> {
    fn new(g: &'a BifacialTerrain) -> Self {
        Self {
            g,
            chunks: HashMap::new(),
        }
    }
    fn at(&mut self, x: i32, y: i32, z: i32) -> StateId {
        let c = chunk_of(x, y, z);
        let g = self.g;
        let chunk = self.chunks.entry(c).or_insert_with(|| {
            let mut chunk = Chunk::default();
            g.generate(c, &mut chunk);
            chunk
        });
        chunk.get(x - c.x * S, y - c.y * S, z - c.z * S)
    }
}

#[test]
fn the_seed_sites_its_count_of_portals_spaced_apart_on_land_on_both_faces() {
    // both-faces: a site is on land on both.
    let rules = MIDPLANE.portals;
    for seed in SEEDS {
        let g = BifacialTerrain::new(seed);
        let sites = g.portal_sites().to_vec();
        // As many as the seed asks for (the build's rules find them all on these seeds).
        let n = wanted(seed, &rules) as usize;
        assert!((rules.count_min as usize..=rules.count_max as usize).contains(&n));
        assert_eq!(sites.len(), n, "seed {seed}");
        // Deterministic: the same seed, the same sites (searched afresh too); another seed, others.
        assert_eq!(BifacialTerrain::new(seed).portal_sites(), &sites[..]);
        assert_eq!(
            dwell_worldgen::portals::search(seed, &rules, g.local(Face::A), g.local(Face::B)),
            sites
        );
        if seed != 0 {
            assert_ne!(BifacialTerrain::new(0).portal_sites(), &sites[..]);
        }
        for (i, p) in sites.iter().enumerate() {
            let d = f64::from(p.x).hypot(f64::from(p.z));
            assert!(d <= f64::from(rules.within), "seed {seed} {p:?}");
            for q in &sites[i + 1..] {
                let s = f64::from(p.x - q.x).hypot(f64::from(p.z - q.z));
                assert!(s >= f64::from(rules.spacing), "seed {seed}: {p:?} {q:?} {s}");
            }
            for f in [Face::A, Face::B] {
                let t = g.local(f);
                assert!(t.coast_at(p.x, p.z) >= rules.inland, "seed {seed} {p:?} {f:?}");
                let above_sea = f64::from(t.column_at(p.x, p.z).height) - SEA_LEVEL_M;
                // Not under the sea, and the shaft no longer than the rules allow.
                assert!(
                    (1.0..=f64::from(rules.max_height)).contains(&above_sea),
                    "seed {seed} {p:?} {f:?}: {above_sea}"
                );
            }
        }
    }
}

dwell_world::both_faces!(
    /// From face `f`'s side: every shaft column is open from its ground to the midplane (the
    /// bedrock and the core cut through), the ladder runs unbroken from the midplane to the top of
    /// the wall it climbs out onto, the rim stands round the opening, and nothing grows within the
    /// clear ground.
    fn a_shaft_is_open_from_the_ground_to_the_midplane_with_its_ladder_and_rim(f: FaceFrame) {
        let bedrock = require(MIDPLANE.bedrock);
        for seed in [0u64, 7] {
            let g = BifacialTerrain::new(seed);
            let p = g.portals();
            let t = g.local(f.face);
            let mut v = Voxels::new(&g);
            for &portal in p.sites.iter().take(2) {
                let r = p.reach();
                let ladder_top = t.ladder_top(p, portal);
                let mut checked = (0, 0, 0);
                for dz in -r..=r {
                    for dx in -r..=r {
                        let (x, z) = (portal.x + dx, portal.z + dz);
                        let top = t.ground_y(x, z).expect("inside the disc");
                        match p.part_of(portal, x, z) {
                            Part::Shaft { ladder } => {
                                for h in MIDPLANE_Y..=top.max(ladder_top) + 2 {
                                    let m = v.at(x, f.row(h), z);
                                    if ladder && h <= ladder_top {
                                        assert_eq!(m, f.state(p.ladder), "{portal:?} ladder {h}");
                                    } else {
                                        assert_eq!(m, M.air, "{portal:?} ({dx}, {dz}) at {h}");
                                    }
                                }
                                checked.0 += 1;
                            }
                            Part::Rim => {
                                let rows = p.rules.rim_rows;
                                for h in top + 1..=top + rows {
                                    assert_eq!(v.at(x, f.row(h), z), f.state(p.rim), "rim");
                                }
                                assert_eq!(v.at(x, f.row(top + rows + 1), z), M.air);
                                // The shaft's wall keeps its bedrock.
                                assert_eq!(v.at(x, f.row(MIDPLANE_Y), z), bedrock);
                                checked.1 += 1;
                            }
                            Part::Clear => {
                                // Bare ground: no decoration or tree over it.
                                for h in top + 1..=top + 24 {
                                    assert_eq!(v.at(x, f.row(h), z), M.air, "clear at {h}");
                                }
                                checked.2 += 1;
                            }
                            Part::None => {}
                        }
                    }
                }
                assert!(
                    checked.0 >= 13 && checked.1 > 0 && checked.2 > 0,
                    "{checked:?}"
                );
                // The ladder's top is the wall's top: a player climbs out onto the rim.
                let (wx, wz) = p.wall_column(portal);
                assert!(info(v.at(wx, f.row(ladder_top), wz)).solid);
                assert!(!info(v.at(wx, f.row(ladder_top + 1), wz)).solid);
            }
        }
    }
);

#[test]
fn the_ladder_runs_unbroken_from_face_a_to_face_b_across_the_midplane() {
    // both-faces: the ladder crosses between them.
    for seed in [0u64, 3] {
        let g = BifacialTerrain::new(seed);
        let p = g.portals();
        let mut v = Voxels::new(&g);
        for &portal in p.sites.iter() {
            let (lx, lz) = p.ladder_column(portal);
            let top_a = FaceFrame::A.row(g.local(Face::A).ladder_top(p, portal));
            let top_b = FaceFrame::B.row(g.local(Face::B).ladder_top(p, portal));
            assert!(FaceFrame::at_row(top_b).is_b() && !FaceFrame::at_row(top_a).is_b());
            for y in top_b..=top_a {
                assert!(
                    info(v.at(lx, y, lz)).climbable,
                    "seed {seed} {portal:?} row {y}"
                );
            }
            assert!(!info(v.at(lx, top_a + 1, lz)).climbable);
            assert!(!info(v.at(lx, top_b - 1, lz)).climbable);
            // Its wall is solid the whole way: the ladder is fixed to it.
            let (wx, wz) = p.wall_column(portal);
            for y in top_b..=top_a {
                assert!(
                    info(v.at(wx, y, wz)).solid,
                    "seed {seed} {portal:?} wall row {y}"
                );
            }
        }
    }
}

#[test]
fn a_portals_chunks_are_the_same_in_any_order_and_from_any_generator() {
    // both-faces: the shaft's chunks on both faces, generated in two orders.
    let seed = 7;
    let g = BifacialTerrain::new(seed);
    let p = g.portals();
    let mut coords = Vec::new();
    for &portal in p.sites.iter().take(3) {
        let top_a = g.local(Face::A).ladder_top(p, portal);
        let top_b = FaceFrame::B.row(g.local(Face::B).ladder_top(p, portal));
        let r = p.reach();
        for cz in floor_div(portal.z - r, S)..=floor_div(portal.z + r, S) {
            for cx in floor_div(portal.x - r, S)..=floor_div(portal.x + r, S) {
                for cy in floor_div(top_b, S)..=floor_div(top_a, S) {
                    coords.push(ChunkCoord::new(cx, cy, cz));
                }
            }
        }
    }
    let hash = |g: &BifacialTerrain, c: ChunkCoord| {
        let mut chunk = Chunk::default();
        g.generate(c, &mut chunk);
        chunk_hash(&chunk)
    };
    let forward: Vec<u64> = coords.iter().map(|&c| hash(&g, c)).collect();
    // Backward, a fresh generator per chunk, as a tool or another thread would build.
    let mut backward: Vec<u64> = coords
        .iter()
        .rev()
        .map(|&c| hash(&BifacialTerrain::new(seed), c))
        .collect();
    backward.reverse();
    assert_eq!(forward, backward);
    assert!(coords.len() > 12);
}

#[test]
fn the_fine_lod_shows_a_shafts_opening() {
    // face-a-only: the LOD's shaft is the same code on both faces (generate_lod_at); this checks
    // the opening that a section of face A's ground draws.
    let g = BifacialTerrain::new(0);
    let p = g.portals();
    let portal = p.sites[0];
    let top = g.local(Face::A).ground_y(portal.x, portal.z).expect("inside");
    let level = 0;
    let s = lod::section_size(level);
    let at = |v: i64, o: i64| (v - o).div_euclid(s) as i32;
    let c = LodCoord::new(
        level,
        at(i64::from(portal.x), lod::ORIGIN_X),
        at(i64::from(top), lod::ORIGIN_Y),
        at(i64::from(portal.z), lod::ORIGIN_Z),
    );
    let mut cells = LodCells::new();
    g.generate_lod(c, &mut cells, None, None);
    let o = lod::section_origin(c);
    let cell = |x: i32, y: i32, z: i32| {
        let i = |v: i32, o: i64| (i64::from(v) - o).div_euclid(lod::cell_size(level)) as i32;
        cells[lod::cell_index(i(x, o.x), i(y, o.y), i(z, o.z))]
    };
    // The axis column is open at its ground; the ground beyond the clear ground is not.
    assert_eq!(cell(portal.x, top, portal.z), M.air);
    let r = p.reach() + 1;
    let beyond = g
        .local(Face::A)
        .ground_y(portal.x + r, portal.z)
        .expect("inside");
    if (o.y..o.y + s).contains(&i64::from(beyond)) && r < lod::SECTION_CELLS {
        assert_ne!(cell(portal.x + r, beyond, portal.z), M.air);
    }
}

#[test]
fn portal_rules_added_by_data_alone_shape_the_world() {
    // both-faces: a fixture of rules builds both faces' openings.
    let rules = PortalRules {
        count_min: 2,
        count_max: 2,
        spacing: 8000.0,
        radius: 4.0,
        rim_block: "dwell:sand",
        rim_width: 2.0,
        rim_rows: 2,
        ladder: "dwell:ladder[facing=west]",
        wall: [1, 0],
        ..MIDPLANE.portals
    };
    let g = BifacialTerrain::with_portal_rules(5, rules);
    let p = g.portals();
    assert_eq!(p.sites.len(), 2);
    let (a, b) = (p.sites[0], p.sites[1]);
    assert!(f64::from(a.x - b.x).hypot(f64::from(a.z - b.z)) >= 8000.0);
    assert_ne!(p.sites[..], BifacialTerrain::new(5).portal_sites()[..]);
    // The ladder on the east wall, four columns out, facing west; the rim of sand, two cells high.
    assert_eq!(p.ladder_column(a), (a.x + 4, a.z));
    assert_eq!(p.wall_column(a), (a.x + 5, a.z));
    assert_eq!(p.part_of(a, a.x + 4, a.z), Part::Shaft { ladder: true });
    let mut v = Voxels::new(&g);
    for f in FaceFrame::under_test() {
        let t = g.local(f.face);
        let (lx, lz) = p.ladder_column(a);
        assert_eq!(
            v.at(lx, f.row(MIDPLANE_Y), lz),
            f.state(require("dwell:ladder[facing=west]"))
        );
        let (rx, rz) = (a.x - 5, a.z);
        assert_eq!(p.part_of(a, rx, rz), Part::Rim);
        let top = t.ground_y(rx, rz).expect("inside");
        assert_eq!(v.at(rx, f.row(top + 2), rz), f.state(require("dwell:sand")));
        assert_eq!(v.at(a.x, f.row(MIDPLANE_Y + 3), a.z), M.air);
    }
    // Portals resolved from given sites cut the same shapes.
    let again = Portals::with_sites(rules, p.sites.clone());
    assert_eq!(again.part_of(a, a.x, a.z), Part::Shaft { ladder: false });
}
