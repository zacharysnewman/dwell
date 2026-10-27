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
                assert!(
                    s >= f64::from(rules.spacing),
                    "seed {seed}: {p:?} {q:?} {s}"
                );
            }
            for f in [Face::A, Face::B] {
                let t = g.local(f);
                assert!(
                    t.coast_at(p.x, p.z) >= rules.inland,
                    "seed {seed} {p:?} {f:?}"
                );
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
                // The monument's columns (its own test checks them).
                let monument = t.monument(p, portal).expect("the build's rules stand one");
                let under: std::collections::HashSet<(i32, i32)> =
                    monument.cells().map(|(c, _)| (c[0], c[2])).collect();
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
                            Part::Clear if under.contains(&(x, z)) => {}
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
    let top = g
        .local(Face::A)
        .ground_y(portal.x, portal.z)
        .expect("inside");
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

dwell_world::both_faces!(
    /// The dev link `?spawn=portal` (`portal-b`): the player stands on face `f`'s rim over the
    /// ladder of the portal nearest the centre, the ground under its feet and air at its head.
    fn the_portal_spawn_stands_on_the_rim_over_the_ladder(f: FaceFrame) {
        let seed = 7;
        let [x, y, z] =
            dwell_worldgen::portal_spawn_for(GENERATOR_TERRAIN, seed, f.face).expect("portals");
        let g = BifacialTerrain::new(seed);
        let p = g.portals();
        let near = p
            .sites
            .iter()
            .min_by_key(|s| i64::from(s.x).pow(2) + i64::from(s.z).pow(2))
            .copied()
            .expect("a portal");
        assert_eq!(p.wall_column(near), (x.floor() as i32, z.floor() as i32));
        let mut v = Voxels::new(&g);
        let feet = f.row(row_of_m(f.y(y)));
        let (cx, cz) = (x.floor() as i32, z.floor() as i32);
        assert!(!info(v.at(cx, feet, cz)).solid, "air at the feet");
        assert!(
            info(v.at(cx, f.row(row_of_m(f.y(y)) - 1), cz)).solid,
            "ground under them"
        );
        assert_eq!(f.face, FaceFrame::at_height(y).face);
    }
);

#[test]
fn no_portal_opens_in_a_river_or_a_lake() {
    // both-faces: a site's whole footprint is checked on each face.
    // Generator 21's world map (ADR 0049) puts rivers and lakes on the lowlands where the portals'
    // sites lie: a shaft opening under water would drain it down the shaft (and the rim stands in
    // it), so a site is dry over its footprint on both faces. Seeds 51 and 103 sited one in a
    // river on face B before the rule asked for dry ground.
    let rules = MIDPLANE.portals;
    let foot = f64::from(rules.radius) + f64::from(rules.rim_width);
    let f = foot.ceil() as i32;
    for seed in SEEDS.into_iter().chain([51, 103]) {
        let g = BifacialTerrain::new(seed);
        for p in g.portal_sites() {
            for face in [Face::A, Face::B] {
                let t = g.local(face);
                for dz in -f..=f {
                    for dx in -f..=f {
                        if f64::from(dx * dx + dz * dz) > foot * foot {
                            continue;
                        }
                        let c = t.column_at(p.x + dx, p.z + dz);
                        assert_eq!(
                            c.water,
                            dwell_worldgen::terrain::Water::None,
                            "seed {seed} {p:?} {face:?} ({dx}, {dz})"
                        );
                    }
                }
            }
        }
    }
}

dwell_world::both_faces!(
    /// Owner, 2026-10-26: pillars round every portal's opening, so the portals are found. Face
    /// `f`'s monument stands with its anchor on the axis and its bottom row on the lowest ground
    /// under it: every cell it fills is in the world (where the ground there is lower), up to its
    /// top 30 m over the ground, which the sky test doesn't skip.
    fn every_portal_stands_its_monument_on_its_ground(f: FaceFrame) {
        for seed in [0u64, 7] {
            let g = BifacialTerrain::new(seed);
            let p = g.portals();
            let t = g.local(f.face);
            let mut v = Voxels::new(&g);
            for &portal in p.sites.iter().take(2) {
                let m = t.monument(p, portal).expect("the build's rules stand one");
                let feet = p.monument_feet(portal);
                let lowest = feet
                    .iter()
                    .map(|&(x, z)| t.ground_y(x, z).expect("inside"))
                    .min()
                    .expect("it has feet");
                assert_eq!(m.min[1], lowest + 1, "seed {seed} {portal:?}");
                assert_eq!(m.max[1] - m.min[1] + 1, 60, "30 m of ½ m rows");
                assert_eq!(
                    (m.min[0] + m.max[0], m.min[2] + m.max[2]),
                    (2 * portal.x, 2 * portal.z)
                );
                let mut placed = 0;
                for (c, state) in m.cells() {
                    let ground = t.ground_y(c[0], c[2]).expect("inside");
                    if c[1] <= ground {
                        continue; // the ground there is higher: it holds the pillar
                    }
                    assert_eq!(v.at(c[0], f.row(c[1]), c[2]), f.state(state), "{c:?}");
                    placed += 1;
                }
                assert!(placed > 2000, "{placed}");
                // Over the crown, air: nothing else stands there.
                assert_eq!(v.at(portal.x, f.row(m.max[1] + 1), portal.z), M.air);
                assert_ne!(
                    v.at(portal.x, f.row(m.max[1]), portal.z),
                    M.air,
                    "the crown"
                );
            }
        }
    }
);

#[test]
fn the_lod_shows_a_monument_from_afar() {
    // face-a-only: the LOD's monument is the same code on both faces (generate_lod_at); this checks
    // the crown that a 16 m level of face A draws.
    let g = BifacialTerrain::new(0);
    let p = g.portals();
    let portal = p.sites[0];
    let m = g.local(Face::A).monument(p, portal).expect("one");
    let level = (0..12)
        .find(|&l| lod::cell_size(l) == p.rules.monument_lod_cell)
        .expect("a level of that cell");
    let s = lod::section_size(level);
    let at = |v: i64, o: i64| (v - o).div_euclid(s) as i32;
    let crown = [portal.x, m.max[1], portal.z];
    let c = LodCoord::new(
        level,
        at(i64::from(crown[0]), lod::ORIGIN_X),
        at(i64::from(crown[1]), lod::ORIGIN_Y),
        at(i64::from(crown[2]), lod::ORIGIN_Z),
    );
    let mut cells = LodCells::new();
    g.generate_lod(c, &mut cells, None, None);
    let o = lod::section_origin(c);
    let i = |v: i32, o: i64| (i64::from(v) - o).div_euclid(lod::cell_size(level)) as i32;
    let got = cells[lod::cell_index(i(crown[0], o.x), i(crown[1], o.y), i(crown[2], o.z))];
    assert_eq!(got, require("dwell:stone"), "the crown's cell");
    // A level coarser than the data's draws none (the ground's own cells only).
    let coarse = level + 1;
    let cs = lod::section_size(coarse);
    let at = |v: i64, o: i64| (v - o).div_euclid(cs) as i32;
    let c = LodCoord::new(
        coarse,
        at(i64::from(crown[0]), lod::ORIGIN_X),
        at(i64::from(crown[1]), lod::ORIGIN_Y),
        at(i64::from(crown[2]), lod::ORIGIN_Z),
    );
    g.generate_lod(c, &mut cells, None, None);
    let o = lod::section_origin(c);
    let i = |v: i32, o: i64| (i64::from(v) - o).div_euclid(lod::cell_size(coarse)) as i32;
    if cells.len() == lod::VOLUME {
        let got = cells[lod::cell_index(i(crown[0], o.x), i(crown[1], o.y), i(crown[2], o.z))];
        assert_eq!(got, M.air, "no crown past lodCellM");
    }
}

#[test]
fn a_portal_monument_is_named_by_data_alone() {
    // both-faces: a fixture's monument stands on both faces, and none stands without one.
    let rules = PortalRules {
        monument: Some("dwell:proto_rock"),
        ..MIDPLANE.portals
    };
    let g = BifacialTerrain::with_portal_rules(5, rules);
    let p = g.portals();
    let a = p.sites[0];
    let mut v = Voxels::new(&g);
    for f in FaceFrame::under_test() {
        let t = g.local(f.face);
        let m = t.monument(p, a).expect("the fixture's");
        assert_eq!(m.max[1] - m.min[1] + 1, 3, "proto_rock is 3 rows tall");
        let (c, state) = m
            .cells()
            .find(|(c, _)| c[1] > t.ground_y(c[0], c[2]).expect("inside"))
            .expect("a cell over the ground");
        assert_eq!(v.at(c[0], f.row(c[1]), c[2]), f.state(state));
    }
    let none = BifacialTerrain::with_portal_rules(
        5,
        PortalRules {
            monument: None,
            ..rules
        },
    );
    let p = none.portals();
    assert!(none.local(Face::A).monument(p, p.sites[0]).is_none());
    assert!(p.monument_feet(p.sites[0]).is_empty());
}
