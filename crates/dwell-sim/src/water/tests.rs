//! The water grid's tests (Phase 53 G2, docs/WATER_SURFACE_GRID.md): scenes built face-locally and
//! run on both faces.

use super::*;
use dwell_blocks::require;
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
        // below falls.
        let mut s = Scene::new(f);
        s.stepped_river();
        s.run(2);
        let (up0, down0) = (
            s.level(12, 1).expect("above"),
            s.level(20, 1).expect("below"),
        );
        let stone = require("dwell:stone");
        let dam: Vec<(i32, i32, i32)> = (0..=2)
            .flat_map(|z| (-1..=6).map(move |r| (16, r, z)))
            .collect();
        s.edit(&dam, stone);
        s.run(900);
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
