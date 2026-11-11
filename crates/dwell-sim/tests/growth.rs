//! Trees growing over time on the server (ADR 0079 S3): the growers step grows a recorded tree by
//! the world's days, never over a player's edit, never once felled, and the records survive a save.

mod common;

use common::*;
use dwell_sim::server::ServerConfig;
use dwell_storage::{WorldDb, WorldStore};
use dwell_world::{FLAT_TOP_Y, FaceFrame, empty_generator, m_of_row};
use dwell_worldgen::growth::{Record, cells_at};
use dwell_worldgen::species::{SPECIES, index};

const BASE: i32 = FLAT_TOP_Y + 40;

fn config(f: FaceFrame) -> ServerConfig {
    ServerConfig {
        generator_override: Some(empty_generator()),
        spawn: Some(f.point([0.5, m_of_row(BASE), 0.5])),
        // A world day a tick, so the test sets the days it wants.
        growth_speed: 3600.0,
        ..flat()
    }
}

/// An oak at stage 2 on face `f`, 6 m from the spawn.
fn oak(f: FaceFrame) -> Record {
    Record {
        anchor: f.cell([6, BASE, 0]),
        face: f.face,
        species: index("dwell:oak").unwrap() as u16,
        seed: 11,
        stage: 2,
        since: 0.0,
        turns: 0,
        planter: 0,
        variant: 0,
        stopped: false,
    }
}

fn per() -> f64 {
    f64::from(SPECIES[index("dwell:oak").unwrap()].days_per_stage)
}

/// A server with the oak's stage-2 cells in its world and its record due `n` stages.
fn scene(f: FaceFrame, cfg: ServerConfig, n: f64) -> (Fixture, Record) {
    let mut fx = Fixture::new(cfg);
    let mut r = oak(f);
    for _ in 0..3 {
        fx.server.step();
    }
    for ([x, y, z], m) in cells_at(&r, 2) {
        fx.server.world().set_voxel(x, y, z, m);
    }
    r.since = fx.server.world_days() - per() * n - 0.01;
    fx.server.plant_tree(r.clone());
    (fx, r)
}

fn cells(fx: &mut Fixture, of: &[([i32; 3], u16)]) -> Vec<u16> {
    of.iter()
        .map(|([x, y, z], _)| fx.server.world().get_voxel(*x, *y, *z))
        .collect()
}

dwell_world::both_faces!(
    fn a_tree_grows_its_due_stages_by_the_world_days(f: FaceFrame) {
        let (mut fx, r) = scene(f, config(f), 3.0);
        fx.server.grow_trees_now();
        let got = fx.server.growth().trees[&r.anchor].clone();
        assert_eq!(got.stage, 5, "three stages after three stages' days");
        let five = cells_at(&r, 5);
        let want: Vec<u16> = five.iter().map(|(_, m)| *m).collect();
        assert_eq!(cells(&mut fx, &five), want, "the world holds stage 5's tree");
        // Nothing more is due yet.
        fx.server.grow_trees_now();
        assert_eq!(fx.server.growth().trees[&r.anchor].stage, 5);
    }
);

dwell_world::both_faces!(
    fn a_player_edit_stands_and_a_felled_tree_stops(f: FaceFrame) {
        let (mut fx, r) = scene(f, config(f), 3.0);
        let stone = f.state(dwell_blocks::require("dwell:stone"));
        let two: Vec<[i32; 3]> = cells_at(&r, 2).into_iter().map(|(c, _)| c).collect();
        let built = cells_at(&r, 5)
            .into_iter()
            .map(|(c, _)| c)
            .find(|c| !two.contains(c))
            .unwrap();
        fx.server.world().set_voxel(built[0], built[1], built[2], stone);
        fx.server.grow_trees_now();
        assert_eq!(fx.server.world().get_voxel(built[0], built[1], built[2]), stone);

        // A second tree, felled at its foot before it is due: it never grows.
        let (mut fx, r) = scene(f, config(f), 3.0);
        let a = r.anchor;
        fx.server.world().set_voxel(a[0], a[1], a[2], dwell_world::M.air);
        fx.server.grow_trees_now();
        let got = fx.server.growth().trees[&a].clone();
        assert!(got.stopped);
        assert_eq!(got.stage, 2);
    }
);

#[test]
fn the_growth_records_survive_a_save() {
    // face-a-only: a byte format through the store.
    let f = FaceFrame::A;
    let db = WorldDb::in_memory().expect("db");
    assert_eq!(db.tree_growth(), None, "an older world has none");
    let cfg = || ServerConfig {
        store: Some(WorldStore::new(db.clone(), false)),
        ..config(f)
    };
    let anchor = {
        let (mut fx, r) = scene(f, cfg(), 2.0);
        fx.server.grow_trees_now();
        fx.server.save_now();
        fx.server.flush_store();
        r.anchor
    };
    assert!(db.tree_growth().is_some(), "saved");
    let fx = Fixture::new(cfg());
    let got = &fx.server.growth().trees[&anchor];
    assert_eq!(got.stage, 4, "the stage kept");
}
