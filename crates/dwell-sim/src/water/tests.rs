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

/// The scenes' world map flow (m³/s): a three-cell channel's over a fall half a row deep.
fn channel_q() -> f64 {
    1.7 * 3.0 * 0.5f64.powf(1.5)
}

/// Water fed `q` m³/s through every column (a scene's stand-in for the world map).
fn fed(q: f64) -> Water {
    Water::with_runoff(std::sync::Arc::new(move |_, _, _| q))
}

impl Scene {
    fn new(f: FaceFrame) -> Self {
        Self::with_air(f, [-16, -8, -8], [32, 8, 16])
    }
    /// A scene whose loaded air spans `lo`–`hi`.
    fn with_air(f: FaceFrame, lo: [i32; 3], hi: [i32; 3]) -> Self {
        let mut s = Self {
            world: VoxelWorld::new(empty_generator(), None),
            f,
            water: fed(channel_q()),
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
        self.run_at(ticks, dt, self.player());
    }
    /// [`Scene::run`] with the player at `p`.
    fn run_at(&mut self, ticks: u32, dt: f64, p: [f64; 3]) {
        let p = [p];
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
            (b.inflow() - want).abs() < 1e-6,
            "inflow {} (want {want})",
            b.inflow()
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
        s.water = fed(0.0); // on no river of the map
        s.edit(&[[10, 2, 4], [10, 3, 4]], M.air);
        s.run(1, 0.0);
        let lake = s.water.basin_at(s.cell(4, 0, 4)).expect("the lake").clone();
        assert_eq!(lake.inflow(), 0.0, "a closed lake has no inflow");
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
        s.water = fed(1.7 * 0.5f64.powf(1.5));
        // Found by an edit beside it (the notch as it was), then the trench.
        s.edit(&[[-1, 0, 0], [-1, 0, 1], [-1, 1, 0], [-1, 1, 1]], M.air);
        s.run(1, 0.0);
        let pool = s.water.basin_at(s.cell(4, 0, 1)).expect("the pool").clone();
        let want = 1.7 * 0.5f64.powf(1.5);
        assert!(
            (pool.inflow() - want).abs() < 1e-6,
            "inflow {} (want {want})",
            pool.inflow()
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

/// A river of two steps along +x (z 0–2, walled to row 5): the upper pool (x −10 to −1, floor row
/// 0, water row 1) falls at x = 0 into the lower pool (x 0–9, floor row −1, water row 0), which falls
/// at x = 10 into a walled catch basin below.
fn two_steps(f: FaceFrame) -> Scene {
    let mut s = Scene::with_air(f, [-16, -8, -8], [32, 8, 16]);
    let stone = require("dwell:stone");
    s.fill([-11, -1, -1], [9, -1, 3], stone);
    s.fill([-11, 0, -1], [-1, 0, 3], stone);
    s.fill([-11, -1, -1], [9, 5, -1], stone);
    s.fill([-11, -1, 3], [9, 5, 3], stone);
    s.fill([-11, -1, -1], [-11, 5, 3], stone);
    s.fill([9, -6, -3], [21, -6, 5], stone);
    s.fill([9, -6, -3], [9, -3, 5], stone);
    s.fill([21, -6, -3], [21, -3, 5], stone);
    s.fill([9, -6, -3], [21, -3, -3], stone);
    s.fill([9, -6, 5], [21, -3, 5], stone);
    s.fill([0, 0, 0], [9, 0, 2], M.water);
    s.fill([-10, 1, 0], [-1, 1, 2], M.water);
    s
}

dwell_world::both_faces!(
    /// Owner, 2026-11-03: half blocks placed in a river's bed below a stepped waterfall flooded it
    /// to the top of the fall. Found together, the lower pool's inflow (what left it) already held
    /// what the upper pool sends it, which then arrived again: counted once, nothing changes. Red
    /// before: the lower pool rose into row 1.
    fn a_pool_and_the_pool_above_it_found_together_count_the_falls_water_once(f: FaceFrame) {
        let mut s = two_steps(f);
        // Edits that change nothing, beside each pool.
        s.edit(&[[-5, 2, 1], [5, 1, 1]], M.air);
        s.run(600, 1.0);
        for x in [0, 5, 8] {
            assert!(!s.wet(x, 1, 1), "the lower pool rose at x {x}");
            assert!(s.wet(x, 0, 1), "the lower pool is there at x {x}");
        }
        for x in [-10, -5, -1] {
            assert!(s.wet(x, 1, 1), "the upper pool is there at x {x}");
        }
    }
);

dwell_world::both_faces!(
    /// Owner, 2026-11-03: a flood that joined a river's steps drained the upper ones dry when it
    /// went. A dam floods the lower pool up over the upper one (one basin); taken away, the flood
    /// recedes and the upper pool, fed where its water comes in, stays. Red before: dry.
    fn a_flood_over_a_rivers_steps_leaves_each_step_its_water_when_it_recedes(f: FaceFrame) {
        let mut s = two_steps(f);
        let stone = require("dwell:stone");
        // Found first (as the river was), then dammed to row 2.
        s.edit(&[[-5, 2, 1], [5, 1, 1]], M.air);
        s.run(5, 1.0);
        let dam: Vec<[i32; 3]> = (0..=2)
            .flat_map(|z| (0..=2).map(move |r| [9, r, z]))
            .collect();
        s.edit(&dam, stone);
        s.run(900, 1.0);
        assert!(s.wet(-5, 2, 1), "the flood covers the upper step");
        s.edit(&dam, M.air);
        s.run(900, 1.0);
        for x in [-10, -5, -1] {
            assert!(s.wet(x, 1, 1), "the upper pool is back at x {x}");
            assert!(!s.wet(x, 2, 1), "and no higher at x {x}");
        }
        for x in [0, 5] {
            assert!(
                s.wet(x, 0, 1) && !s.wet(x, 1, 1),
                "the lower pool as it was at x {x}"
            );
        }
    }
);

dwell_world::both_faces!(
    /// The owner's case (2026-11-03): three cells set in a river's bed just below a stepped fall
    /// (three pools, each falling two rows to the next) change nothing: no pool rises or drains.
    /// Red before: the bottom pool rose, merged up the falls, and the river drained.
    fn blocks_set_in_a_riverbed_under_a_stepped_fall_change_nothing(f: FaceFrame) {
        let mut s = Scene::with_air(f, [-32, -8, -8], [32, 12, 16]);
        let stone = require("dwell:stone");
        // Walls along z to row 8; the pools' floors: A rows ≤ 3 (x −30 to −21), B ≤ 1 (x −20 to
        // −11), C ≤ −1 (x −10 to 9); each one row of water; C falls at x = 10 into a catch basin.
        s.fill([-31, -1, -1], [9, 8, -1], stone);
        s.fill([-31, -1, 3], [9, 8, 3], stone);
        s.fill([-31, -1, -1], [-31, 8, 3], stone);
        s.fill([-30, -1, 0], [-21, 3, 2], stone);
        s.fill([-20, -1, 0], [-11, 1, 2], stone);
        s.fill([-10, -1, 0], [9, -1, 2], stone);
        s.fill([-30, 4, 0], [-21, 4, 2], M.water);
        s.fill([-20, 2, 0], [-11, 2, 2], M.water);
        s.fill([-10, 0, 0], [9, 0, 2], M.water);
        s.fill([9, -6, -3], [21, -6, 5], stone);
        s.fill([9, -6, -3], [9, -3, 5], stone);
        s.fill([21, -6, -3], [21, -3, 5], stone);
        s.fill([9, -6, -3], [21, -3, -3], stone);
        s.fill([9, -6, 5], [21, -3, 5], stone);
        // Found by an edit at each pool (as a player's edits would), then the half blocks.
        s.edit(&[[-25, 5, 1], [-15, 3, 1], [5, 1, 1]], M.air);
        s.run(5, 1.0);
        s.edit(&[[-10, 0, 0], [-10, 0, 1], [-10, 0, 2]], stone);
        s.run(900, 1.0);
        let row = |s: &Scene, x0: i32, x1: i32, r: i32| (x0..=x1).all(|x| s.wet(x, r, 1));
        let dry = |s: &Scene, x0: i32, x1: i32, r: i32| (x0..=x1).all(|x| !s.wet(x, r, 1));
        assert!(
            row(&s, -30, -21, 4) && dry(&s, -30, -21, 5),
            "pool A as it was"
        );
        assert!(
            row(&s, -20, -11, 2) && dry(&s, -20, -11, 3),
            "pool B as it was"
        );
        assert!(row(&s, -9, 9, 0) && dry(&s, -10, 9, 1), "pool C as it was");
    }
);

dwell_world::both_faces!(
    /// Owner, 2026-11-03: a block set at the bottom of any river flooded it without end, or
    /// drained it. A body as generated is at rest (docs/WATER_FLOW.md §2.2): an open cell beside it
    /// that it didn't fill (here a gap in the bank over a plant) holds it in, and its natural
    /// outlets (the fall) pass the map's flow. Red before: the gap was an outlet, its flow counted
    /// in the river's inflow, and water ran out onto the bank and pooled there.
    fn a_river_found_at_rest_keeps_its_banks_dry(f: FaceFrame) {
        let mut s = channel(f);
        let stone = require("dwell:stone");
        // A gap in the near wall at the water's row, over a plant on a ledge, and a walled ledge
        // beyond it where water would pool.
        s.set(4, 0, 3, M.air);
        s.set(4, -1, 3, require("dwell:tall_grass"));
        s.set(4, -2, 3, stone);
        s.fill([2, -2, 4], [6, -2, 7], stone);
        s.fill([1, -2, 3], [1, 1, 8], stone);
        s.fill([7, -2, 3], [7, 1, 8], stone);
        s.fill([1, -2, 8], [7, 1, 8], stone);
        s.fill([2, -1, 3], [3, 1, 3], stone);
        s.fill([5, -1, 3], [6, 1, 3], stone);
        // One block in the bed.
        s.edit(&[[2, 0, 1]], stone);
        s.run(600, 1.0);
        for x in 2..=6 {
            for z in 4..=7 {
                assert!(!s.wet(x, -1, z), "water on the ledge at x {x}, z {z}");
            }
        }
        for x in [0, 4, 8] {
            assert!(
                s.wet(x, 0, 1) && !s.wet(x, 1, 1),
                "the river as it was at x {x}"
            );
        }
        let pool = s.water.basin_at(s.cell(4, 0, 1)).expect("the river");
        let over: f64 = pool.outlets.iter().map(|o| o.flow).sum();
        assert!(
            (over - channel_q()).abs() < 0.02 * channel_q(),
            "passes {over} m³/s of the map's {}",
            channel_q()
        );
    }
);

dwell_world::both_faces!(
    /// A basin part of whose container is in a chunk that isn't loaded waits (docs/WATER_FLOW.md
    /// §15): an unloaded cell is a wall to water, so flowing on would lose what stands there. Red
    /// before: the lake drained on in what it could see, and the rows in the missing chunk, back,
    /// stayed wet over a lake drained below them.
    fn a_basin_waits_while_a_chunk_of_it_is_unloaded(f: FaceFrame) {
        let mut s = Scene::new(f);
        let stone = require("dwell:stone");
        // A closed lake four rows deep across the chunk edge at x = 0 (x −6 to 5).
        s.fill([-7, -1, -1], [6, -1, 6], stone);
        s.fill([-7, -1, -1], [6, 3, -1], stone);
        s.fill([-7, -1, 6], [6, 3, 6], stone);
        s.fill([-7, -1, -1], [-7, 3, 6], stone);
        s.fill([6, -1, -1], [6, 3, 6], stone);
        s.fill([7, -6, -2], [20, -6, 8], stone);
        s.fill([-6, 0, 0], [5, 3, 5], M.water);
        s.water = fed(0.0); // on no river of the map
        s.edit(&[[6, 2, 2], [6, 3, 2]], M.air);
        s.run(3, 1.0);
        let west = chunk_of(
            s.cell(-3, 0, 2)[0],
            s.cell(-3, 0, 2)[1],
            s.cell(-3, 0, 2)[2],
        );
        assert_ne!(
            west,
            chunk_of(s.cell(3, 0, 2)[0], s.cell(3, 0, 2)[1], s.cell(3, 0, 2)[2])
        );
        let saved = s.world.find(west).expect("loaded").clone();
        s.world.remove(west);
        let (stored, lost) = (s.water.stored(), s.water.lost);
        s.run(200, 1.0);
        assert!(
            (s.water.stored() - stored).abs() < 1e-9 && s.water.lost == lost,
            "waits: stored {stored} → {}, lost {lost} → {}",
            s.water.stored(),
            s.water.lost
        );
        s.world.put(west, saved);
        s.run(600, 1.0);
        for x in [-6, -3, 0, 3, 5] {
            assert!(
                !s.wet(x, 3, 2) && !s.wet(x, 2, 2),
                "dry above the cut's floor at x {x}"
            );
            assert!(s.wet(x, 1, 2), "still water below it at x {x}");
        }
    }
);

dwell_world::both_faces!(
    /// Owner, 2026-11-03: a ramp beside the water stood dry, the water ending in a wall at it. A
    /// block with a `flooded` state at the edge of a row the water writes takes it, and loses it
    /// when the row dries. Red before: never flooded.
    fn ramps_at_the_waters_edge_flood_and_dry_with_it(f: FaceFrame) {
        let mut s = channel(f);
        let stone = require("dwell:stone");
        let ramp = f.state(
            dwell_blocks::with_property(require("dwell:stone_ramp"), "flooded", "false")
                .expect("a ramp with a flooded state"),
        );
        s.set(4, 1, 3, ramp);
        let dam: Vec<[i32; 3]> = (0..=2).map(|z| [9, 0, z]).collect();
        s.edit(&dam, stone);
        s.run(600, 1.0);
        assert!(s.wet(4, 1, 2), "the pool rose into row 1");
        assert!(
            dwell_blocks::info(s.get(4, 1, 3)).flooded,
            "the ramp beside it is flooded"
        );
        s.edit(&dam, M.air);
        s.run(900, 1.0);
        assert!(!s.wet(4, 1, 2), "row 1 dried");
        assert!(
            !dwell_blocks::info(s.get(4, 1, 3)).flooded,
            "the ramp dried with it"
        );
    }
);

dwell_world::both_faces!(
    /// A basin out of every player's reach waits (docs/WATER_FLOW.md §15): it holds its level and
    /// passes on what comes in. Red before: the lower pool ran dry.
    fn a_waiting_basin_passes_on_what_comes_in(f: FaceFrame) {
        // A long upper pool (x −200 to 9) falling at x = 10 into a pool (x 10–19) falling at x = 20
        // into a catch basin; the player 180 m out past the upper pool's far end.
        let mut s = Scene::with_air(f, [-210, -8, -4], [40, 6, 8]);
        let stone = require("dwell:stone");
        s.fill([-201, -1, -1], [9, 0, 3], stone);
        s.fill([-201, -1, -1], [19, 4, -1], stone);
        s.fill([-201, -1, 3], [19, 4, 3], stone);
        s.fill([-201, -1, -1], [-201, 4, 3], stone);
        s.fill([10, -1, -1], [19, -1, 3], stone);
        s.fill([-200, 1, 0], [9, 1, 2], M.water);
        s.fill([10, 0, 0], [19, 0, 2], M.water);
        s.fill([19, -6, -3], [32, -6, 5], stone);
        s.fill([19, -6, -3], [19, -3, 5], stone);
        s.fill([32, -6, -3], [32, -3, 5], stone);
        s.fill([19, -6, -3], [32, -3, -3], stone);
        s.fill([19, -6, 5], [32, -3, 5], stone);
        s.edit(&[[-100, 2, 1], [15, 1, 1]], M.air);
        let near = [s.f.point([-350.0, m_of_row(G) + 1.0, 1.5])];
        for _ in 0..600 {
            s.water.tick(&mut s.world, &near, 1.0);
        }
        for x in [10, 15, 19] {
            assert!(
                s.wet(x, 0, 1) && !s.wet(x, 1, 1),
                "the lower pool as it was at x {x}"
            );
        }
        let lower = s.water.basin_at(s.cell(15, 0, 1)).expect("the lower pool");
        assert!(
            !lower.near(&near, FLOW.live_radius_m),
            "the lower pool is out of reach"
        );
        let upper = s.water.basin_at(s.cell(0, 1, 1)).expect("the upper pool");
        assert!(
            upper.near(&near, FLOW.live_radius_m),
            "the upper pool is live"
        );
        assert!(
            (upper.level() - m_of_row(G + 2)).abs() < 0.05,
            "the upper pool's level {}",
            upper.level()
        );
    }
);

dwell_world::both_faces!(
    /// Owner, 2026-11-03: a wall across a river filled the water up to the wall but not over it
    /// and down the other side, and a wall level with the water didn't raise it at all. The river
    /// above rises until it passes its flow over the wall, the spill is drawn over the wall's top
    /// and running down its far side, and the river below keeps its level. Red on 0.43.0: the
    /// river above a wall level with its water didn't rise (one pool over the wall, one level).
    fn a_wall_across_a_river_is_spilled_over_and_the_river_below_holds(f: FaceFrame) {
        let stone = require("dwell:stone");
        let running = require("dwell:running_water");
        for top in [0, 1] {
            let mut s = channel(f);
            // Found as it was (an edit beside it), then the wall at x = 5 up to row `top`.
            s.edit(&[[4, 1, 1]], M.air);
            s.run(5, 1.0);
            let wall: Vec<[i32; 3]> = (0..=2)
                .flat_map(|z| (0..=top).map(move |r| [5, r, z]))
                .collect();
            s.edit(&wall, stone);
            s.run(900, 1.0);
            let over = top + 1;
            for x in [0, 2, 4] {
                assert!(s.wet(x, over, 1), "top {top}: risen to row {over} at x {x}");
            }
            assert!(s.wet(5, over, 1), "top {top}: drawn over the wall's top");
            for r in 1..=over {
                assert_eq!(
                    s.get(6, r, 1),
                    running,
                    "top {top}: running down the far side at row {r}"
                );
            }
            for x in [7, 8, 9] {
                assert!(
                    s.wet(x, 0, 1) && !s.wet(x, 1, 1),
                    "top {top}: the river below as it was at x {x}"
                );
            }
        }
    }
);

/// [`channel`] with its walls to row 8 (room to rise behind a wall).
fn tall_channel(f: FaceFrame) -> Scene {
    let mut s = channel(f);
    let stone = require("dwell:stone");
    s.fill([-1, 4, -1], [9, 8, -1], stone);
    s.fill([-1, 4, 3], [9, 8, 3], stone);
    s.fill([-1, 4, -1], [-1, 8, 3], stone);
    s
}

dwell_world::both_faces!(
    /// Owner, 2026-11-03: a hole below the water in a wall across a river passes what its size and
    /// depth drive: `C × area × √(2 g Δh)` (C from the weir coefficient, docs/WATER_FLOW.md §4),
    /// so the river behind a tall wall with one hole at its foot rises until the hole carries its
    /// flow (here ~2 m), and the river below keeps its level. Red before: the hole joined the two
    /// sides into one pool, and the river didn't rise.
    fn a_hole_in_a_wall_passes_what_its_size_and_depth_drive(f: FaceFrame) {
        let mut s = tall_channel(f);
        let stone = require("dwell:stone");
        s.edit(&[[4, 1, 1]], M.air);
        s.run(5, 1.0);
        let hole = [5, 0, 1];
        let wall: Vec<[i32; 3]> = (0..=2)
            .flat_map(|z| (0..=7).map(move |r| [5, r, z]))
            .filter(|&c| c != hole)
            .collect();
        s.edit(&wall, stone);
        s.run(1500, 1.0);
        let up = s
            .water
            .basin_at(s.cell(2, 0, 1))
            .expect("the river above")
            .clone();
        let down = s.water.basin_at(s.cell(8, 0, 1)).expect("the river below");
        assert_ne!(up.id, down.id, "two pools, the hole between");
        let c = discharge(1.7);
        let head = (channel_q() / (c * CELL_H)).powi(2) / (2.0 * crate::water::G);
        let rise = up.level() - down.level();
        assert!(
            (rise - head).abs() < 0.05 * head,
            "risen {rise} m over the river below (the law: {head} m)"
        );
        for r in 1..=3 {
            assert!(s.wet(2, r, 1), "risen through row {r}");
        }
        assert!(!s.wet(2, 6, 1), "not to the wall's top");
        for x in [7, 8, 9] {
            assert!(
                s.wet(x, 0, 1) && !s.wet(x, 1, 1),
                "the river below as it was at x {x}"
            );
        }
    }
);

/// A closed lake (x 0–9, z 0–9, water rows 0–2 under walls to row 4) and a wall across it at
/// x = 5 to row 3 (over the water, what it displaces included) but for `open` (cells left in it).
fn walled_lake(f: FaceFrame, open: &[[i32; 3]]) -> Scene {
    let mut s = Scene::with_air(f, [-4, -8, -4], [28, 8, 16]);
    let stone = require("dwell:stone");
    s.fill([-1, -1, -1], [10, -1, 10], stone);
    s.fill([-1, -1, -1], [10, 4, -1], stone);
    s.fill([-1, -1, 10], [10, 4, 10], stone);
    s.fill([-1, -1, -1], [-1, 4, 10], stone);
    s.fill([10, -1, -1], [10, 4, 10], stone);
    s.fill([11, -6, -2], [24, -6, 12], stone);
    s.fill([0, 0, 0], [9, 2, 9], M.water);
    s.water = fed(0.0);
    s.edit(&[[-1, 4, 4]], M.air);
    s.run(3, 1.0);
    let wall: Vec<[i32; 3]> = (0..=9)
        .flat_map(|z| (0..=3).map(move |r| [5, r, z]))
        .filter(|c| !open.contains(c))
        .collect();
    s.edit(&wall, stone);
    s.run(3, 1.0);
    s
}

dwell_world::both_faces!(
    /// The opening law decides what is one body (docs/WATER_FLOW.md §4): a lake with a wall
    /// across it but a wide gap (6 × 4 cells) is one basin; with only one cell open the two halves
    /// are two, and a cut draining one half leaves the other above it a while, evening out through
    /// the hole, both down to the cut's floor in the end. Red before: one basin, one level.
    fn a_gap_wide_enough_is_one_body_a_hole_two(f: FaceFrame) {
        let gap: Vec<[i32; 3]> = (2..=7)
            .flat_map(|z| (0..=3).map(move |r| [5, r, z]))
            .collect();
        let s = walled_lake(f, &gap);
        let a = s.water.basin_at(s.cell(2, 0, 4)).map(|b| b.id);
        let b = s.water.basin_at(s.cell(8, 0, 4)).map(|b| b.id);
        assert!(a.is_some() && a == b, "one body through the gap");

        let mut s = walled_lake(f, &[[5, 0, 4]]);
        let near = s.water.basin_at(s.cell(8, 0, 4)).map(|b| b.id);
        let far = s.water.basin_at(s.cell(2, 0, 4)).map(|b| b.id);
        assert!(near.is_some() && near != far, "two through a hole");
        s.edit(&[[10, 1, 4], [10, 2, 4], [10, 3, 4]], M.air);
        s.run(40, 1.0);
        let level = |s: &Scene, x: i32| s.water.basin_at(s.cell(x, 0, 4)).expect("a half").level();
        assert!(
            level(&s, 2) > level(&s, 8) + 0.1,
            "the far half lags: {} over {}",
            level(&s, 2),
            level(&s, 8)
        );
        s.run(2000, 1.0);
        let floor = m_of_row(G + 1);
        for x in [2, 8] {
            assert!(
                (level(&s, x) - floor).abs() < CELL_H / 2.0,
                "drained to the cut's floor at x {x}: {}",
                level(&s, x)
            );
        }
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

dwell_world::both_faces!(
    /// Owner, 2026-11-03: a dam's flood reached only so far up the river, and an edit there then
    /// lowered it. A river runs out of the loaded world (the chunk west of x = 0 not loaded when
    /// the dam goes in at x = 9): the flood stops at the unloaded edge, a wall to water. When the
    /// chunk loads, the flood goes on up the river, and an edit there changes nothing.
    fn a_dams_flood_goes_on_up_the_river_when_its_chunks_load(f: FaceFrame) {
        let mut s = Scene::with_air(f, [-24, -8, -8], [32, 12, 16]);
        let stone = require("dwell:stone");
        s.fill([-21, -1, -1], [9, -1, 3], stone);
        s.fill([-21, -1, -1], [9, 8, -1], stone);
        s.fill([-21, -1, 3], [9, 8, 3], stone);
        s.fill([-21, -1, -1], [-21, 8, 3], stone);
        s.fill([9, -6, -3], [21, -6, 5], stone);
        s.fill([9, -6, -3], [9, -3, 5], stone);
        s.fill([21, -6, -3], [21, -3, 5], stone);
        s.fill([9, -6, -3], [21, -3, -3], stone);
        s.fill([9, -6, 5], [21, -3, 5], stone);
        s.fill([-20, 0, 0], [9, 0, 2], M.water);
        let c = s.cell(-5, 0, 1);
        let west = chunk_of(c[0], c[1], c[2]);
        let e = s.cell(5, 0, 1);
        assert_ne!(west, chunk_of(e[0], e[1], e[2]));
        let saved = s.world.find(west).expect("loaded").clone();
        s.world.remove(west);
        let dam: Vec<[i32; 3]> = (0..=2)
            .flat_map(|z| (0..=2).map(move |r| [9, r, z]))
            .collect();
        s.edit(&dam, stone);
        s.run(600, 1.0);
        assert!(s.wet(4, 3, 1), "the flood behind the dam");
        s.world.put(west, saved);
        s.run(900, 1.0);
        for x in [-20, -10, -5, -1, 4] {
            assert!(s.wet(x, 3, 1), "the flood up the river at x {x}");
        }
        s.edit(&[[-10, 6, 1]], M.air);
        s.run(900, 1.0);
        for x in [-20, -10, -5, -1, 4] {
            assert!(s.wet(x, 3, 1), "the flood holds after an edit at x {x}");
        }
    }
);

dwell_world::both_faces!(
    /// Owner, 2026-11-03: a dam must fill whether or not anyone is near, so the river below it
    /// changes for whoever is there. A dam set in a channel, the player then 4 km away: the pool
    /// behind it is filling (active), so it flows on with no player near, rises a row and spills
    /// over the dam at the river's flow, then settles. Red before: it waited as it was.
    fn a_dam_out_of_reach_fills_on_until_it_settles(f: FaceFrame) {
        let mut s = channel(f);
        let far = f.point([4000.5, m_of_row(G) + 1.0, 4.5]);
        s.edit(&[[9, 0, 0], [9, 0, 1], [9, 0, 2]], require("dwell:stone"));
        s.run_at(1, 1.0, far);
        assert!(
            s.water.basin_at(s.cell(4, 0, 1)).expect("the pool").active,
            "filling"
        );
        assert!(!s.water.wanted_chunks().is_empty(), "its chunks wanted");
        s.run_at(900, 1.0, far);
        for x in 0..=8 {
            assert!(s.wet(x, 1, 1), "the pool rose into row 1 at x {x}");
        }
        let want = channel_q();
        let pool = s.water.basin_at(s.cell(4, 0, 1)).expect("the pool");
        let over: f64 = pool.outlets.iter().map(|o| o.flow).sum();
        assert!(
            (over - want).abs() < 0.05 * want,
            "spills {over} m³/s of {want}"
        );
        assert!(!pool.active, "settled");
    }
);

dwell_world::both_faces!(
    /// A river far from players comes back at its steady state when a player returns
    /// (docs/WATER_FLOW.md §6): a dam set in a channel whose chunk then unloads for ten minutes of
    /// water time (the pool waits, as it was, owing the time), then loads again: within a few ticks
    /// the pool stands where the same dam watched all along does. Red before: it came back as it
    /// was left, filling from there.
    fn a_dammed_river_unloaded_comes_back_as_if_watched(f: FaceFrame) {
        let dam = [[9, 0, 0], [9, 0, 1], [9, 0, 2]];
        let mut watched = channel(f);
        watched.edit(&dam, require("dwell:stone"));
        watched.run(604, 1.0);
        let mut s = channel(f);
        s.edit(&dam, require("dwell:stone"));
        s.run(1, 1.0);
        let c = s.cell(4, 0, 1);
        let k = chunk_of(c[0], c[1], c[2]);
        let saved = s.world.find(k).expect("loaded").clone();
        s.world.remove(k);
        s.run(600, 1.0);
        s.world.put(k, saved);
        assert!(!s.wet(4, 1, 1), "waited as it was");
        s.run(3, 1.0);
        let level = |s: &Scene| s.water.basin_at(s.cell(4, 0, 1)).expect("the pool").level();
        assert!(
            (level(&s) - level(&watched)).abs() < 0.1,
            "at {} (watched: {})",
            level(&s),
            level(&watched)
        );
        for x in 0..=8 {
            assert!(s.wet(x, 1, 1), "the pool rose into row 1 at x {x}");
        }
        let pool = s.water.basin_at(s.cell(4, 0, 1)).expect("the pool");
        assert_eq!(pool.owed, 0.0, "caught up");
    }
);

/// Each chunk's revision in the scene's world now (0 unloaded).
fn revisions(s: &Scene) -> impl Fn(dwell_world::ChunkCoord) -> u32 + '_ {
    |c| s.world.find(c).map_or(0, |ch| ch.revision())
}

dwell_world::both_faces!(
    /// The basins are saved with the world (docs/WATER_FLOW.md §8): a dammed pool half way to its
    /// spill, saved and taken in by a new water (a restart), fills on as the one that went on did
    /// (its level, its spill, and the flow network that feeds it). Red before: a restart lost the
    /// pool, and the next edit found it at rest, as it then stood, below the dam's crest.
    fn a_basin_saved_with_the_world_goes_on_as_it_was(f: FaceFrame) {
        let mut s = channel(f);
        s.edit(&[[9, 0, 0], [9, 0, 1], [9, 0, 2]], require("dwell:stone"));
        s.run(10, 1.0);
        let bytes = s.water.save(revisions(&s));
        let mut back = fed(channel_q());
        assert_eq!(
            back.restore(&bytes, revisions(&s)),
            Some(s.water.basins().count())
        );
        // The same world (its chunks as saved) under the restored water.
        let mut t = Scene::new(f);
        for x in (-16..=32).step_by(4) {
            for r in (-8..=8).step_by(4) {
                for z in (-8..=16).step_by(4) {
                    let c = s.cell(x, r, z);
                    let k = chunk_of(c[0], c[1], c[2]);
                    if let Some(ch) = s.world.find(k) {
                        t.world.put(k, ch.clone());
                    }
                }
            }
        }
        t.water = back;
        s.run(600, 1.0);
        t.run(600, 1.0);
        let level = |s: &Scene| s.water.basin_at(s.cell(4, 0, 1)).expect("the pool").level();
        assert!(
            (level(&s) - level(&t)).abs() < 1e-6,
            "{} went on, {} restored",
            level(&s),
            level(&t)
        );
        for x in 0..=8 {
            assert!(t.wet(x, 1, 1), "restored, risen into row 1 at x {x}");
        }
        // The same bytes again: the save is the state.
        assert_eq!(
            s.water.save(revisions(&s)).len(),
            s.water.save(revisions(&s)).len()
        );
    }
);

dwell_world::both_faces!(
    /// A saved basin whose chunk changed since (another build, which doesn't save the basins,
    /// edited it) is left out: the next edit finds the water again as it then is.
    fn a_saved_basin_whose_chunk_changed_since_is_found_again(f: FaceFrame) {
        let mut s = channel(f);
        s.edit(&[[9, 0, 0], [9, 0, 1], [9, 0, 2]], require("dwell:stone"));
        s.run(10, 1.0);
        let bytes = s.water.save(revisions(&s));
        s.set(4, 2, 1, require("dwell:stone"));
        let mut back = fed(channel_q());
        assert_eq!(back.restore(&bytes, revisions(&s)), Some(0));
        assert!(back.basin_at(s.cell(4, 0, 1)).is_none());
    }
);

/// A straight map river along +x through z −1 to 3 (m along it: x + 1000), 5 km long, into the
/// sea.
struct Straight;

impl Rivers for Straight {
    fn at(&self, _: dwell_world::Face, x: i32, z: i32) -> Option<(u32, f64)> {
        (-1..=3).contains(&z).then_some((0, f64::from(x) + 1000.0))
    }
    fn length(&self, _: dwell_world::Face, _: u32) -> f64 {
        5000.0
    }
    fn next(&self, _: dwell_world::Face, _: u32) -> RiverNext {
        RiverNext::End
    }
}

/// [`channel`] moved `dx` along x (a stretch of the same river farther down), walled to row `top`.
fn channel_at(s: &mut Scene, dx: i32, top: i32) {
    let stone = require("dwell:stone");
    let mut fill = |lo: [i32; 3], hi: [i32; 3], m| {
        s.fill([lo[0] + dx, lo[1], lo[2]], [hi[0] + dx, hi[1], hi[2]], m);
    };
    fill([-1, -1, -1], [9, -1, 3], stone);
    fill([-1, -1, -1], [9, top, -1], stone);
    fill([-1, -1, 3], [9, top, 3], stone);
    fill([-1, -1, -1], [-1, top, 3], stone);
    fill([9, -6, -3], [21, -6, 5], stone);
    fill([9, -6, -3], [9, -3, 5], stone);
    fill([21, -6, -3], [21, -3, 5], stone);
    fill([9, -6, -3], [21, -3, -3], stone);
    fill([9, -6, 5], [21, -3, 5], stone);
    fill([0, 0, 0], [9, 0, 2], M.water);
}

dwell_world::both_faces!(
    /// Owner, 2026-11-03: two players far apart on one river: a dam upstream must reach the river
    /// downstream across the country between. Two stretches of one map river 40 m apart, nothing
    /// found between them: a dam across the upper one holds its flow back while it fills, the
    /// change goes down the map's river at `riverSpeed` and the lower stretch, taking in less,
    /// falls; once the dam spills the river's flow again, the lower one comes back. Red before:
    /// the lower stretch never knew.
    fn a_dam_upstream_reaches_the_river_far_below_across_unfound_country(f: FaceFrame) {
        let mut s = Scene::with_air(f, [-16, -8, -8], [72, 10, 16]);
        s.water = fed(channel_q()).with_rivers(std::sync::Arc::new(Straight));
        channel_at(&mut s, 0, 8);
        channel_at(&mut s, 40, 3);
        // Both found as they were (an edit beside each), at rest.
        s.edit(&[[4, 1, 1], [44, 1, 1]], M.air);
        s.run(5, 1.0);
        let lower = |s: &Scene| s.water.basin_at(s.cell(44, 0, 1)).expect("lower").volume;
        let full = lower(&s);
        // A dam six rows high: it takes a while to fill.
        let dam: Vec<[i32; 3]> = (0..=2)
            .flat_map(|z| (0..=5).map(move |r| [9, r, z]))
            .collect();
        s.edit(&dam, require("dwell:stone"));
        // 40 m at 1 m/s: nothing yet after 20 s.
        s.run(20, 1.0);
        assert!(
            (lower(&s) - full).abs() < 1e-6,
            "not reached yet: {} of {full}",
            lower(&s)
        );
        // Reached: the lower stretch takes in nothing and falls.
        s.run(60, 1.0);
        assert!(
            lower(&s) < 0.8 * full,
            "the river below falls: {} of {full}",
            lower(&s)
        );
        // What is going down the river is saved with the basins.
        let bytes = s.water.save(revisions(&s));
        let mut back = fed(channel_q());
        back.restore(&bytes, revisions(&s)).expect("read back");
        assert!(!s.water.fronts().is_empty(), "changes going down the river");
        assert_eq!(back.fronts(), s.water.fronts());
        // The dam fills and spills the flow again: the lower stretch comes back.
        s.run(1500, 1.0);
        assert!(
            (lower(&s) - full).abs() < 0.05 * full,
            "the river below back: {} of {full}",
            lower(&s)
        );
    }
);

dwell_world::both_faces!(
    /// Owner, 2026-11-03: water should fill up smoothly, not by whole ½ m rows. A dammed pool's
    /// surface row is written at its fill (eighths of the row, `fillProperty`): as it rises the
    /// fill goes up step by step, the rows under it full, and a body found from it holds what the
    /// fills hold. Red before: the row appeared whole once half full.
    fn a_rising_pool_s_surface_row_is_written_at_its_fill(f: FaceFrame) {
        let mut s = tall_channel(f);
        // A dam three rows high, the river flowing in: the pool rises through rows 1 and 2.
        let dam: Vec<[i32; 3]> = (0..=2)
            .flat_map(|z| (0..=2).map(move |r| [9, r, z]))
            .collect();
        s.edit(&dam, require("dwell:stone"));
        let fill = |s: &Scene, r: i32| {
            let m = s.get(4, r, 1);
            dwell_blocks::try_info(m)
                .filter(|i| i.liquid)
                .map_or(0.0, |i| f64::from(i.fill))
        };
        let mut seen: Vec<f64> = Vec::new();
        for _ in 0..40 {
            s.run(1, 1.0);
            let f1 = fill(&s, 1);
            if f1 > 0.0 && f1 < 1.0 && seen.last() != Some(&f1) {
                seen.push(f1);
            }
        }
        assert!(seen.len() >= 4, "row 1 filled by steps, saw {seen:?}");
        assert!(seen.windows(2).all(|w| w[1] > w[0]), "rising: {seen:?}");
        // Under the surface row, full.
        let pool = s.water.basin_at(s.cell(4, 0, 1)).expect("the pool");
        assert!(pool.level() > m_of_row(G + 1), "risen into row 1");
        assert_eq!(fill(&s, 0), 1.0, "row 0 full");
    }
);
