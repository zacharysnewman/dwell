//! Tall grass growing and spreading (ADR 0075 T3c, docs/ECOLOGY.md §10), on both faces: it spreads
//! over days up to and never past the climate's cap, a dry stretch stays sparse, a player's block
//! wins, eaten grass regrows, a load catches up a bounded number of days, and a rule added by data
//! alone grows on its own ground.

use std::collections::HashMap;

use dwell_blocks::{AIR, StateId, require};
use dwell_world::{CHUNK_SIZE, ChunkCoord, FLAT_TOP_Y, FaceFrame, both_faces};
use dwell_worldgen::grass::{Cells, GRASS, GrassData, Rule};
use dwell_worldgen::noise::{hash2, unit};

/// A face-local chunk on the flat world's top, its floor at row `FLOOR`.
const LY: i32 = FLAT_TOP_Y.div_euclid(CHUNK_SIZE);
const FLOOR: i32 = LY * CHUNK_SIZE + 4;

/// A world of cells (world coordinates; absent: air), now and as generated.
#[derive(Default, Clone)]
struct World {
    now: HashMap<[i32; 3], StateId>,
    generated: HashMap<[i32; 3], StateId>,
}

impl Cells for World {
    fn now(&self, c: [i32; 3]) -> Option<StateId> {
        Some(*self.now.get(&c).unwrap_or(&AIR))
    }
    fn generated(&mut self, c: [i32; 3]) -> Option<StateId> {
        Some(*self.generated.get(&c).unwrap_or(&AIR))
    }
}

impl World {
    /// A generated cell (face-local), now as generated.
    fn natural(&mut self, f: FaceFrame, p: [i32; 3], s: StateId) {
        self.now.insert(f.cell(p), f.state(s));
        self.generated.insert(f.cell(p), f.state(s));
    }
    /// A player's edit (face-local): now only.
    fn edit(&mut self, f: FaceFrame, p: [i32; 3], s: StateId) {
        self.now.insert(f.cell(p), f.state(s));
    }
    fn at(&self, f: FaceFrame, p: [i32; 3]) -> StateId {
        f.state(*self.now.get(&f.cell(p)).unwrap_or(&AIR))
    }
    fn apply(&mut self, writes: &[([i32; 3], StateId)]) {
        for &(c, s) in writes {
            self.now.insert(c, s);
        }
    }
}

/// A chunk-wide floor of `ground` with one generated tuft of `block` in its middle.
fn meadow(f: FaceFrame, ground: StateId, block: StateId) -> World {
    let mut w = World::default();
    for x in 0..CHUNK_SIZE {
        for z in 0..CHUNK_SIZE {
            w.natural(f, [x, FLOOR, z], ground);
        }
    }
    w.natural(f, [16, FLOOR + 1, 16], block);
    w
}

fn chunk(f: FaceFrame) -> ChunkCoord {
    f.chunk(ChunkCoord { x: 0, y: LY, z: 0 })
}

/// An envelope of a fixed climate share, the roll a per-column hash.
fn climate(share: f64) -> impl Fn(i32, i32) -> (f32, f64) {
    move |x, z| (unit(hash2(0x5EED, x, z)), share)
}

/// The share of the floor's cells holding `block`.
fn density(w: &World, f: FaceFrame, block: StateId) -> f64 {
    let mut n = 0;
    for x in 0..CHUNK_SIZE {
        for z in 0..CHUNK_SIZE {
            n += usize::from(w.at(f, [x, FLOOR + 1, z]) == block);
        }
    }
    n as f64 / f64::from(CHUNK_SIZE * CHUNK_SIZE)
}

/// Grows `days` days, one at a time, as the server's daily step does.
fn grow_daily(
    rule: &Rule,
    w: &mut World,
    f: FaceFrame,
    env: &dyn Fn(i32, i32) -> (f32, f64),
    days: u32,
) {
    for day in 0..days {
        let writes = rule.grow_chunk(f, chunk(f), day, 1, &env, w);
        w.apply(&writes);
    }
}

both_faces!(
    fn grass_spreads_over_days_up_to_and_not_past_the_climate_cap(f: FaceFrame) {
        let rule = Rule::build(7);
        let tall = rule.block();
        let mut w = meadow(f, require("dwell:grass"), tall);
        let env = climate(1.0);
        let start = density(&w, f, tall);
        grow_daily(&rule, &mut w, f, &env, 10);
        let ten = density(&w, f, tall);
        assert!(ten > start, "it spreads: {start} → {ten} in 10 days");
        grow_daily(&rule, &mut w, f, &env, 300);
        let full = density(&w, f, tall);
        let cap = rule.cap(1.0);
        assert!(
            full > cap - 0.15,
            "a lush meadow fills toward {cap}: {full}"
        );
        // Never past the cap: every tuft but the generated one stands on a fertile column.
        for x in 0..CHUNK_SIZE {
            for z in 0..CHUNK_SIZE {
                if w.at(f, [x, FLOOR + 1, z]) == tall && (x, z) != (16, 16) {
                    assert!(rule.fertile(env(x, z)), "({x}, {z}) is not fertile");
                }
            }
        }
        let fertile = (0..CHUNK_SIZE)
            .flat_map(|x| (0..CHUNK_SIZE).map(move |z| (x, z)))
            .filter(|&(x, z)| rule.fertile(env(x, z)))
            .count() as f64
            / f64::from(CHUNK_SIZE * CHUNK_SIZE);
        assert!(
            full <= fertile + 1.0 / 1024.0,
            "{full} over the fertile {fertile}"
        );
        assert!(
            (fertile - cap).abs() < 0.05,
            "the fertile share is the cap: {fertile}"
        );
    }
);

both_faces!(
    fn a_dry_stretch_stays_sparse(f: FaceFrame) {
        let rule = Rule::build(7);
        let tall = rule.block();
        let mut w = meadow(f, require("dwell:grass"), tall);
        // Seed a dense patch so dryness, not isolation, is what holds it back.
        for x in 10..22 {
            for z in 10..22 {
                w.natural(f, [x, FLOOR + 1, z], tall);
            }
        }
        let before = density(&w, f, tall);
        let env = climate(0.1);
        grow_daily(&rule, &mut w, f, &env, 300);
        let after = density(&w, f, tall);
        // Only fertile columns (≈ 8 % of the open floor) can take it beyond the seeded patch.
        assert!(
            after - before < 0.1,
            "a dry stretch stays sparse: {before} → {after}"
        );
        let none = climate(0.0);
        let mut bare = meadow(f, require("dwell:grass"), tall);
        grow_daily(&rule, &mut bare, f, &none, 300);
        assert_eq!(
            density(&bare, f, tall),
            1.0 / 1024.0,
            "no climate share, no growth"
        );
    }
);

both_faces!(
    fn a_players_block_is_never_overwritten(f: FaceFrame) {
        let rule = Rule::build(7);
        let tall = rule.block();
        let grass = require("dwell:grass");
        let stone = require("dwell:stone");
        let mut w = meadow(f, grass, tall);
        // A placed stone in a cell beside the tuft, a placed grass block on top of the floor (its
        // cell generated as air), and a cell whose generated tuft was replaced by a player's stone.
        w.edit(f, [17, FLOOR + 1, 16], stone);
        w.edit(f, [15, FLOOR + 1, 16], grass);
        // A roof over a cell: no light or air.
        w.edit(f, [16, FLOOR + 3, 17], stone);
        let env = climate(1.0);
        grow_daily(&rule, &mut w, f, &env, 300);
        assert_eq!(
            w.at(f, [17, FLOOR + 1, 16]),
            stone,
            "the placed stone stands"
        );
        assert_eq!(
            w.at(f, [15, FLOOR + 1, 16]),
            grass,
            "the placed grass block stands"
        );
        assert_eq!(
            w.at(f, [15, FLOOR + 2, 16]),
            AIR,
            "nothing grows on a player's ground"
        );
        assert_eq!(
            w.at(f, [16, FLOOR + 1, 17]),
            AIR,
            "nothing grows under a roof"
        );
    }
);

both_faces!(
    fn eaten_grass_regrows(f: FaceFrame) {
        let rule = Rule::build(7);
        let tall = rule.block();
        let grass = require("dwell:grass");
        let env = climate(1.0);
        // A generated stand of fertile tufts, all eaten.
        let mut w = World::default();
        let mut stand = Vec::new();
        for x in 0..CHUNK_SIZE {
            for z in 0..CHUNK_SIZE {
                w.natural(f, [x, FLOOR, z], grass);
                if (x + z) % 7 == 0 && rule.fertile(env(x, z)) {
                    w.natural(f, [x, FLOOR + 1, z], tall);
                    stand.push([x, FLOOR + 1, z]);
                }
            }
        }
        for &p in &stand {
            w.edit(f, p, AIR);
        }
        assert_eq!(density(&w, f, tall), 0.0);
        grow_daily(&rule, &mut w, f, &env, 20);
        let back = stand.iter().filter(|&&p| w.at(f, p) == tall).count();
        assert!(
            back as f64 > 0.9 * stand.len() as f64,
            "{back} of {} eaten tufts regrew in 20 days",
            stand.len()
        );
    }
);

both_faces!(
    fn a_load_catches_up_a_bounded_number_of_days(f: FaceFrame) {
        let rule = Rule::build(7);
        let tall = rule.block();
        let catch = rule.data.catch_up_days;
        assert_eq!(rule.catch_up(0, 1000), (1000 - catch, catch));
        assert_eq!(rule.catch_up(995, 1000), (995, 5));
        assert_eq!(rule.catch_up(1000, 1000), (1000, 0));
        let env = climate(1.0);
        // A chunk loaded at day 1000 that last grew at day 0 grows the bounded days only, the same
        // as growing those days on the spot.
        let mut loaded = meadow(f, require("dwell:grass"), tall);
        let (first, days) = rule.catch_up(0, 1000);
        let writes = rule.grow_chunk(f, chunk(f), first, days, &env, &mut loaded);
        let mut stepped = meadow(f, require("dwell:grass"), tall);
        for d in first..first + days {
            let w = rule.grow_chunk(f, chunk(f), d, 1, &env, &mut stepped);
            stepped.apply(&w);
        }
        loaded.apply(&writes);
        assert_eq!(density(&loaded, f, tall), density(&stepped, f, tall));
        assert!(
            density(&loaded, f, tall) < rule.cap(1.0),
            "bounded: not a filled meadow"
        );
    }
);

both_faces!(
    fn a_rule_added_by_data_alone_grows_on_its_ground(f: FaceFrame) {
        let fixture = GrassData {
            block: "dwell:tall_grass",
            on: &["dwell:dirt"],
            max_density: 1.0,
            spread_per_day: 1.0,
            regrow_per_day: 0.0,
            reach: 2,
            open_above: 1,
            catch_up_days: 5,
            ..GRASS
        };
        let rule = Rule::new(fixture, 3);
        let tall = rule.block();
        let mut w = meadow(f, require("dwell:dirt"), tall);
        let env = climate(1.0);
        grow_daily(&rule, &mut w, f, &env, 3);
        // Reach 2 at a chance of 1: a 13 × 13 square in 3 days.
        assert_eq!(density(&w, f, tall), 169.0 / 1024.0);
        let mut on_grass = meadow(f, require("dwell:grass"), tall);
        grow_daily(&rule, &mut on_grass, f, &env, 3);
        assert_eq!(density(&on_grass, f, tall), 1.0 / 1024.0, "not its ground");
    }
);

#[test]
fn the_builds_rule_resolves_by_name() {
    let rule = Rule::build(1);
    assert_eq!(rule.block(), require("dwell:tall_grass"));
    assert!(
        dwell_worldgen::decorations::DECORATIONS
            .iter()
            .any(|d| d.name == GRASS.decoration)
    );
}
