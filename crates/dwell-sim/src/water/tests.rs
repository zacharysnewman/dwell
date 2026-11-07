//! The water grid's tests (Phase 53 G2, docs/WATER_SURFACE_GRID.md): scenes built face-locally and
//! run on both faces.

use super::*;
use dwell_blocks::{info, require};
use dwell_world::{FLAT_TOP_Y, empty_generator};

/// A scene in face `f`'s local frame: x, a row offset from `BASE`, z. Stone below `BASE`, air above.
struct Scene {
    world: VoxelWorld,
    f: FaceFrame,
    water: Water,
}

const BASE: i32 = FLAT_TOP_Y + 40;

impl Scene {
    fn new(f: FaceFrame) -> Self {
        let mut s = Self {
            world: VoxelWorld::new(empty_generator(), None),
            f,
            water: Water::new(),
        };
        let stone = require("dwell:stone");
        s.fill([-24, -6, -24], [40, -1, 40], stone);
        s.fill([-24, 0, -24], [40, 12, 40], M.air);
        s
    }
    fn cell(&self, x: i32, r: i32, z: i32) -> [i32; 3] {
        self.f.cell([x, BASE + r, z])
    }
    fn set(&mut self, x: i32, r: i32, z: i32, m: StateId) {
        let c = self.cell(x, r, z);
        self.world.set_voxel(c[0], c[1], c[2], m);
    }
    fn get(&self, x: i32, r: i32, z: i32) -> StateId {
        self.world.get_voxel_if_loaded(self.cell(x, r, z))
    }
    fn fill(&mut self, lo: [i32; 3], hi: [i32; 3], m: StateId) {
        for x in lo[0]..=hi[0] {
            for r in lo[1]..=hi[1] {
                for z in lo[2]..=hi[2] {
                    self.set(x, r, z, m);
                }
            }
        }
    }
    /// An edit: the cells set, the water told what was there.
    fn edit(&mut self, cells: &[(i32, i32, i32)], m: StateId) {
        let mut old = Vec::new();
        for &(x, r, z) in cells {
            old.push((self.cell(x, r, z), self.get(x, r, z)));
            self.set(x, r, z, m);
        }
        self.water.touch(old);
    }
    /// `secs` of water time in ticks of 1 s.
    fn run(&mut self, secs: usize) -> usize {
        let mut writes = 0;
        for _ in 0..secs {
            writes += self.water.tick(&mut self.world, &[], 1.0).len();
        }
        writes
    }
    /// The level at face-local column (x, z), face-local m above `BASE`'s row.
    fn level(&self, x: i32, z: i32) -> Option<f64> {
        self.water
            .level_at(self.f, x, z)
            .map(|l| l - m_of_row(self.f.row(self.f.row(BASE))))
    }
    /// A walled basin: stone walls (rows 0..=4) around [x0, x1] × [z0, z1], water `rows` deep.
    fn basin(&mut self, x0: i32, x1: i32, z0: i32, z1: i32, rows: i32) {
        let stone = require("dwell:stone");
        for r in 0..=4 {
            for x in x0 - 1..=x1 + 1 {
                self.set(x, r, z0 - 1, stone);
                self.set(x, r, z1 + 1, stone);
            }
            for z in z0 - 1..=z1 + 1 {
                self.set(x0 - 1, r, z, stone);
                self.set(x1 + 1, r, z, stone);
            }
        }
        self.fill([x0, 0, z0], [x1, rows - 1, z1], M.water);
    }
}

dwell_world::both_faces!(
    fn generated_water_rests_and_costs_nothing(f: FaceFrame) {
        let mut s = Scene::new(f);
        s.basin(0, 7, 0, 7, 3);
        let writes = s.run(30);
        assert_eq!(writes, 0, "water at rest writes nothing");
        assert_eq!(s.water.awake_chunks(), 0, "and isn't stepped");
        assert!(s.water.held_at(f, 3, 3), "it is held as found");
        let lv = s.level(3, 3).expect("water");
        assert!((lv - 1.5).abs() < 1e-3, "found at its level: {lv}");
    }
);

dwell_world::both_faces!(
    fn a_lake_cut_open_drains_into_a_pit_to_one_level(f: FaceFrame) {
        let mut s = Scene::new(f);
        // a lake 8 × 8, 3 rows (1.5 m) deep; a dry pit 8 × 8 beside it, 3 rows deep below the
        // lake's floor
        s.basin(0, 7, 0, 7, 3);
        let stone = require("dwell:stone");
        // the pit: its floor three rows down, its walls up to row 4
        s.fill([10, -3, 0], [17, -1, 7], M.air);
        for r in -3..=4 {
            for z in -1..=8 {
                s.set(18, r, z, stone);
            }
            for x in 9..=18 {
                s.set(x, r, -1, stone);
                s.set(x, r, 8, stone);
            }
        }
        // the lake's wall (x = 8) stands between: cut a gap 4 wide down to the lake's floor
        s.run(5);
        let cut: Vec<(i32, i32, i32)> = (2..=5)
            .flat_map(|z| (0..=4).map(move |r| (8, r, z)))
            .collect();
        s.edit(&cut, M.air);
        s.edit(
            &(2..=5)
                .flat_map(|z| (0..=4).map(move |r| (9, r, z)))
                .collect::<Vec<_>>(),
            M.air,
        );
        s.run(600);
        let (lake, pit) = (s.level(3, 3).expect("lake"), s.level(14, 3).expect("pit"));
        assert!(
            (lake - pit).abs() < 0.05,
            "one level: lake {lake:.3}, pit {pit:.3}"
        );
        // the lake's 96 m³ shared over the lake, the pit, and the cut: the level well below its start
        assert!(lake < 1.0 && lake > -0.5, "the level fell: {lake:.3}");
        // the pit's cells are water blocks
        assert!(
            info(s.get(14, -3, 3)).liquid,
            "the pit's bottom cell is water"
        );
    }
);

dwell_world::both_faces!(
    fn free_water_is_conserved(f: FaceFrame) {
        let mut s = Scene::new(f);
        s.basin(0, 9, 0, 5, 4);
        // free it all: an edit inside it (a block set and taken away at once)
        let stone = require("dwell:stone");
        s.edit(&[(4, 0, 2)], stone);
        s.run(1);
        for x in 0..=9 {
            for z in 0..=5 {
                s.edit(&[(x, 5, z)], M.air);
            }
        }
        s.run(1);
        let v0 = s.water.free_volume();
        assert!(v0 > 50.0, "most of the basin is free: {v0}");
        // a wave: water taken from one end and set at the other changes nothing in sum
        s.run(120);
        let v1 = s.water.free_volume();
        assert!((v1 - v0).abs() < 0.01, "conserved: {v0:.4} → {v1:.4}");
    }
);

dwell_world::both_faces!(
    fn the_free_water_survives_a_save(f: FaceFrame) {
        let mut s = Scene::new(f);
        s.basin(0, 7, 0, 7, 3);
        let stone = require("dwell:stone");
        // a block dropped in the lake frees it
        s.edit(&[(3, 0, 3)], stone);
        s.run(30);
        let bytes = s.water.save(|_| 0);
        let mut back = Water::new();
        assert!(back.restore(&bytes, |_| 0).is_some(), "the record reads");
        assert!(
            (back.free_volume() - s.water.free_volume()).abs() < 1e-3,
            "the same water"
        );
        let (a, b) = (s.water.level_at(f, 1, 1), back.level_at(f, 1, 1));
        assert_eq!(
            a.map(|v| (v * 1e4).round()),
            b.map(|v| (v * 1e4).round()),
            "the same level"
        );
        assert!(
            Water::new().restore(b"DWW5....", |_| 0).is_none(),
            "an older world's record isn't read"
        );
    }
);

impl Scene {
    /// A drawn river as the generator draws them: a channel 3 wide (z 0..=2) along x 0..40, its
    /// floor a row lower every 8 columns, each pool flat, 2 rows deep. Returns its cells' states.
    fn stepped_river(&mut self) {
        let stone = require("dwell:stone");
        for x in -1..=40 {
            let drop = x.max(0) / 8;
            for r in -6..=6 {
                for z in -1..=3 {
                    let wall = x == -1 || x == 40 || z == -1 || z == 3;
                    let bed = r < -drop;
                    let water = !wall && !bed && r < 2 - drop;
                    let m = if wall || bed {
                        stone
                    } else if water {
                        M.water
                    } else {
                        M.air
                    };
                    self.set(x, r, z, m);
                }
            }
        }
    }
    fn snapshot(&self) -> Vec<StateId> {
        let mut v = Vec::new();
        for x in -1..=40 {
            for r in -6..=6 {
                for z in -1..=3 {
                    v.push(self.get(x, r, z));
                }
            }
        }
        v
    }
}

dwell_world::both_faces!(
    fn a_drawn_rivers_steps_rest_when_an_edit_frees_them(f: FaceFrame) {
        // A generated river is drawn as flat pools in steps, not as a steady flow: freed as it is,
        // every step would pour, and the release spread the change down the whole river (a stone
        // set in a seed-0 river's bed emptied 844 cells). Stepped beside the water as generated,
        // the freed river rests exactly as drawn.
        let mut s = Scene::new(f);
        s.stepped_river();
        s.run(2);
        let before = s.snapshot();
        let stone = require("dwell:stone");
        // a stone in the second pool's bed (x 8..=15, its floor row -1), away from its steps
        s.edit(&[(11, -1, 1)], stone);
        s.run(600);
        let after = s.snapshot();
        let changed = before.iter().zip(&after).filter(|(a, b)| a != b).count();
        assert!(
            changed <= 2,
            "{changed} cells changed (the stone, and the water it lifted)"
        );
        assert_eq!(s.water.awake_chunks(), 0, "settled");
    }
);

dwell_world::both_faces!(
    fn a_dam_across_a_drawn_river_backs_it_up(f: FaceFrame) {
        // The flow the river was drawn with goes on: dammed, the pool above rises and the one
        // below falls. Two water minutes: the reach below is closed (its water stays in it since
        // 0.49.25, the rest's sinks no longer taking it, BUGS.md #41), and later the pool above,
        // spilling over the channel's walls onto the plain round it, fills the plain until it
        // comes back in.
        let mut s = Scene::new(f);
        s.stepped_river();
        s.run(2);
        let (up0, down0) = (
            s.level(12, 1).expect("above"),
            s.level(20, 1).expect("below"),
        );
        let stone = require("dwell:stone");
        let dam: Vec<(i32, i32, i32)> = (0..=2)
            .flat_map(|z| (-1..=12).map(move |r| (16, r, z)))
            .collect();
        s.edit(&dam, stone);
        s.run(120);
        let (up, down) = (
            s.level(12, 1).expect("above"),
            s.level(20, 1).expect("below"),
        );
        assert!(
            up > up0 + 0.05,
            "the pool above the dam rose: {up0:.3} → {up:.3}"
        );
        assert!(
            down < down0 - 0.05,
            "the pool below fell: {down0:.3} → {down:.3}"
        );
    }
);

impl Scene {
    /// A generated river's level reach, as the generator draws it since G1a: a channel 3 wide
    /// (z 0..=2) along x 0..40 between walls 3.5 m tall, its bed flat (row 0), its water `rows`
    /// deep and flat, no steps. The water is the river's by a fixture map running `Q` m²/s along
    /// +x (and its current that over the depth, as `TerrainGenerator::river_current`).
    fn flat_river(&mut self, rows: i32) {
        let stone = require("dwell:stone");
        for x in -1..=40 {
            for r in -6..=6 {
                for z in -1..=3 {
                    let wall = x == -1 || x == 40 || z == -1 || z == 3;
                    let m = if wall || r < 0 {
                        stone
                    } else if r < rows {
                        M.water
                    } else {
                        M.air
                    };
                    self.set(x, r, z, m);
                }
            }
        }
    }
    fn with_river_map(&mut self) {
        self.water = river_map(Water::new());
    }
    /// The current at face-local column (x, z), mid-river, from the current table.
    fn current(&self, x: i32, z: i32) -> [f32; 2] {
        self.world.currents.at(self.f.point([
            f64::from(x) + 0.5,
            m_of_row(BASE) + 0.2,
            f64::from(z) + 0.5,
        ]))
    }
    /// A dam across the flat river at x 24.
    fn dam_flat_river(&mut self) {
        let dam: Vec<(i32, i32, i32)> = (0..=2)
            .flat_map(|z| (0..=6).map(move |r| (24, r, z)))
            .collect();
        self.edit(&dam, require("dwell:stone"));
    }
}

/// The fixture river's flow (m²/s along +x).
const Q: f64 = 0.1;

fn river_map(w: Water) -> Water {
    w.with_rivers(std::sync::Arc::new(|_, _, _, depth| {
        (depth >= 0.05).then_some([(Q / depth).min(1.5), 0.0])
    }))
    .with_river_flow(std::sync::Arc::new(|_, _, _| Some([Q, 0.0])))
}

dwell_world::both_faces!(
    fn a_dam_across_a_flat_river_backs_it_up_by_the_maps_flow(f: FaceFrame) {
        // Owner, 2026-11-06 (BUGS.md #38): "damming should raise the water and does". A level
        // reach is drawn flat, so by the law alone the water as generated carried no flow and a
        // dam had nothing to back up (red: 0 cm). Carrying the map's flow, the reach above the
        // dam rises by it (0.3 m³/s over 72 m²) and the reach below it falls.
        let mut s = Scene::new(f);
        s.with_river_map();
        s.flat_river(2);
        s.run(2);
        let (up0, down0) = (
            s.level(12, 1).expect("above"),
            s.level(30, 1).expect("below"),
        );
        s.dam_flat_river();
        s.run(300);
        let (up, down) = (
            s.level(12, 1).expect("above"),
            s.level(30, 1).unwrap_or(0.0),
        );
        assert!(
            up > up0 + 0.25,
            "the reach above the dam rose: {up0:.3} → {up:.3}"
        );
        assert!(
            down < down0 - 0.05,
            "the reach below fell: {down0:.3} → {down:.3}"
        );
    }
);

dwell_world::both_faces!(
    fn a_flowing_flat_river_rests_when_an_edit_frees_it(f: FaceFrame) {
        // The flow is carried through the water as generated as through the water as it is: an
        // untouched river, freed by a stone in its bed, stays as drawn (no drift, no release
        // down the river) and sleeps again.
        let mut s = Scene::new(f);
        s.with_river_map();
        s.flat_river(2);
        s.run(2);
        let before = s.snapshot();
        s.edit(&[(11, 0, 1)], require("dwell:stone"));
        s.run(600);
        let after = s.snapshot();
        let changed = before.iter().zip(&after).filter(|(a, b)| a != b).count();
        assert!(
            changed <= 2,
            "{changed} cells changed (the stone, and the water it lifted)"
        );
        assert_eq!(s.water.awake_chunks(), 0, "settled");
        let lv = s.level(30, 1).expect("water");
        assert!((lv - 1.0).abs() < 0.03, "the river kept its level: {lv:.3}");
    }
);

dwell_world::both_faces!(
    fn a_shallow_flat_reach_runs_downstream_held_and_freed(f: FaceFrame) {
        // Owner, 2026-11-06: "dead rivers ... the shallow ones missing currents". One cell deep
        // and level, the river still runs downstream in the current table (swimmers, bodies,
        // the flow shown): held, by its map; freed by an edit, by the flow it carries (red: the
        // law alone left a freed level reach still).
        let mut s = Scene::new(f);
        s.with_river_map();
        s.flat_river(1);
        s.run(1);
        let [u, v] = s.current(10, 1);
        assert!(
            u > 0.1 && v.abs() < 0.01,
            "held, it runs: {u:.3}, {v:.3} m/s"
        );
        // A wall beside it cut and set again: the water near it is free, the reach unchanged.
        s.edit(&[(10, 0, 3)], M.air);
        s.edit(&[(10, 0, 3)], require("dwell:stone"));
        s.run(5);
        assert!(!s.water.held_at(f, 10, 1), "freed");
        let [u, v] = s.current(10, 1);
        assert!(
            u > 0.1 && v.abs() < 0.05,
            "freed, it runs on downstream: {u:.3}, {v:.3} m/s"
        );
    }
);

dwell_world::both_faces!(
    fn a_world_saved_before_the_rivers_flow_backs_up_when_dammed(f: FaceFrame) {
        // Owner, 2026-11-06: existing worlds are fixed on load. A record saved by a build whose
        // water carried no river flow (its rest flows ~0) is restored as it is (the same record,
        // DWG1); the flow is the map's, found again for its generated columns, so the reach runs
        // and a dam backs it up.
        let mut s = Scene::new(f);
        s.flat_river(2);
        s.edit(&[(11, 0, 1)], require("dwell:stone"));
        s.run(30);
        assert!(!s.water.held_at(f, 12, 1), "free water to save");
        let bytes = s.water.save(|_| 0);
        let mut back = river_map(Water::new());
        assert!(
            back.restore(&bytes, |_| 0).is_some(),
            "the old record reads"
        );
        s.water = back;
        s.run(5);
        let [u, _] = s.current(12, 1);
        assert!(u > 0.04, "the restored reach runs: {u:.3} m/s");
        let up0 = s.level(12, 1).expect("above");
        s.dam_flat_river();
        s.run(300);
        let up = s.level(12, 1).expect("above");
        assert!(up > up0 + 0.25, "dammed, it rose: {up0:.3} → {up:.3}");
    }
);

#[test]
fn the_report_is_json() {
    // face-a-only: a diagnostic's format, not the water's behaviour
    let mut s = Scene::new(FaceFrame::A);
    s.basin(0, 3, 0, 3, 2);
    s.run(1);
    let at = s.cell(1, 0, 1);
    let v: serde_json::Value = serde_json::from_str(&s.water.report(at, 2)).expect("JSON");
    assert_eq!(v["held"], 16, "{v}");
    assert!(
        v["columns"].as_array().is_some_and(|c| !c.is_empty()),
        "{v}"
    );
}

dwell_world::both_faces!(
    fn a_lake_cut_open_has_a_current_toward_the_cut_until_it_settles(f: FaceFrame) {
        // Phase 53 G3a (docs/WATER_SURFACE_GRID.md §2c): the grid writes its awake water's
        // velocity into the world's currents, read by swimmers and floating bodies.
        let mut s = Scene::new(f);
        s.basin(0, 7, 0, 7, 3);
        s.run(5);
        let at = |s: &Scene, x: i32| {
            s.world
                .currents
                .at(s.f.point([f64::from(x) + 0.5, m_of_row(BASE) + 0.75, 3.5]))
        };
        assert_eq!(at(&s, 6), [0.0; 2], "a lake at rest is still");
        // open its +x wall onto the ground beyond, down to the lake's floor
        let cut: Vec<(i32, i32, i32)> = (8..=9)
            .flat_map(|x| (2..=5).flat_map(move |z| (0..=4).map(move |r| (x, r, z))))
            .collect();
        s.edit(&cut, M.air);
        s.run(2);
        let [u, v] = at(&s, 6);
        assert!(
            u > 0.1,
            "the water by the cut runs toward it: {u:.3}, {v:.3} m/s"
        );
        assert!(v.abs() < u, "mostly toward the cut: {u:.3}, {v:.3} m/s");
    }
);

dwell_world::both_faces!(
    fn a_liquid_given_by_data_alone_runs_by_its_own_roughness(f: FaceFrame) {
        // G4 (WATER_SURFACE_GRID.md §4): the grid's liquids and settings are data. A fixture
        // liquid ten times as rough, given only as flow data, drains the same cut lake slower.
        use dwell_world::flow::{FLOW, FlowData, LiquidFlow};
        static ROUGH: [LiquidFlow; 1] = [LiquidFlow {
            block: "dwell:water",
            manning: 0.3,
            weir: 1.7,
            running: "dwell:running_water",
        }];
        let rough = FlowData {
            liquids: &ROUGH,
            ..FLOW
        };
        let drained = |water: Water| {
            let mut s = Scene::new(f);
            s.water = water;
            s.basin(0, 7, 0, 7, 3);
            s.run(2);
            // a gap in the far wall down to the lake's floor, onto the open ground beyond
            let cut: Vec<(i32, i32, i32)> = (2..=5)
                .flat_map(|z| (0..=4).map(move |r| (8, r, z)))
                .collect();
            s.edit(&cut, M.air);
            s.run(20);
            1.5 - s.level(3, 3).expect("the lake")
        };
        let (plain, slow) = (
            drained(Water::new()),
            drained(Water::with_settings(Settings::of(&rough))),
        );
        assert!(plain > 0.05, "the lake drains: {plain:.3} m");
        assert!(
            slow < 0.7 * plain,
            "the rough liquid drains slower: {slow:.3} against {plain:.3} m"
        );
    }
);

dwell_world::both_faces!(
    fn held_water_runs_as_the_map_says_and_freed_water_by_the_law(f: FaceFrame) {
        // Phase 53 G3b: held (generated) water's current is the world map's river's, given here by
        // a fixture map running 0.4 m/s along +z; once an edit frees it, the law's.
        let mut s = Scene::new(f);
        s.water = Water::new().with_rivers(std::sync::Arc::new(|_, _, _, depth| {
            (depth >= 0.05).then_some([0.0, 0.4])
        }));
        s.basin(0, 7, 0, 7, 3);
        s.run(1);
        let at = |s: &Scene, x: i32| {
            s.world
                .currents
                .at(s.f.point([f64::from(x) + 0.5, m_of_row(BASE) + 0.75, 3.5]))
        };
        assert_eq!(at(&s, 3), [0.0, 0.4], "the held lake runs as its map says");
        // A wall cut by the lake: the water near it is free, and still (the law's: it is level).
        s.edit(&[(7, 0, 3)], M.air);
        s.edit(&[(7, 0, 3)], require("dwell:stone"));
        s.run(1);
        let [u, v] = at(&s, 6);
        assert!(
            u.abs() < 0.1 && v.abs() < 0.1,
            "freed, it moves by the law: {u:.3}, {v:.3} m/s"
        );
    }
);

dwell_world::both_faces!(
    fn a_water_tick_keeps_to_its_step_budget_however_much_water_is_awake(f: FaceFrame) {
        // Owner, 2026-11-05: damming a river stalled the game (the player stopped and started).
        // Water runs on game time (72 x real by the day's length): a water tick of 0.1 s was 7.2 s
        // of water, about 45 substeps over every awake chunk, 50 ms a tick on the server's thread.
        // A tick now does at most `stepBudget` column-substeps (at least one substep); the rest of
        // its water time is lost (the water runs slow), not the game.
        let mut s = Scene::new(f);
        s.basin(0, 30, 0, 30, 4);
        s.run(2);
        let cut: Vec<(i32, i32, i32)> = (31..=32)
            .flat_map(|x| (10..=20).flat_map(move |z| (0..=5).map(move |r| (x, r, z))))
            .collect();
        s.edit(&cut, M.air);
        let budget = s.water.cfg.step_budget;
        for _ in 0..20 {
            let (before, awake) = (s.water.substeps, s.water.awake_chunks());
            s.water.tick(&mut s.world, &[], 7.2);
            let steps = (s.water.substeps - before) as usize;
            assert!(
                steps <= (budget / (awake.max(1) * CA)).max(1),
                "{steps} substeps over {awake} awake chunks: past the budget of {budget} column-steps"
            );
        }
        assert!(s.water.awake_chunks() > 0, "the cut lake drains");
    }
);

/// A scene driven through a water host as the server drives it (ADR 0062): its chunks by the
/// mirror, its edits as `In::Edit`, a run a tick, the writes applied at the next tick's start
/// (dropped where the world has since made the cell solid).
struct Hosted {
    s: Scene,
    host: host::WaterHost,
    mirror: Mirror,
    pending: Vec<host::In>,
}

impl Hosted {
    fn new(f: FaceFrame, placement: host::Placement) -> Self {
        let s = Scene::new(f);
        Self {
            host: host::WaterHost::new(placement, Water::new()),
            s,
            mirror: Mirror::default(),
            pending: Vec::new(),
        }
    }
    fn apply(&mut self) {
        for o in self.host.poll() {
            if let host::Out::Ran { writes, .. } = o {
                for w in writes {
                    let loaded = self
                        .s
                        .world
                        .find(chunk_of(w.cell[0], w.cell[1], w.cell[2]))
                        .is_some();
                    if let Some(m) = w.apply_to(self.s.world.get_voxel_if_loaded(w.cell))
                        && loaded
                    {
                        self.s.world.write_voxel(w.cell[0], w.cell[1], w.cell[2], m);
                    }
                }
            }
        }
    }
    fn edit(&mut self, cells: &[(i32, i32, i32)], m: StateId) {
        for &(x, r, z) in cells {
            self.s.set(x, r, z, m);
            self.pending.push(host::In::Edit {
                cell: self.s.cell(x, r, z),
                byte: cells::byte_of(m),
            });
        }
    }
    /// `n` ticks of `dt` water s, each waited on (so the threaded host runs exactly as inline).
    fn run(&mut self, n: usize, dt: f64) {
        for _ in 0..n {
            self.apply();
            for ch in self.mirror.sync(&self.s.world) {
                self.pending.push(as_in(ch));
            }
            self.pending.push(host::In::Run(dt));
            self.host.send(std::mem::take(&mut self.pending));
            assert!(
                self.host.wait(std::time::Duration::from_secs(20)),
                "the water answers"
            );
        }
        self.apply();
    }
    fn record(&mut self) -> Vec<u8> {
        self.host.send(vec![host::In::Record]);
        assert!(self.host.wait(std::time::Duration::from_secs(20)));
        self.host
            .poll()
            .into_iter()
            .find_map(|o| match o {
                host::Out::Record(b) => Some(b),
                _ => None,
            })
            .expect("a record")
    }
    /// The scene's cells (rows -4..=6 over x, z in -2..=20).
    fn cells(&self) -> Vec<StateId> {
        let mut out = Vec::new();
        for x in -2..=20 {
            for r in -4..=6 {
                for z in -2..=20 {
                    out.push(self.s.get(x, r, z));
                }
            }
        }
        out
    }
}

fn as_in(ch: cells::Change) -> host::In {
    match ch {
        cells::Change::Load { at, cells, fresh } => host::In::Load { at, cells, fresh },
        cells::Change::Unload(at) => host::In::Unload(at),
    }
}

/// The pit scene of `a_lake_cut_open_drains_into_a_pit_to_one_level`, cut open, through a host.
fn pit_through(f: FaceFrame, placement: host::Placement) -> Hosted {
    let mut h = Hosted::new(f, placement);
    h.s.basin(0, 7, 0, 7, 3);
    let stone = require("dwell:stone");
    h.s.fill([10, -3, 0], [17, -1, 7], M.air);
    for r in -3..=4 {
        for z in -1..=8 {
            h.s.set(18, r, z, stone);
        }
        for x in 9..=18 {
            h.s.set(x, r, -1, stone);
            h.s.set(x, r, 8, stone);
        }
    }
    h.run(3, 1.0);
    let cut: Vec<(i32, i32, i32)> = (2..=5)
        .flat_map(|z| (0..=4).flat_map(move |r| [(8, r, z), (9, r, z)]))
        .collect();
    h.edit(&cut, M.air);
    h.run(60, 1.0);
    h
}

dwell_world::both_faces!(
    fn the_water_on_its_own_thread_runs_as_inline(f: FaceFrame) {
        // ADR 0062: the grid behind the host runs the same whether inline or on its thread (each
        // tick waited on here: the thread then sees exactly the batches inline does), and the cut
        // lake drains into the pit through either.
        let mut inline = pit_through(f, host::Placement::Inline);
        let mut thread = pit_through(f, host::Placement::Thread);
        assert!(
            info(inline.s.get(14, -3, 3)).liquid,
            "the lake ran into the pit"
        );
        assert!(
            inline.cells() == thread.cells(),
            "the same water in the world"
        );
        assert_eq!(inline.record(), thread.record(), "the same record");
    }
);

dwell_world::both_faces!(
    fn the_record_survives_the_host(f: FaceFrame) {
        // ADR 0062: the record asked of a threaded host restores into a new one.
        let mut a = pit_through(f, host::Placement::Thread);
        let record = a.record();
        let mut b = Hosted::new(f, host::Placement::Thread);
        b.s.world = std::mem::replace(
            &mut a.s.world,
            VoxelWorld::new(dwell_world::empty_generator(), None),
        );
        b.host.send(vec![host::In::Restore(record.clone())]);
        assert!(b.host.wait(std::time::Duration::from_secs(20)));
        assert!(
            b.host
                .poll()
                .iter()
                .any(|o| matches!(o, host::Out::Restored(Some(n)) if *n > 0)),
            "restored"
        );
        b.run(1, 0.0);
        assert_eq!(
            b.record()[..16],
            record[..16],
            "the same clock and chunk count"
        );
    }
);

dwell_world::both_faces!(
    fn a_threaded_water_owes_what_it_hasnt_run_and_drops_past_the_cap(f: FaceFrame) {
        // ADR 0062: off the server's thread the water falls behind instead of losing time: what a
        // run's budget didn't reach is owed and run next, up to `max_owed`; past it, dropped.
        let mut s = Scene::new(f);
        s.basin(0, 30, 0, 30, 4);
        let mut core = host::WaterCore::new(
            Water::new(),
            host::CoreConfig {
                budget: 256,
                max_owed: 20.0,
                slow: std::time::Duration::ZERO,
            },
        );
        let mut mirror = Mirror::default();
        let mut items: Vec<host::In> = mirror.sync(&s.world).into_iter().map(as_in).collect();
        core.receive(1, items);
        core.work();
        // an edit frees the lake, then a long run: the budget (one substep a run) can't keep up
        let stone = require("dwell:stone");
        s.set(15, 0, 15, stone);
        items = vec![
            host::In::Edit {
                cell: s.cell(15, 0, 15),
                byte: cells::byte_of(stone),
            },
            host::In::Run(30.0),
        ];
        core.receive(2, items);
        let stats = |outs: Vec<host::Out>| {
            outs.into_iter()
                .find_map(|o| match o {
                    host::Out::Ran { stats, .. } => Some(stats),
                    _ => None,
                })
                .expect("a run")
        };
        let first = stats(core.work());
        assert!(
            first.ran_s < 30.0,
            "the budget stopped it short: {}",
            first.ran_s
        );
        assert!(core.busy(), "the rest is owed");
        assert!(
            (first.owed_s - 20.0).abs() < 1e-6 && first.dropped_s > 0.0,
            "owed up to the cap, the rest dropped: {first:?}"
        );
        let second = stats(core.work());
        assert!(
            second.ran_s > 0.0 && second.owed_s < first.owed_s,
            "it runs what it owes: {second:?}"
        );
    }
);

dwell_world::both_faces!(
    fn water_risen_over_a_steep_slope_floods_its_ramps_and_dries_them_when_it_goes(f: FaceFrame) {
        // Owner, 2026-11-06 (BUGS.md #36): "the bottom half of steep slopes is still either not
        // flooded or visually not flooded". A ramp (a shaped block with a `flooded` state) was solid
        // to the grid, so water rising over it never flooded its open part. The pit scene of
        // `a_lake_cut_open_drains_into_a_pit_to_one_level`, a steep slope (two ramps) standing in
        // the pit and a single ramp on its floor.
        let mut s = Scene::new(f);
        s.basin(0, 7, 0, 7, 3);
        let stone = require("dwell:stone");
        s.fill([10, -3, 0], [17, -1, 7], M.air);
        for r in -3..=4 {
            for z in -1..=8 {
                s.set(18, r, z, stone);
            }
            for x in 9..=18 {
                s.set(x, r, -1, stone);
                s.set(x, r, 8, stone);
            }
        }
        let ramp = |shape: &str| {
            f.state(require(&format!(
                "dwell:stone_ramp[facing=east,flooded=false,half=bottom,shape={shape}]"
            )))
        };
        s.set(14, -3, 3, ramp("steep_lower"));
        s.set(14, -2, 3, ramp("steep_upper"));
        s.set(12, -3, 5, ramp("ramp"));
        // a ramp on a pedestal in the lake, its top at the lake's level, flooded as the sea's are
        // generated
        s.set(3, 0, 3, stone);
        s.set(3, 1, 3, stone);
        s.set(
            3,
            2,
            3,
            f.state(require(
                "dwell:stone_ramp[facing=east,flooded=true,half=bottom,shape=ramp]",
            )),
        );
        s.run(5);
        let flooded = |s: &Scene, x: i32, r: i32, z: i32| info(s.get(x, r, z)).flooded;
        assert!(!flooded(&s, 14, -3, 3), "dry before the water comes");
        let cut: Vec<(i32, i32, i32)> = (2..=5)
            .flat_map(|z| (0..=4).flat_map(move |r| [(8, r, z), (9, r, z)]))
            .collect();
        s.edit(&cut, M.air);
        s.run(600);
        let pit = s.level(15, 3).expect("the pit holds water");
        assert!(pit > -0.5, "the water stands over the slope: {pit:.3}");
        assert!(
            flooded(&s, 14, -3, 3),
            "the steep slope's lower ramp is flooded"
        );
        assert!(flooded(&s, 14, -2, 3), "and its upper");
        assert!(flooded(&s, 12, -3, 5), "the ramp on the floor is flooded");
        assert!(
            s.get(14, -3, 3)
                == f.state(require(
                    "dwell:stone_ramp[facing=east,flooded=true,half=bottom,shape=steep_lower]"
                )),
            "the same ramp, flooded"
        );
        // the lake's ramp, flooded as found, stands out of the lake now drained below it
        let lake = s.level(2, 3).expect("the lake");
        assert!(
            lake < 1.0,
            "the lake fell below its ramp's open part: {lake:.3}"
        );
        assert!(!flooded(&s, 3, 2, 3), "dried when the water left");
    }
);

impl Scene {
    /// A stone dam across the fixture river (z 0..=2) at column `x`, from the bed to row `top`.
    fn dam(&mut self, x: i32, top: i32) {
        let stone = require("dwell:stone");
        let cells: Vec<(i32, i32, i32)> = (0..=2)
            .flat_map(|z| (-6..=top).map(move |r| (x, r, z)))
            .filter(|&(x, r, z)| self.get(x, r, z) != stone)
            .collect();
        self.edit(&cells, stone);
    }
    /// The water standing on the dam at column `x` whose top is row `top`: its deepest (m) and the
    /// liquid cells drawn over it.
    fn on_dam(&self, x: i32, top: i32) -> (f64, usize) {
        let surface = m_of_row(top + 1);
        let deepest = (0..=2)
            .filter_map(|z| self.level(x, z))
            .map(|lv| lv - surface)
            .fold(0.0, f64::max);
        let cells = (0..=2)
            .flat_map(|z| (top + 1..=top + 4).map(move |r| (r, z)))
            .filter(|&(r, z)| info(self.get(x, r, z)).liquid)
            .count();
        (deepest, cells)
    }
    /// The flat river (2 rows deep) whose map flow begins below the dam site at x 24: none in the
    /// reach above (x ≤ 24), `Q` from x 25 on, as where a creek joins. The face between the two
    /// carries their mean, so the column at x 24 sends more than it gets: its rest-layer
    /// divergence is a spring of Q/2 (5 cm/s), as a river's columns have wherever its flow per
    /// metre changes (a narrowing, a junction, a drawn step).
    fn river_joined_below_the_dam(&mut self) {
        self.water = Water::new()
            .with_rivers(std::sync::Arc::new(|_, x, _, depth| {
                (x >= 25 && depth >= 0.05).then_some([(Q / depth).min(1.5), 0.0])
            }))
            .with_river_flow(std::sync::Arc::new(|_, x, _| (x >= 25).then_some([Q, 0.0])));
        self.flat_river(2);
        self.run(2);
    }
}

/// Water seconds the water standing on a dam has to drain off it (it does in seconds; the river
/// beside it goes on moving: BUGS.md #41).
const DRAIN_S: usize = 120;

dwell_world::both_faces!(
    fn water_on_a_raised_dam_drains_off_it(f: FaceFrame) {
        // Owner, 2026-11-06 (BUGS.md #40): "once water has entered the column (covered the top of
        // the dam with water) that water will never drain away even if you raise the height of
        // the dam ... standing water that continues to 'flow over' into the river bed beyond". A
        // dam column kept its rest-layer divergence (here the spring of a column whose flow
        // changes), which poured water onto the dam's top for ever, whatever its height (red:
        // water stood 4 cm on the raised dam, drawn over it, spilling both ways).
        let mut s = Scene::new(f);
        s.river_joined_below_the_dam();
        // A dam to the river's level: the water it displaced stands on it and spills.
        s.dam(24, 1);
        s.water.tick(&mut s.world, &[], 0.01);
        let (over, _) = s.on_dam(24, 1);
        assert!(over > 0.5, "the low dam was overtopped: {over:.3} m");
        s.run(60);
        // Raised 2 m: what stood on it drains off, and nothing more comes.
        s.dam(24, 5);
        s.run(DRAIN_S);
        let (deepest, cells) = s.on_dam(24, 5);
        assert!(
            deepest < 0.01 && cells == 0,
            "water stands on the raised dam: {deepest:.3} m, {cells} cells drawn over it"
        );
    }
);

dwell_world::both_faces!(
    fn a_world_saved_with_water_on_a_dam_drains_it_on_load(f: FaceFrame) {
        // Existing worlds heal on load (BUGS.md #40): a record saved with water standing on a dam
        // (DWG1, as before) is restored as it is, and the dam's column, its floor over the level
        // its water stood at as generated, no longer keeps that water's spring.
        let mut s = Scene::new(f);
        s.river_joined_below_the_dam();
        s.dam(24, 5);
        s.water.tick(&mut s.world, &[], 0.01);
        let (over, _) = s.on_dam(24, 5);
        assert!(
            over > 0.5,
            "saved with the water it lifted on it: {over:.3} m"
        );
        let bytes = s.water.save(|_| 0);
        let mut back = Water::new()
            .with_river_flow(std::sync::Arc::new(|_, x, _| (x >= 25).then_some([Q, 0.0])));
        assert!(back.restore(&bytes, |_| 0).is_some(), "the record reads");
        s.water = back;
        s.run(DRAIN_S);
        let (deepest, cells) = s.on_dam(24, 5);
        assert!(
            deepest < 0.01 && cells == 0,
            "water stands on the dam after loading: {deepest:.3} m, {cells} cells"
        );
    }
);

impl Scene {
    /// A generated river with a level reach and a step, running into a lake below it: a channel 3
    /// wide (z 0..=2) along x 0..=31, its bed row 0 to x 15 and row -1 from x 16 (a drawn step),
    /// its water 2 rows deep; `hollow`, a dip in the lower bed (x 20..=21, row -2) that holds its
    /// water. At x 31 it pours over a step into a lake (x 32..=39, z -20..=22, floor row -5, 1 m
    /// deep), its level 1 m under the lower bed. The map's flow `Q` runs along +x in the river,
    /// none in the lake.
    fn river_into_a_lake(&mut self, hollow: bool) {
        let stone = require("dwell:stone");
        for x in -1..=40 {
            for z in -21..=23 {
                for r in -6..=6 {
                    let m = if x <= 31 {
                        let floor = if x < 16 {
                            0
                        } else if hollow && (20..=21).contains(&x) {
                            -2
                        } else {
                            -1
                        };
                        let level = if x < 16 { 2 } else { 1 };
                        if x == -1 || !(0..=2).contains(&z) || r < floor {
                            stone
                        } else if r < level {
                            M.water
                        } else {
                            M.air
                        }
                    } else if x == 40 || !(-20..=22).contains(&z) || r < -5 {
                        stone
                    } else if r < -3 {
                        M.water
                    } else {
                        M.air
                    };
                    self.set(x, r, z, m);
                }
            }
        }
        self.water = Water::new()
            .with_rivers(std::sync::Arc::new(|_, x, _, depth| {
                (x <= 31 && depth >= 0.05).then_some([(Q / depth).min(1.5), 0.0])
            }))
            .with_river_flow(std::sync::Arc::new(|_, x, _| (x <= 31).then_some([Q, 0.0])));
        self.run(2);
    }
    /// The river's liquid cells from column `x0` down to the lake (x ≤ 31), skipping columns
    /// `skip`, and the deepest water among those columns (m).
    fn wet_below(&self, x0: i32, skip: &[i32]) -> (usize, f64) {
        let mut cells = 0;
        let mut deepest = 0.0f64;
        for x in (x0..=31).filter(|x| !skip.contains(x)) {
            for z in 0..=2 {
                for r in -3..=4 {
                    cells += usize::from(info(self.get(x, r, z)).liquid);
                }
                if let Some((c, l)) = self.water.find(self.f, x, z) {
                    let k = &self.water.chunks[c];
                    if !k.w.floor[l].is_nan() {
                        deepest = deepest.max(f64::from(k.w.depth[l]));
                    }
                }
            }
        }
        (cells, deepest)
    }
    /// The river's water chunks awake (x 0..=31, z 0..=2: two chunks; the lake's are others).
    fn river_awake(&self) -> usize {
        [0, 1]
            .into_iter()
            .filter(|&cx| {
                self.water
                    .index
                    .get(&(self.f, cx, 0))
                    .is_some_and(|&c| self.water.chunks[c as usize].awake)
            })
            .count()
    }
}

/// Water seconds a dammed river has to drain the reach below the dam (BUGS.md #41).
const DRY_S: usize = 900;

dwell_world::both_faces!(
    fn the_river_below_a_dam_dries_completely(f: FaceFrame) {
        // Owner, 2026-11-06 (BUGS.md #41): "Water never dries up below a dam ... Like
        // completely." With no other inflow, every water cell from the dam down to the lake goes,
        // and its chunks sleep; the lake takes what ran into it, no more and no less.
        let mut s = Scene::new(f);
        s.river_into_a_lake(false);
        assert!(s.wet_below(2, &[]).0 > 100, "the river as generated");
        s.dam(0, 6);
        s.dam(1, 6);
        s.run(DRY_S);
        let (cells, deepest) = s.wet_below(2, &[]);
        assert!(
            cells == 0 && deepest < f64::from(STEP) * 0.5,
            "below the dam: {cells} water cells, the deepest {deepest:.4} m"
        );
        assert_eq!(s.river_awake(), 0, "the dry river sleeps");
        let lake = s.level(36, 1).expect("the lake");
        assert!(
            lake > -1.5 && lake < -1.2,
            "the lake rose by the river it took: {lake:.3}"
        );
    }
);

dwell_world::both_faces!(
    fn a_hollow_below_a_dam_keeps_its_water_still(f: FaceFrame) {
        // A dip in the bed that can't drain keeps its water, still, and lets its chunk sleep; the
        // rest of the reach dries.
        let mut s = Scene::new(f);
        s.river_into_a_lake(true);
        s.dam(0, 6);
        s.dam(1, 6);
        s.run(DRY_S);
        // full to under its lip (row -1's floor, 0.5 m over the hollow's), and level
        let levels: Vec<f64> = (20..=21)
            .flat_map(|x| (0..=2).map(move |z| (x, z)))
            .map(|(x, z)| s.level(x, z).expect("the hollow's water"))
            .collect();
        let (lo, hi) = levels
            .iter()
            .fold((f64::MAX, f64::MIN), |(a, b), &v| (a.min(v), b.max(v)));
        assert!(
            lo > -0.6 && hi < -0.5 + f64::from(STEP) * 0.5 && hi - lo < 0.005,
            "the hollow holds its water, level: {levels:.3?}"
        );
        let (cells, deepest) = s.wet_below(2, &[20, 21]);
        assert!(
            cells == 0 && deepest < f64::from(STEP) * 0.5,
            "outside the hollow: {cells} water cells, the deepest {deepest:.4} m"
        );
        assert_eq!(s.river_awake(), 0, "the hollow's water sleeps");
        assert_eq!(s.run(60), 0, "and writes nothing");
    }
);

dwell_world::both_faces!(
    fn a_world_saved_below_a_dam_dries_on_load(f: FaceFrame) {
        // Existing worlds dry on load (BUGS.md #41): a record saved just after the dam (DWG1, as
        // before) is restored as it is, and the reach below leaves the rest from the dam down.
        let mut s = Scene::new(f);
        s.river_into_a_lake(false);
        s.dam(0, 6);
        s.dam(1, 6);
        s.run(2);
        let bytes = s.water.save(|_| 0);
        let mut back = Water::new()
            .with_river_flow(std::sync::Arc::new(|_, x, _| (x <= 31).then_some([Q, 0.0])));
        assert!(back.restore(&bytes, |_| 0).is_some(), "the record reads");
        s.water = back;
        s.run(DRY_S);
        let (cells, deepest) = s.wet_below(2, &[]);
        assert!(
            cells == 0 && deepest < f64::from(STEP) * 0.5,
            "below the dam after loading: {cells} water cells, the deepest {deepest:.4} m"
        );
    }
);

impl Scene {
    /// The water chunks with column x 0..48, z 0 awake (the fixture river's and basin's three).
    fn chunks_awake(&self) -> usize {
        (0..3)
            .filter(|&cx| {
                self.water
                    .index
                    .get(&(self.f, cx, 0))
                    .is_some_and(|&c| self.water.chunks[c as usize].awake)
            })
            .count()
    }
    /// Runs `secs` of water time; returns the seconds any of the three chunks was awake, and the
    /// cells written.
    fn awake_for(&mut self, secs: usize) -> (usize, usize) {
        let (mut awake, mut writes) = (0, 0);
        for _ in 0..secs {
            writes += self.run(1);
            awake += usize::from(self.chunks_awake() > 0);
        }
        (awake, writes)
    }
}

/// Water minutes a reservoir behind a dam has to fill to its spill (the fixture's, 1.1 m over
/// 72 m² at 0.3 m³/s: about 5) and to settle (BUGS.md #41).
const SPILL_S: usize = 1200;

fn reservoir_body(f: FaceFrame) {
    // BUGS.md #41's remaining part: the full reservoir behind a dam never slept. Its river
    // runs steadily over the crest and down to the loaded world's edge, and a sleeping chunk
    // was taken to carry no flow, so a chunk asleep beside an awake one owed the whole river
    // and woke at once, and the chain never slept together. Open: the reservoir's chunks are
    // awake for all of the last 5 minutes (BUGS.md #41 lists what was tried).
    let mut s = Scene::new(f);
    s.with_river_map();
    s.flat_river(2);
    s.run(2);
    s.dam(24, 3);
    s.run(SPILL_S);
    let up = s.level(12, 1).expect("the reservoir");
    assert!(up > 2.0, "filled to its spill (the crest at 2 m): {up:.3}");
    let (awake, writes) = s.awake_for(300);
    let now = s.level(12, 1).expect("the reservoir");
    assert!(
        awake <= 30 && writes == 0,
        "the spilling reservoir settles: awake {awake} of the last 300 s, {writes} cells written"
    );
    assert!(
        (now - up).abs() < 0.01,
        "its level steady: {up:.4} → {now:.4}"
    );
}

/// BUGS.md #41, still open (the reservoir's chunks awake for all of the last 5 minutes); run by
/// hand with `-- --ignored`.
#[test]
#[ignore = "BUGS.md #41, open: a reservoir spilling over its dam never falls asleep"]
fn a_reservoir_filled_to_its_spill_falls_asleep() {
    // both-faces: runs the body on each face itself
    for f in [FaceFrame::A, FaceFrame::B] {
        reservoir_body(f);
    }
}

fn stirred_body(f: FaceFrame) {
    // Deep still water's slosh, which Manning's friction barely damps, keeps a stirred basin
    // awake: a 40 m basin 3.5 m deep, a block column dropped in at one end, sloshes ±1 cm and
    // stays awake for all of 30 water minutes. Open (BUGS.md #41): a drag on slow water
    // (60 s under 5 cm/s) settled it in 8 minutes but slowed a cut lake's last levelling so it
    // slept off level (`tests/water.rs` `a_lake_cut_open_drains_on_after_its_player_leaves`).
    let mut s = Scene::new(f);
    s.fill([0, -5, 0], [39, -1, 7], M.water);
    s.basin(0, 39, 0, 7, 2);
    s.run(2);
    let col: Vec<(i32, i32, i32)> = (1..=3)
        .flat_map(|x| (2..=4).flat_map(move |z| (-5..=1).map(move |r| (x, r, z))))
        .collect();
    s.edit(&col, require("dwell:stone"));
    s.run(10);
    let v0 = s.water.free_volume();
    s.run(600);
    let (awake, writes) = s.awake_for(300);
    assert!(
        awake == 0 && writes == 0,
        "the stirred basin settles: awake {awake} of 300 s after ten minutes, {writes} cells"
    );
    let v1 = s.water.free_volume();
    assert!((v1 - v0).abs() < 0.01, "conserved: {v0:.4} → {v1:.4}");
}

/// BUGS.md #41, still open: run by hand with `-- --ignored`.
#[test]
#[ignore = "BUGS.md #41, open: stirred deep water sloshes on, awake"]
fn stirred_deep_water_settles() {
    // both-faces: runs the body on each face itself
    for f in [FaceFrame::A, FaceFrame::B] {
        stirred_body(f);
    }
}

/// The fixture river with a map current faster than its flow over its depth, as a real river's
/// (Manning's on its course's mean fall, BUGS.md #42): 1 m/s along +x, carrying `Q`.
fn fast_river_map(w: Water) -> Water {
    w.with_rivers(std::sync::Arc::new(|_, _, _, depth| {
        (depth >= 0.05).then_some([1.0, 0.0])
    }))
    .with_river_flow(std::sync::Arc::new(|_, _, _| Some([Q, 0.0])))
}

dwell_world::both_faces!(
    fn a_freed_river_column_runs_as_its_held_neighbours(f: FaceFrame) {
        // 2026-11-06: a held river column runs at the map's current (1.28 m/s at the owner's
        // creek), a freed one at the flow it carries over its depth (~0.1 m/s), so right by an
        // edit the river visibly slowed. A column freed by an edit beside the channel (not in
        // it), still flowing with the river, shows the map's current (red: 0.1 of 1.0).
        let mut s = Scene::new(f);
        s.water = fast_river_map(Water::new());
        s.flat_river(2);
        s.run(1);
        let [held, _] = s.current(30, 1);
        assert!((held - 1.0).abs() < 0.01, "held, the map's: {held:.3} m/s");
        s.edit(&[(10, 0, 3)], M.air);
        s.edit(&[(10, 0, 3)], require("dwell:stone"));
        s.run(5);
        assert!(!s.water.held_at(f, 10, 1), "freed");
        assert!(s.water.held_at(f, 30, 1), "still held downstream");
        let [u, v] = s.current(10, 1);
        assert!(
            (u - held).abs() < 0.2 * held && v.abs() < 0.2 * held,
            "freed, it runs as the river round it: {u:.3}, {v:.3} m/s (held {held:.3})"
        );
        // A world saved so runs so on load (the map's current found again for its columns).
        let bytes = s.water.save(|_| 0);
        let mut back = fast_river_map(Water::new());
        assert!(back.restore(&bytes, |_| 0).is_some(), "the record reads");
        s.water = back;
        s.run(5);
        let [u, _] = s.current(10, 1);
        assert!(
            (u - held).abs() < 0.2 * held,
            "restored, it runs so: {u:.3} m/s"
        );
    }
);

dwell_world::both_faces!(
    fn a_dammed_reachs_pool_keeps_its_own_still_current(f: FaceFrame) {
        // The pool backed up behind a dam is no longer the river: it keeps its own (near still)
        // current, not the map's.
        let mut s = Scene::new(f);
        s.water = fast_river_map(Water::new());
        s.flat_river(2);
        s.run(2);
        s.dam_flat_river();
        s.run(300);
        let rise = s.level(20, 1).expect("pool") - 1.0;
        assert!(rise > 0.25, "the pool rose: {rise:.3} m");
        let [u, v] = s.current(20, 1);
        assert!(
            u.hypot(v) < 0.1,
            "the pool is near still: {u:.3}, {v:.3} m/s"
        );
    }
);
