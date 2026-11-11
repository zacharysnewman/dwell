//! Saplings a player plants (ADR 0079 S4): a species' sapling placed on its `plantOn` ground becomes
//! its stage-0 tree and a growth record, which the growers step grows a stage every `daysPerStage`
//! world days; on other ground it is refused; a broken leaf of a species tree drops its sapling at
//! the species' `dropChance` (adventure); the planted record survives a save. Both faces.

mod common;

use common::*;
use dwell_blocks::require;
use dwell_protocol::*;
use dwell_sim::entropy::{Entropy, SeededEntropy};
use dwell_sim::server::ServerConfig;
use dwell_storage::{WorldDb, WorldStore};
use dwell_world::{FLAT_TOP_Y, FaceFrame, M, empty_generator, m_of_row};
use dwell_worldgen::growth::{PLANTED, Record, cells_at, sapling_drop, seed_at};
use dwell_worldgen::species::{SPECIES, index};

/// The scene's ground level (face-local row): stone below it; grass at x = 6, sand at x = -6.
const BASE: i32 = FLAT_TOP_Y + 40;
const GRASS: [i32; 3] = [6, BASE - 1, 0];
const SAND: [i32; 3] = [-6, BASE - 1, 0];

fn config(f: FaceFrame, mode: &str) -> ServerConfig {
    ServerConfig {
        generator_override: Some(empty_generator()),
        spawn: Some(f.point([0.5, m_of_row(BASE), 0.5])),
        mode: mode.into(),
        // A world day a tick, so the test steps the days it wants.
        growth_speed: 3600.0,
        ..flat()
    }
}

/// A server in `mode` on stone in face `f`'s frame, with grass and sand cells; one player joined.
fn scene(f: FaceFrame, cfg: ServerConfig, entropy: Box<dyn Entropy>) -> Fixture {
    let mut fx = Fixture::with_entropy(cfg, entropy);
    let stone = f.state(require("dwell:stone"));
    for x in -10..=10 {
        for z in -10..=10 {
            for r in -3..0 {
                let [x, y, z] = f.cell([x, BASE + r, z]);
                fx.server.world().set_voxel(x, y, z, stone);
            }
        }
    }
    for (at, block) in [(GRASS, "dwell:grass"), (SAND, "dwell:sand")] {
        let [x, y, z] = f.cell(at);
        fx.server
            .world()
            .set_voxel(x, y, z, f.state(require(block)));
    }
    fx.join(1, &Client::new(1));
    fx.send(1, Message::InventoryAction(InventoryAction::Hello));
    feet_at(&mut fx, f, 0.5, m_of_row(BASE), 0.5);
    step(&mut fx, 5);
    fx.take_all();
    fx
}

fn feet_at(fx: &mut Fixture, f: FaceFrame, x: f64, feet: f64, z: f64) {
    let h = fx.server.player_handle_of(1).expect("a live player");
    let (_, physics, players) = fx.server.sim();
    let half = f64::from(players.half_height(h));
    let centre = f.point([x, feet + half, z]);
    players.teleport(
        physics,
        h,
        dwell_physics::DVec3::from(centre),
        dwell_physics::Vec3::ZERO,
    );
}

fn step(fx: &mut Fixture, ticks: u32) {
    for _ in 0..ticks {
        fx.server.step();
    }
}

/// An edit of face-local cell `local`, clicked on its face `face` (0 +X … 5 −Z; 2 the top).
fn edit_on(
    fx: &mut Fixture,
    f: FaceFrame,
    action: BlockEditAction,
    local: [i32; 3],
    face: usize,
    material: u16,
) {
    fx.send(
        1,
        Message::BlockEditRequest(BlockEditRequest {
            action,
            cell: f.cell(local),
            face: f.cell_face(face) as u8,
            material,
            rows: 1,
            down: false,
            point: None,
        }),
    );
    step(fx, 3);
}

/// Plants `item` on the ground cell `on` (face-local): the planted cell above it.
fn plant(fx: &mut Fixture, f: FaceFrame, item: &str, on: [i32; 3]) -> [i32; 3] {
    // Stand beside it, in reach.
    feet_at(fx, f, f64::from(on[0]) - 1.5, m_of_row(BASE), 0.5);
    step(fx, 2);
    edit_on(fx, f, BlockEditAction::Place, on, 2, f.state(require(item)));
    f.cell([on[0], on[1] + 1, on[2]])
}

fn world_holds(fx: &mut Fixture, cells: &[([i32; 3], u16)]) -> bool {
    cells
        .iter()
        .all(|([x, y, z], m)| fx.server.world().get_voxel(*x, *y, *z) == *m)
}

dwell_world::both_faces!(
    fn a_planted_sapling_stands_stage_0_and_grows_a_stage_every_days_per_stage(f: FaceFrame) {
        for (item, species, on) in [
            ("dwell:oak_sapling", "dwell:oak", GRASS),
            ("dwell:palm_sapling", "dwell:palm", SAND),
        ] {
            let mut fx = scene(f, config(f, "creative"), Box::new(CountingEntropy(1)));
            let anchor = plant(&mut fx, f, item, on);
            let r = fx
                .server
                .growth()
                .trees
                .get(&anchor)
                .unwrap_or_else(|| panic!("{item}: a record at the planted cell"))
                .clone();
            assert_eq!(usize::from(r.species), index(species).unwrap());
            assert_eq!((r.stage, r.face, r.planter), (0, f.face, PLANTED));
            assert_eq!(r.seed, seed_at(anchor), "seeded by the position");
            let zero = cells_at(&r, 0);
            assert!(world_holds(&mut fx, &zero), "{item}: stage 0's cells stand");
            let [x, y, z] = anchor;
            assert_ne!(
                fx.server.world().get_voxel(x, y, z),
                f.state(require(item)),
                "the sapling block became the tree"
            );
            // After daysPerStage × n days it is stage n.
            let per = f64::from(SPECIES[usize::from(r.species)].days_per_stage);
            let n = 3u8;
            while fx.server.world_days() < r.since + per * f64::from(n) {
                fx.server.step();
            }
            fx.server.grow_trees_now();
            let got = fx.server.growth().trees[&anchor].clone();
            assert_eq!(got.stage, n, "{item}: stage {n} after {n} stages' days");
            assert!(
                world_holds(&mut fx, &cells_at(&r, n)),
                "{item}: the world holds stage {n}'s tree"
            );
        }
    }
);

dwell_world::both_faces!(
    fn a_sapling_on_the_wrong_ground_is_refused(f: FaceFrame) {
        for (item, on) in [
            ("dwell:palm_sapling", GRASS),
            ("dwell:oak_sapling", SAND),
            ("dwell:pine_sapling", [2, BASE - 1, 0]), // stone
        ] {
            let mut fx = scene(f, config(f, "creative"), Box::new(CountingEntropy(1)));
            let anchor = plant(&mut fx, f, item, on);
            let [x, y, z] = anchor;
            assert_eq!(fx.server.world().get_voxel(x, y, z), M.air, "{item}");
            assert!(fx.server.growth().trees.is_empty(), "{item}: no record");
            assert_eq!(
                fx.server.stats_of(1).unwrap().edits_rejected,
                1,
                "{item}: refused"
            );
        }
    }
);

dwell_world::both_faces!(
    fn broken_leaves_of_a_species_tree_drop_its_sapling_at_the_datas_rate(f: FaceFrame) {
        let cfg = config(f, "adventure");
        let mut fx = scene(f, cfg, Box::new(SeededEntropy(42)));
        // A grown oak (stage 7) on the grass, recorded.
        let r = Record {
            anchor: f.cell([GRASS[0], BASE, 0]),
            face: f.face,
            species: index("dwell:oak").unwrap() as u16,
            seed: 5,
            stage: 7,
            since: 0.0,
            turns: 0,
            planter: PLANTED,
            variant: 0,
            stopped: false,
        };
        let cells = cells_at(&r, 7);
        for ([x, y, z], m) in &cells {
            fx.server.world().set_voxel(*x, *y, *z, *m);
        }
        fx.server.plant_tree(r.clone());
        let leaf = f.state(require("dwell:leaves"));
        // Peeled from the front: in each (x, y) column the tree's cell still standing nearest −z,
        // where it is a leaf (in sight of a player standing in front of it), pass after pass.
        let mut broken = 0usize;
        for _ in 0..6 {
            let mut front: std::collections::BTreeMap<(i32, i32), ([i32; 3], u16)> =
                Default::default();
            for &(c, _) in &cells {
                let m = fx.server.world().get_voxel(c[0], c[1], c[2]);
                if m == M.air {
                    continue;
                }
                let l = f.cell(c);
                let e = front.entry((l[0], l[1])).or_insert((l, m));
                if l[2] < e.0[2] {
                    *e = (l, m);
                }
            }
            for (l, _) in front.values().filter(|(_, m)| *m == leaf) {
                feet_at(
                    &mut fx,
                    f,
                    f64::from(l[0]) + 0.5,
                    m_of_row(l[1]) - 1.2,
                    f64::from(l[2]) - 1.5,
                );
                step(&mut fx, 1);
                edit_on(&mut fx, f, BlockEditAction::Break, *l, 5, 0);
                let [x, y, z] = f.cell(*l);
                broken += usize::from(fx.server.world().get_voxel(x, y, z) == M.air);
            }
        }
        assert!(broken >= 100, "broke {broken} leaves");
        let count = |fx: &Fixture, item: &str| -> u32 {
            let lying: u32 = fx
                .server
                .loose_items()
                .iter()
                .filter(|it| it.stack.item == item)
                .map(|it| u32::from(it.stack.count))
                .sum();
            lying + fx.server.inventory_of(1).map_or(0, |i| i.count_of(item))
        };
        let saplings = count(&fx, "dwell:oak_sapling");
        let sticks = count(&fx, "dwell:stick");
        let chance = f64::from(SPECIES[usize::from(r.species)].sapling.unwrap().drop_chance);
        // Seeded: a few saplings from the crown, at about the data's rate, alongside the sticks.
        assert!(saplings >= 1, "{saplings} saplings from {broken} leaves");
        assert!(
            f64::from(saplings) <= (broken as f64 * chance * 4.0).max(4.0),
            "{saplings} saplings from {broken} leaves"
        );
        assert!(sticks > saplings, "the sticks still drop: {sticks}");
    }
);

#[test]
fn a_leaf_drops_a_sapling_at_the_species_drop_chance() {
    // face-a-only: a roll, no geometry. Over many seeded rolls the rate is the data's.
    let mut e = SeededEntropy(9);
    let mut roll = || {
        let mut b = [0u8; 8];
        e.fill(&mut b);
        (u64::from_le_bytes(b) >> 11) as f64 / (1u64 << 53) as f64
    };
    for (i, s) in SPECIES.iter().enumerate() {
        let r = Record {
            anchor: [0, 0, 0],
            face: dwell_world::Face::A,
            species: i as u16,
            seed: 0,
            stage: 7,
            since: 0.0,
            turns: 0,
            planter: PLANTED,
            variant: 0,
            stopped: false,
        };
        let n = 20_000;
        let got = (0..n)
            .filter(|_| sapling_drop(&r, roll()).is_some())
            .count();
        let want = f64::from(s.sapling.unwrap().drop_chance);
        let rate = got as f64 / n as f64;
        assert!(
            (rate - want).abs() < 0.01,
            "{}: {rate} against {want}",
            s.name
        );
        assert_eq!(sapling_drop(&r, 0.0), s.sapling.map(|p| p.item));
    }
}

#[test]
fn a_planted_trees_record_survives_a_save() {
    // face-a-only: a byte format through the store.
    let f = FaceFrame::A;
    let db = WorldDb::in_memory().expect("db");
    let cfg = || ServerConfig {
        store: Some(WorldStore::new(db.clone(), false)),
        ..config(f, "creative")
    };
    let (anchor, want) = {
        let mut fx = scene(f, cfg(), Box::new(CountingEntropy(1)));
        let anchor = plant(&mut fx, f, "dwell:oak_sapling", GRASS);
        let want = fx.server.growth().trees[&anchor].clone();
        fx.server.save_now();
        fx.server.flush_store();
        (anchor, want)
    };
    let fx = Fixture::new(cfg());
    assert_eq!(fx.server.growth().trees.get(&anchor), Some(&want));
}
