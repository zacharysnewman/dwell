//! The waterfalls' tests (docs/WATER_SURFACE_GRID.md §2e): scenes built face-locally, run on both
//! faces.

use super::*;
use crate::currents::{CA, current_key};
use crate::{FLAT_TOP_Y, empty_generator};
use dwell_blocks::require;

const BASE: i32 = FLAT_TOP_Y + 40;

/// A scene in face `f`'s local frame: rows offset from `BASE`.
struct Scene {
    w: VoxelWorld,
    f: FaceFrame,
}

impl Scene {
    /// Stone up to row −1, air above; a ledge (x ≤ 3) of stone to row 5, its water rows 6..=7
    /// (level 4 m above `BASE`'s row), its current `c` (m/s along x, z) for z 0..=`width − 1`;
    /// its water walled at z −1 and z `width`. The ground beyond (x ≥ 4) at row 0's floor (0 m).
    fn ledge(f: FaceFrame, width: i32, c: [f32; 2]) -> Self {
        let mut s = Self {
            w: VoxelWorld::new(empty_generator(), None),
            f,
        };
        let stone = require("dwell:stone");
        for x in -4..12 {
            for z in -4..8 {
                s.set(x, -1, z, stone);
                for r in 0..16 {
                    s.set(x, r, z, M.air);
                }
            }
        }
        for x in -4..=3 {
            for z in -1..=width {
                for r in 0..=5 {
                    s.set(x, r, z, stone);
                }
                let wall = z == -1 || z == width;
                for r in 6..=7 {
                    s.set(x, r, z, if wall { stone } else { M.water });
                }
            }
        }
        for x in -4..=3 {
            for z in 0..width {
                s.current(x, z, c);
            }
        }
        s
    }
    fn set(&mut self, x: i32, r: i32, z: i32, m: StateId) {
        let [a, b, c] = self.f.cell([x, BASE + r, z]);
        self.w.set_voxel(a, b, c, m);
    }
    fn current(&mut self, x: i32, z: i32, c: [f32; 2]) {
        let (key, i) = current_key(self.f, x, z);
        let mut cells = self
            .w
            .currents
            .get(key)
            .cloned()
            .unwrap_or_else(|| Box::new([[0.0; 2]; CA]));
        cells[i] = c;
        self.w.currents.set(key, Some(cells));
    }
    /// A world point from face-local metres above `BASE`'s row.
    fn point(&self, x: f64, y: f64, z: f64) -> [f64; 3] {
        self.f.point([x, m_of_row(BASE) + y, z])
    }
    fn falls(&self) -> Vec<Fall> {
        falls_near(&self.w, self.point(4.0, 2.0, 1.0), 24.0)
    }
}

crate::both_faces!(
    fn water_running_over_a_ledge_falls_as_one_sheet_along_its_lip(f: FaceFrame) {
        let s = Scene::ledge(f, 3, [0.6, 0.0]);
        let falls = s.falls();
        assert_eq!(falls.len(), 1, "{falls:?}");
        let fall = falls[0];
        assert_eq!(
            (fall.face, fall.x, fall.z, fall.dir, fall.width),
            (f, 3, 0, 0, 3)
        );
        let base = m_of_row(BASE);
        assert!(
            (fall.lip - base - 4.0).abs() < 1e-9,
            "lip {}",
            fall.lip - base
        );
        assert!((fall.land - base).abs() < 1e-9, "land {}", fall.land - base);
        // the flow per metre: the depth (the ledge's water, 1 m) × the current over the lip
        assert!((fall.flow - 0.6).abs() < 1e-6, "flow {}", fall.flow);
        assert!((fall.discharge() - 1.8).abs() < 1e-5);
        let mid = fall.lip_mid();
        assert_eq!((mid[0], mid[2]), (4.0, 1.5));
    }
);

crate::both_faces!(
    fn still_water_water_running_away_and_a_short_drop_make_no_fall(f: FaceFrame) {
        assert!(Scene::ledge(f, 2, [0.0, 0.0]).falls().is_empty(), "still");
        assert!(
            Scene::ledge(f, 2, [-0.6, 0.0]).falls().is_empty(),
            "running away from the lip"
        );
        assert!(
            Scene::ledge(f, 2, [0.0, 0.6]).falls().is_empty(),
            "along it, into a wall"
        );
        // the ground beyond raised to 1 row under the lip: a 0.5 m step, under `min_drop_m`
        let mut s = Scene::ledge(f, 2, [0.6, 0.0]);
        let stone = require("dwell:stone");
        for x in 4..12 {
            for z in -1..=2 {
                for r in 0..=6 {
                    s.set(x, r, z, stone);
                }
            }
        }
        assert!(s.falls().is_empty(), "{:?}", s.falls());
        // a wall beside the lip
        let mut s = Scene::ledge(f, 2, [0.6, 0.0]);
        for z in 0..2 {
            s.set(4, 7, z, stone);
        }
        assert!(s.falls().is_empty(), "walled");
    }
);

crate::both_faces!(
    fn a_fall_lands_on_the_water_below_it(f: FaceFrame) {
        let mut s = Scene::ledge(f, 2, [0.6, 0.0]);
        // a pool below, 2 rows deep and half a cell more (level 1.25 m)
        for x in 4..12 {
            for z in 0..2 {
                s.set(x, 0, z, M.water);
                s.set(x, 1, z, M.water);
                s.set(x, 2, z, dwell_blocks::liquid_at_fill(M.water, 0.5));
            }
        }
        let falls = s.falls();
        assert_eq!(falls.len(), 1, "{falls:?}");
        let land = falls[0].land - m_of_row(BASE);
        assert!((land - 1.25).abs() < 0.07, "lands on the pool: {land}");
    }
);

crate::both_faces!(
    fn what_is_in_a_sheet_is_pushed_down_and_out_and_cannot_rise(f: FaceFrame) {
        let s = Scene::ledge(f, 2, [0.6, 0.0]);
        // a body (0.9 m tall, 0.3 m radius) just beyond the lip, 2 m below it
        let inside = push_at(&s.w, s.point(4.4, 2.0, 0.5), 0.9, 0.3).expect("in the sheet");
        assert_eq!(inside.dir, 0);
        assert!(inside.strength > 0.9, "{inside:?}");
        // beyond its reach, above its lip, or in the water above it: nothing
        assert!(
            push_at(&s.w, s.point(6.5, 2.0, 0.5), 0.9, 0.3).is_none(),
            "too far out"
        );
        assert!(
            push_at(&s.w, s.point(4.4, 6.0, 0.5), 0.9, 0.3).is_none(),
            "above the lip"
        );
        assert!(
            push_at(&s.w, s.point(1.5, 4.5, 0.5), 0.9, 0.3).is_none(),
            "in the river above"
        );
        assert!(
            push_at(&s.w, s.point(4.4, 2.0, 3.5), 0.9, 0.3).is_none(),
            "beside it"
        );
        // swimming up it: carried down and out instead, never up
        let mut v = [0.0, 3.0, 0.0];
        for _ in 0..60 {
            v = push_velocity(v, inside, 1.0 / 60.0);
            assert!(v[1] <= 0.0, "{v:?}");
        }
        assert!(v[0] > 1.0 && v[1] < -2.0, "{v:?}");
        // already faster out and down than the water: left as it is
        let fast = [5.0, -9.0, 0.0];
        assert_eq!(push_velocity(fast, inside, 1.0 / 60.0), fast);
    }
);
