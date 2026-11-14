//! Tall grass growing and spreading (ADR 0075 T3c, docs/ECOLOGY.md §10), on both faces: it spreads
//! over days up to and never past the climate's cap, a dry stretch stays sparse, a player's block
//! wins, eaten grass regrows, a load catches up a bounded number of days, a rule added by data
//! alone grows on its own ground, and tufts regrow and spread over ½ m grass ramps (`onRamps`) but
//! never over a 1 m ramp.

use std::collections::HashMap;

use dwell_blocks::{AIR, StateId, require};
use dwell_world::ramps::{RampFacing, gentle_cell, steep_cells};
use dwell_world::{CHUNK_SIZE, ChunkCoord, FLAT_TOP_Y, FaceFrame, both_faces};
use dwell_worldgen::grass::{Cells, GRASS, GrassData, GrassStage, Rule, generated_tuft};
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

/// Whether `s` is tall grass at any stage.
fn is_tuft(s: StateId) -> bool {
    s != AIR && dwell_blocks::block_of(s).name == "dwell:tall_grass"
}

/// The share of the floor's cells holding `block` (tall grass: at any stage).
fn density(w: &World, f: FaceFrame, block: StateId) -> f64 {
    let mut n = 0;
    for x in 0..CHUNK_SIZE {
        for z in 0..CHUNK_SIZE {
            let s = w.at(f, [x, FLOOR + 1, z]);
            n += usize::from(s == block || is_tuft(block) && is_tuft(s));
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
                if is_tuft(w.at(f, [x, FLOOR + 1, z])) && (x, z) != (16, 16) {
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
        let back = stand.iter().filter(|&&p| is_tuft(w.at(f, p))).count();
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
        // The stages too: every cell the same, grown in one call or day by day.
        let mut a: Vec<_> = loaded.now.iter().filter(|(_, s)| **s != AIR).collect();
        let mut b: Vec<_> = stepped.now.iter().filter(|(_, s)| **s != AIR).collect();
        a.sort();
        b.sort();
        assert_eq!(a, b);
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

/// A ½ m grass ramp (face-local, upright) descending toward `facing`.
fn half_ramp(facing: RampFacing) -> StateId {
    gentle_cell("dwell:grass", facing, false).expect("grass has ½ m ramps")
}

/// The pieces of a 1 m grass ramp (face-local, from the bottom).
fn metre_ramp(facing: RampFacing) -> Vec<StateId> {
    steep_cells("dwell:grass", facing, false).expect("grass has 1 m ramps")
}

both_faces!(
    fn a_cut_tuft_on_a_ramp_regrows(f: FaceFrame) {
        let rule = Rule::build(7);
        let tall = rule.block();
        let env = climate(1.0);
        // Generated tufts over ½ m ramps on the fertile columns (as worldgen's `onRamps` puts
        // them, in the cell over the ramp), all cut.
        let mut w = meadow(f, require("dwell:grass"), AIR);
        let mut stand = Vec::new();
        for x in 0..CHUNK_SIZE {
            for z in 0..CHUNK_SIZE {
                if (x + z) % 5 == 0 && rule.fertile(env(x, z)) {
                    let facing = RampFacing::ALL[(x as usize) % 4];
                    w.natural(f, [x, FLOOR + 1, z], half_ramp(facing));
                    w.natural(f, [x, FLOOR + 2, z], tall);
                    stand.push([x, FLOOR + 2, z]);
                }
            }
        }
        for &p in &stand {
            w.edit(f, p, AIR);
        }
        grow_daily(&rule, &mut w, f, &env, 20);
        let back = stand.iter().filter(|&&p| is_tuft(w.at(f, p))).count();
        assert!(
            back as f64 > 0.9 * stand.len() as f64,
            "{back} of {} cut ramp tufts regrew in 20 days",
            stand.len()
        );
    }
);

both_faces!(
    fn grass_spreads_onto_half_metre_ramps_up_to_the_cap_and_never_onto_a_metre_ramp(f: FaceFrame) {
        let rule = Rule::build(7);
        let tall = rule.block();
        let env = climate(1.0);
        let mut w = meadow(f, require("dwell:grass"), tall);
        // A band of ½ m ramps east of the tuft and a band of 1 m ramps west of it, flat ground
        // between, none with generated grass.
        let (half, metre) = (18..26, 4..12);
        for z in 0..CHUNK_SIZE {
            for x in half.clone() {
                w.natural(f, [x, FLOOR + 1, z], half_ramp(RampFacing::East));
            }
            for x in metre.clone() {
                for (i, &s) in metre_ramp(RampFacing::West).iter().enumerate() {
                    w.natural(f, [x, FLOOR + 1 + i as i32, z], s);
                }
            }
        }
        grow_daily(&rule, &mut w, f, &env, 300);
        let mut on_ramps = 0;
        let mut fertile = 0;
        for z in 0..CHUNK_SIZE {
            for x in half.clone() {
                fertile += usize::from(rule.fertile(env(x, z)));
                if is_tuft(w.at(f, [x, FLOOR + 2, z])) {
                    on_ramps += 1;
                    assert!(rule.fertile(env(x, z)), "({x}, {z}) is not fertile");
                }
            }
            for x in metre.clone() {
                for y in FLOOR + 1..FLOOR + 5 {
                    assert!(
                        !is_tuft(w.at(f, [x, y, z])),
                        "tall grass on a 1 m ramp at {x}, {y}, {z}"
                    );
                }
            }
        }
        // The climate's cap holds over ramps as on flat: only fertile columns, and they fill.
        assert!(
            on_ramps as f64 > 0.8 * fertile as f64,
            "spread onto {on_ramps} of {fertile} fertile ramp cells"
        );
    }
);

both_faces!(
    fn a_lone_steep_piece_is_not_ground(f: FaceFrame) {
        // A 1 m ramp's upper piece broken off leaves its lower piece: still not ground for a tuft.
        let rule = Rule::build(7);
        let tall = rule.block();
        let env = climate(1.0);
        let mut w = meadow(f, require("dwell:grass"), tall);
        let lower = metre_ramp(RampFacing::North)[0];
        for x in 0..CHUNK_SIZE {
            for z in 0..CHUNK_SIZE {
                if (x, z) != (16, 16) && (x - 16).abs() <= 3 && (z - 16).abs() <= 3 {
                    w.natural(f, [x, FLOOR + 1, z], lower);
                }
            }
        }
        grow_daily(&rule, &mut w, f, &env, 100);
        for x in 13..20 {
            for z in 13..20 {
                assert!(
                    !is_tuft(w.at(f, [x, FLOOR + 2, z])),
                    "a tuft on a steep piece at {x}, {z}"
                );
            }
        }
    }
);

both_faces!(
    fn a_rule_without_on_ramps_keeps_off_ramps(f: FaceFrame) {
        // The same rule by data alone with `onRamps` off: ramps are not its ground.
        let rule = Rule::new(
            GrassData {
                on_ramps: false,
                ..GRASS
            },
            7,
        );
        let tall = rule.block();
        assert!(Rule::build(7).grows_on(half_ramp(RampFacing::South)));
        assert!(!rule.grows_on(half_ramp(RampFacing::South)));
        let env = climate(1.0);
        let mut w = meadow(f, require("dwell:grass"), tall);
        for x in 18..26 {
            for z in 0..CHUNK_SIZE {
                w.natural(f, [x, FLOOR + 1, z], half_ramp(RampFacing::South));
            }
        }
        grow_daily(&rule, &mut w, f, &env, 300);
        for x in 18..26 {
            for z in 0..CHUNK_SIZE {
                assert!(
                    !is_tuft(w.at(f, [x, FLOOR + 2, z])),
                    "a tuft on a ramp at {x}, {z}"
                );
            }
        }
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

// ---- Growing in height (docs/ECOLOGY.md §10, owner 2026-11-12) ----

/// A chunk-wide floor of grass with a generated tuft of state `tuft` on every column.
fn lawn(f: FaceFrame, tuft: StateId) -> World {
    let mut w = World::default();
    for x in 0..CHUNK_SIZE {
        for z in 0..CHUNK_SIZE {
            w.natural(f, [x, FLOOR, z], require("dwell:grass"));
            w.natural(f, [x, FLOOR + 1, z], tuft);
        }
    }
    w
}

/// Each floor tuft's stage (None: no tuft).
fn stages(rule: &Rule, w: &World, f: FaceFrame) -> Vec<usize> {
    let mut out = Vec::new();
    for x in 0..CHUNK_SIZE {
        for z in 0..CHUNK_SIZE {
            out.extend(rule.stage_of(w.at(f, [x, FLOOR + 1, z])));
        }
    }
    out
}

fn mean(v: &[usize]) -> f64 {
    v.iter().sum::<usize>() as f64 / v.len().max(1) as f64
}

#[test]
fn the_default_stage_is_full_height() {
    // face-a-only: block states and the data, no world cells.
    let rule = Rule::build(1);
    let full = GRASS.stages.len() - 1;
    assert!(GRASS.stages.len() >= 4, "at least 4 stages");
    assert_eq!(
        rule.block(),
        rule.stage_state(full),
        "the default is the last stage"
    );
    assert_eq!(
        dwell_blocks::parse_state("dwell:tall_grass").unwrap(),
        rule.stage_state(full),
        "tall grass saved before stages reads as full height"
    );
    assert_eq!(rule.stage_of(rule.block()), Some(full));
    assert_eq!(rule.stage_of(require("dwell:grass")), None);
    // A sprout is shorter than every later stage (the block's cross heights).
    let h = |i: usize| {
        dwell_blocks::info(rule.stage_state(i))
            .cross
            .unwrap()
            .height
    };
    for i in 1..=full {
        assert!(h(i) > h(i - 1), "stage {i} is taller than stage {}", i - 1);
    }
    assert_eq!(h(full), 0.85, "full height as before");
    // A week or more of game days from a sprout to full height.
    let days: f64 = GRASS.stages.iter().map(|s| s.days).sum();
    assert!(days >= 7.0, "{days} days to full height");
}

both_faces!(
    fn sprouts_grow_a_stage_at_a_time_to_full_height_over_a_week_or_more(f: FaceFrame) {
        let rule = Rule::build(7);
        let full = GRASS.stages.len() - 1;
        let mut w = lawn(f, rule.sprout());
        let env = climate(1.0);
        let mut last = mean(&stages(&rule, &w, f));
        assert_eq!(last, 0.0);
        let mut by_day = Vec::new();
        for day in 0..40 {
            let writes = rule.grow_chunk(f, chunk(f), day, 1, &env, &mut w);
            // A tuft grows at most one stage a day.
            for &(c, s) in &writes {
                let was = rule.stage_of(f.state(*w.now.get(&c).unwrap())).unwrap();
                assert_eq!(rule.stage_of(f.state(s)), Some(was + 1), "one stage a day");
            }
            w.apply(&writes);
            let now = mean(&stages(&rule, &w, f));
            assert!(now >= last, "stages never fall: {last} → {now}");
            last = now;
            by_day.push(stages(&rule, &w, f));
        }
        let full_by = |d: usize| by_day[d].iter().filter(|&&s| s == full).count() as f64 / 1024.0;
        // Memoryless (a stage is all a tuft keeps): a few race, but most take a week or more.
        assert!(full_by(3) < 0.1, "few full in 4 days: {}", full_by(3));
        assert!(
            full_by(6) < 0.5,
            "most not yet full in a week: {}",
            full_by(6)
        );
        assert!(full_by(39) > 0.99, "all full in 40 days: {}", full_by(39));
    }
);

both_faces!(
    fn the_climate_caps_the_top_stage(f: FaceFrame) {
        let rule = Rule::build(7);
        let full = GRASS.stages.len() - 1;
        assert_eq!(rule.top_stage(1.0), full, "a lush meadow grows full");
        assert_eq!(rule.top_stage(0.0), 0, "no share: sprouts only");
        // Each stage's share, by the data: a column just short of it stays a stage lower.
        for (i, st) in GRASS.stages.iter().enumerate().skip(1) {
            assert_eq!(rule.top_stage(st.share), i);
            assert_eq!(rule.top_stage(st.share - 1e-6), i - 1);
        }
        for share in [0.1, 0.3, 0.5] {
            let top = rule.top_stage(share);
            assert!(top < full, "a drier climate is shorter: {share} → {top}");
            let mut w = lawn(f, rule.sprout());
            grow_daily(&rule, &mut w, f, &climate(share), 120);
            let st = stages(&rule, &w, f);
            assert!(
                st.iter().all(|&s| s <= top),
                "share {share}: none past {top}"
            );
            assert!(
                st.iter().filter(|&&s| s == top).count() > 1000,
                "share {share}: they reach {top}"
            );
        }
        // A tuft past its climate's top (placed full by hand) is never cut back.
        let mut w = lawn(f, rule.block());
        grow_daily(&rule, &mut w, f, &climate(0.1), 30);
        assert!(stages(&rule, &w, f).iter().all(|&s| s == full));
    }
);

both_faces!(
    fn new_and_regrown_tufts_start_as_a_sprout(f: FaceFrame) {
        let rule = Rule::build(7);
        let tall = rule.block();
        let env = climate(1.0);
        // One generated tuft, eaten; its neighbours bare: day by day, what appears is a sprout.
        let mut w = meadow(f, require("dwell:grass"), tall);
        w.edit(f, [16, FLOOR + 1, 16], rule.eaten(require("dwell:grass")));
        assert_eq!(
            w.at(f, [16, FLOOR + 1, 16]),
            rule.sprout(),
            "eaten: a sprout"
        );
        for day in 0..30 {
            let writes = rule.grow_chunk(f, chunk(f), day, 1, &env, &mut w);
            for &(c, s) in &writes {
                if *w.now.get(&c).unwrap_or(&AIR) == AIR {
                    assert_eq!(f.state(s), rule.sprout(), "a new tuft is a sprout");
                }
            }
            w.apply(&writes);
        }
        assert_eq!(
            w.at(f, [16, FLOOR + 1, 16]),
            tall,
            "the eaten tuft grew back to full height"
        );
        // Eaten off anything but the rule's ground, it is gone.
        assert_eq!(rule.eaten(require("dwell:stone")), AIR);
    }
);

both_faces!(
    fn stages_by_data_alone(f: FaceFrame) {
        // A fixture rule whose tufts race up (a day a stage) but only where the share is high.
        static STAGES: [GrassStage; 4] = [
            GrassStage {
                value: "0",
                days: 1.0,
                share: 0.0,
            },
            GrassStage {
                value: "1",
                days: 1.0,
                share: 0.0,
            },
            GrassStage {
                value: "2",
                days: 1.0,
                share: 0.0,
            },
            GrassStage {
                value: "3",
                days: 0.0,
                share: 0.9,
            },
        ];
        let rule = Rule::new(
            GrassData {
                stages: &STAGES,
                ..GRASS
            },
            3,
        );
        let mut w = lawn(f, rule.sprout());
        grow_daily(&rule, &mut w, f, &climate(1.0), 3);
        assert!(
            stages(&rule, &w, f).iter().all(|&s| s == 3),
            "full in 3 days"
        );
        let mut dry = lawn(f, rule.sprout());
        grow_daily(&rule, &mut dry, f, &climate(0.5), 10);
        assert!(
            stages(&rule, &dry, f).iter().all(|&s| s == 2),
            "capped at 2"
        );
    }
);

#[test]
fn generation_places_tall_grass_at_its_climates_top_stage() {
    // face-a-only: the state chosen for a share (the terrain places it face-local).
    let rule = Rule::build(1);
    let tall = rule.block();
    assert_eq!(
        generated_tuft(GRASS.decoration, tall, 1.0),
        tall,
        "a lush meadow: full as before"
    );
    for share in [0.0, 0.1, 0.3, 0.5, 0.7, 1.0] {
        assert_eq!(
            generated_tuft(GRASS.decoration, tall, share),
            rule.stage_state(rule.top_stage(share))
        );
    }
    assert!(
        rule.stage_of(generated_tuft(GRASS.decoration, tall, 0.1))
            .unwrap()
            < rule.stage_of(tall).unwrap(),
        "dry ground grows short grass"
    );
    let stone = require("dwell:stone");
    assert_eq!(
        generated_tuft(GRASS.decoration, stone, 0.1),
        stone,
        "not a tuft: as it is"
    );
    assert_eq!(
        generated_tuft("other", tall, 0.1),
        tall,
        "another decoration: as it is"
    );
}
