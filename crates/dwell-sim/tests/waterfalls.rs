//! Water over a drop (docs/WATER_SURFACE_GRID.md §2e, Phase 53 G6): a dam's notch below the
//! reservoir's level spills, and the water reaches the ground below the wall; both faces.

use dwell_blocks::{StateId, require};
use dwell_sim::water::Water;
use dwell_world::{FLAT_TOP_Y, FaceFrame, M, VoxelWorld, empty_generator, m_of_row};

const BASE: i32 = FLAT_TOP_Y + 40;

/// A scene in face `f`'s local frame: x, a row offset from `BASE`, z.
struct Scene {
    world: VoxelWorld,
    f: FaceFrame,
    water: Water,
}

impl Scene {
    fn new(f: FaceFrame) -> Self {
        let mut s = Self {
            world: VoxelWorld::new(empty_generator(), None),
            f,
            water: Water::new(),
        };
        let stone = require("dwell:stone");
        s.fill([-8, -6, -8], [40, -1, 24], stone);
        s.fill([-8, 0, -8], [40, 16, 24], M.air);
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
    fn edit(&mut self, cells: &[(i32, i32, i32)], m: StateId) {
        let mut old = Vec::new();
        for &(x, r, z) in cells {
            old.push((self.cell(x, r, z), self.get(x, r, z)));
            self.set(x, r, z, m);
        }
        self.water.touch(old);
    }
    fn run(&mut self, secs: usize) {
        for _ in 0..secs {
            self.water.tick(&mut self.world, &[], 1.0);
        }
    }
    /// The level at column (x, z), m above `BASE`'s row (face-local).
    fn level(&self, x: i32, z: i32) -> Option<f64> {
        self.water
            .level_at(self.f, x, z)
            .map(|l| l - m_of_row(BASE))
    }
    /// The water cells drawn in column (x, z) between rows `r0..=r1`.
    fn wet_rows(&self, x: i32, z: i32, r0: i32, r1: i32) -> usize {
        (r0..=r1)
            .filter(|&r| dwell_blocks::info(self.get(x, r, z)).liquid)
            .count()
    }

    /// A reservoir (x 0..=7, z 0..=7) on a stone plateau 4 rows high, its water rows 4..=8 (level
    /// 4.5 m), walled by stone to row 10; its x = 8 wall is the dam. Below the dam (x 9..=20) a
    /// walled floor at row 0. Generated water: held at its level (a river that feeds it).
    fn dam(&mut self) {
        let stone = require("dwell:stone");
        self.fill([-1, 0, -1], [8, 3, 8], stone);
        for r in 4..=10 {
            for x in -1..=8 {
                self.set(x, r, -1, stone);
                self.set(x, r, 8, stone);
            }
            for z in -1..=8 {
                self.set(-1, r, z, stone);
                self.set(8, r, z, stone);
            }
        }
        self.fill([0, 4, 0], [7, 8, 7], M.water);
        for r in 0..=3 {
            for x in 9..=21 {
                self.set(x, r, -1, stone);
                self.set(x, r, 8, stone);
            }
            for z in -1..=8 {
                self.set(21, r, z, stone);
            }
        }
    }
}

dwell_world::both_faces!(
    fn a_notch_cut_in_a_dam_below_its_level_spills_down_the_wall(f: FaceFrame) {
        let mut s = Scene::new(f);
        s.dam();
        s.run(2);
        // the notch: x = 8, z 3..=4, rows 6..=10 (its floor at 3 m, the water at 4.5 m)
        let notch: Vec<(i32, i32, i32)> = (3..=4)
            .flat_map(|z| (6..=10).map(move |r| (8, r, z)))
            .collect();
        s.edit(&notch, M.air);
        s.run(60);
        let lip = s.level(8, 3).expect("water stands in the notch");
        assert!(lip > 3.0, "over the notch's floor: {lip:.3}");
        let foot = s.level(9, 3).expect("water at the wall's foot");
        assert!(foot > 0.05, "the foot is wet: {foot:.3}");
        assert!(
            s.wet_rows(9, 3, 0, 1) > 0,
            "and drawn there as water cells: {:?}",
            (0..4).map(|r| s.get(9, r, 3)).collect::<Vec<_>>()
        );
        let far = s.level(15, 4).expect("it spreads over the floor below");
        assert!(far > 0.05, "the floor below fills: {far:.3}");
    }
);

impl Scene {
    /// A drawn river as the generator draws them (`water::tests`' `stepped_river`): a channel 3
    /// wide (z 0..=2) along x 0..40, its floor a row lower every 8 columns, 2 rows deep.
    fn stepped_river(&mut self) {
        let stone = require("dwell:stone");
        for x in -1..=40 {
            let drop = x.max(0) / 8;
            for r in -6..=8 {
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
}

dwell_world::both_faces!(
    fn a_notch_in_a_dam_across_a_drawn_river_spills_below(f: FaceFrame) {
        let mut s = Scene::new(f);
        s.stepped_river();
        s.run(2);
        let stone = require("dwell:stone");
        let dam: Vec<(i32, i32, i32)> = (0..=2)
            .flat_map(|z| (-1..=8).map(move |r| (16, r, z)))
            .collect();
        s.edit(&dam, stone);
        s.run(900);
        let up = s.level(12, 1).expect("above");
        let down = s.level(20, 1).expect("below");
        // the notch: its floor a row below the backwater's level
        let r = dwell_world::row_of_m(up + m_of_row(BASE)) - BASE - 1;
        let notch: Vec<(i32, i32, i32)> = (r..=8).map(|r| (16, r, 1)).collect();
        s.edit(&notch, M.air);
        s.run(120);
        let foot = s.level(17, 1).expect("the foot");
        assert!(foot > down + 0.05, "the foot rose: {down:.3} → {foot:.3}");
    }
);
