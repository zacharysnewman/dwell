//! Water flow tests (docs/WATER_FLOW.md): scenes built face-locally and run on both faces.

use super::*;
use dwell_blocks::require;
use dwell_world::{FLAT_TOP_Y, empty_generator};

/// A scene in face `f`'s local frame: x, a row offset from `G`, z.
struct Scene {
    world: VoxelWorld,
    f: FaceFrame,
    water: Water,
}

const G: i32 = FLAT_TOP_Y + 40;

impl Scene {
    fn new(f: FaceFrame) -> Self {
        Self::with_air(f, [-16, -8, -8], [32, 8, 16])
    }
    /// A scene whose loaded air spans `lo`–`hi`.
    fn with_air(f: FaceFrame, lo: [i32; 3], hi: [i32; 3]) -> Self {
        let mut s = Self {
            world: VoxelWorld::new(empty_generator(), None),
            f,
            water: Water::new(),
        };
        s.fill(lo, hi, M.air);
        s
    }
    fn cell(&self, x: i32, r: i32, z: i32) -> [i32; 3] {
        self.f.cell([x, G + r, z])
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
    /// An edit: the cells set, `water` told what was there.
    fn edit(&mut self, cells: &[[i32; 3]], m: StateId) {
        let mut touched = Vec::new();
        for &[x, r, z] in cells {
            touched.push((self.cell(x, r, z), self.get(x, r, z)));
            self.set(x, r, z, m);
        }
        self.water.touch(touched);
    }
    fn player(&self) -> [f64; 3] {
        self.f.point([4.5, m_of_row(G) + 1.0, 4.5])
    }
    fn run(&mut self, ticks: u32, dt: f64) {
        let p = [self.player()];
        for _ in 0..ticks {
            self.water.tick(&mut self.world, &p, dt);
        }
    }
    fn wet(&self, x: i32, r: i32, z: i32) -> bool {
        self.get(x, r, z) == M.water
    }
    fn outlet_from(&self, x: i32, r: i32, z: i32) -> Option<Outlet> {
        let c = self.cell(x, r, z);
        self.water
            .basins()
            .flat_map(|b| b.outlets.iter())
            .find(|o| o.from == c)
            .copied()
    }
}

/// A channel along +x (x 0–9, z 0–2) of one row of water on a stone floor, walled to row 3,
/// falling at x = 10 into a walled catch basin below (floor row −6, walls to row −3).
fn channel(f: FaceFrame) -> Scene {
    let mut s = Scene::new(f);
    let stone = require("dwell:stone");
    s.fill([-1, -1, -1], [9, -1, 3], stone);
    s.fill([-1, -1, -1], [9, 3, -1], stone);
    s.fill([-1, -1, 3], [9, 3, 3], stone);
    s.fill([-1, -1, -1], [-1, 3, 3], stone);
    s.fill([9, -6, -3], [21, -6, 5], stone);
    s.fill([9, -6, -3], [9, -3, 5], stone);
    s.fill([21, -6, -3], [21, -3, 5], stone);
    s.fill([9, -6, -3], [21, -3, -3], stone);
    s.fill([9, -6, 5], [21, -3, 5], stone);
    s.fill([0, 0, 0], [9, 0, 2], M.water);
    s
}

dwell_world::both_faces!(
    /// A dam across a flowing channel: the pool behind it fills at the channel's old flow (its
    /// inflow, W1's stand-in for runoff, what left it before), rises a row and spills over the dam
    /// at about that flow, and the water is conserved (docs/WATER_FLOW.md §5).
    fn a_dammed_channel_fills_and_spills_over_the_dam(f: FaceFrame) {
        let mut s = channel(f);
        s.edit(&[[9, 0, 0], [9, 0, 1], [9, 0, 2]], require("dwell:stone"));
        s.run(1, 0.0);
        let b = s
            .water
            .basin_at(s.cell(4, 0, 1))
            .expect("the pool is a basin")
            .clone();
        // Its flow over the fall before the dam: three cells over a sill half a row down.
        let want = 1.7 * 3.0 * 0.5f64.powf(1.5);
        assert!(
            (b.inflow - want).abs() < 1e-6,
            "inflow {} (want {want})",
            b.inflow
        );
        let stored0 = s.water.stored();
        s.run(600, 1.0);
        for x in 0..=8 {
            assert!(s.wet(x, 1, 1), "the pool rose into row 1 at x {x}");
        }
        let pool = s.water.basin_at(s.cell(4, 0, 1)).expect("the pool");
        let over: f64 = pool.outlets.iter().map(|o| o.flow).sum();
        assert!(
            (over - want).abs() < 0.05 * want,
            "spills {over} m³/s of {want}"
        );
        // The water that went over landed below and pooled.
        assert!(
            s.water
                .basins()
                .any(|b| !b.reservoir && b.id != pool.id && b.volume > 1.0),
            "a pool below the dam"
        );
        let balance = s.water.stored() + s.water.lost - s.water.brought - stored0;
        assert!(
            balance.abs() < 1e-6 * s.water.brought.max(1.0),
            "unbalanced by {balance} m³"
        );
    }
);

dwell_world::both_faces!(
    /// A closed lake four rows deep: a cut through its wall down to row 2 drains it to the cut's
    /// floor; rows 2 and 3 dry, rows 0 and 1 stay (docs/WATER_FLOW.md §5).
    fn a_cut_drains_a_lake_to_the_cuts_floor(f: FaceFrame) {
        let mut s = Scene::new(f);
        let stone = require("dwell:stone");
        s.fill([-1, -1, -1], [10, -1, 10], stone);
        s.fill([-1, -1, -1], [10, 3, -1], stone);
        s.fill([-1, -1, 10], [10, 3, 10], stone);
        s.fill([-1, -1, -1], [-1, 3, 10], stone);
        s.fill([10, -1, -1], [10, 3, 10], stone);
        s.fill([11, -6, -2], [24, -6, 12], stone);
        s.fill([0, 0, 0], [9, 3, 9], M.water);
        s.edit(&[[10, 2, 4], [10, 3, 4]], M.air);
        s.run(1, 0.0);
        let lake = s.water.basin_at(s.cell(4, 0, 4)).expect("the lake").clone();
        assert_eq!(lake.inflow, 0.0, "a closed lake has no inflow");
        s.run(400, 1.0);
        for x in 0..=9 {
            assert!(
                !s.wet(x, 3, 4) && !s.wet(x, 2, 4),
                "dry above the cut's floor at x {x}"
            );
            assert!(
                s.wet(x, 1, 4) && s.wet(x, 0, 4),
                "still water below it at x {x}"
            );
        }
        let lake = s.water.basin_at(s.cell(4, 0, 4)).expect("the lake");
        let floor = m_of_row(G + 2);
        assert!(
            lake.level() >= floor - 1e-9 && lake.level() < floor + CELL_H / 2.0,
            "drained to {} (the cut's floor {floor})",
            lake.level()
        );
    }
);

dwell_world::both_faces!(
    /// A pool flowing out of a notch in its end wall: a trench dug at its other end down to its
    /// floor takes the flow, the level falls below the notch, and the old way runs dry
    /// (docs/WATER_FLOW.md §5).
    fn a_deeper_trench_takes_the_flow_from_the_old_way(f: FaceFrame) {
        let mut s = Scene::new(f);
        let stone = require("dwell:stone");
        s.fill([-1, -1, -1], [10, -1, 3], stone);
        s.fill([-1, -1, -1], [10, 3, -1], stone);
        s.fill([-1, -1, 3], [10, 3, 3], stone);
        s.fill([-1, -1, -1], [-1, 3, 3], stone);
        s.fill([10, -1, -1], [10, 3, 3], stone);
        s.set(10, 1, 1, M.air); // the notch, its sill row 1
        s.fill([0, 0, 0], [9, 1, 2], M.water);
        s.set(10, 1, 1, M.water); // water in the notch, spilling
        s.fill([11, -6, -2], [24, -6, 4], stone);
        // Found by an edit beside it (the notch as it was), then the trench.
        s.edit(&[[-1, 0, 0], [-1, 0, 1], [-1, 1, 0], [-1, 1, 1]], M.air);
        s.run(1, 0.0);
        let pool = s.water.basin_at(s.cell(4, 0, 1)).expect("the pool").clone();
        let want = 1.7 * 0.5f64.powf(1.5);
        assert!(
            (pool.inflow - want).abs() < 1e-6,
            "inflow {} (want {want})",
            pool.inflow
        );
        s.run(600, 1.0);
        let old = s.outlet_from(11, 1, 1).map_or(0.0, |o| o.flow);
        let new: f64 = [0, 1]
            .iter()
            .filter_map(|&z| s.outlet_from(-2, 0, z))
            .map(|o| o.flow)
            .sum();
        assert!(old < 0.01 * want, "the notch still passes {old} m³/s");
        assert!(
            (new - want).abs() < 0.05 * want,
            "the trench passes {new} of {want} m³/s"
        );
    }
);

dwell_world::both_faces!(
    /// The sea (a body larger than `maxBasinCells`) is a fixed reservoir: a cut through the wall
    /// between it and a dry hollow below its level floods the hollow up to the sea's level, and the
    /// sea's own level doesn't move (docs/WATER_FLOW.md §4.2).
    fn a_cut_from_the_sea_floods_a_hollow_to_sea_level(f: FaceFrame) {
        let mut s = Scene::with_air(f, [-2, -16, -2], [80, 4, 72]);
        let stone = require("dwell:stone");
        // A sea of 70 × 70 × 14 cells (68,600 > maxBasinCells), walled, and the hollow beside it.
        s.fill([-1, -15, -1], [77, -15, 71], stone);
        s.fill([-1, -15, -1], [-1, 2, 71], stone);
        s.fill([-1, -15, -1], [70, 2, -1], stone);
        s.fill([-1, -15, 70], [70, 2, 70], stone);
        s.fill([70, -15, -1], [70, 2, 71], stone);
        s.fill([70, -15, 29], [77, 2, 29], stone);
        s.fill([70, -15, 35], [77, 2, 35], stone);
        s.fill([77, -15, 29], [77, 2, 35], stone);
        s.fill([0, -14, 0], [69, -1, 69], M.water);
        assert!(70 * 70 * 14 > FLOW.max_basin_cells as i32);
        let cut: Vec<[i32; 3]> = (-14..=-1).map(|r| [70, r, 32]).collect();
        s.edit(&cut, M.air);
        s.run(1, 0.0);
        let sea = s
            .water
            .basin_at(s.cell(69, -1, 32))
            .expect("the sea")
            .clone();
        assert!(sea.reservoir, "the sea is a reservoir");
        let p = [s.f.point([73.5, m_of_row(G), 32.5])];
        for _ in 0..400 {
            s.water.tick(&mut s.world, &p, 1.0);
        }
        for x in 71..=76 {
            for r in -14..=-1 {
                assert!(s.wet(x, r, 32), "the hollow is flooded at x {x}, row {r}");
            }
            assert!(!s.wet(x, 0, 32), "no higher than the sea at x {x}");
        }
        let sea_now = s.water.basin_at(s.cell(69, -1, 32)).expect("the sea");
        assert_eq!(sea_now.level(), sea.level(), "the sea's level holds");
        assert!(s.wet(10, -1, 10), "the sea is still there");
    }
);

#[test]
fn the_flow_data_resolves() {
    for l in FLOW.liquids {
        let m = require(l.block);
        assert_eq!(weir_of(m), Some(l.weir));
    }
    for &(b, v) in FLOW.leaks {
        assert_eq!(leak_of(require(b)), v);
    }
    assert_eq!(leak_of(require("dwell:stone")), 0.0);
}
