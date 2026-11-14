//! The town sites a face's generator keeps clear (docs/BLUEPRINTS.md §6, BUGS.md #169): no tree,
//! bush or boulder is placed reaching a planned town's layout box plus `towns.clearM`, so a town is
//! founded on clear ground. The sites are the server's own (`TownRules::site_in` on
//! [`town_ground`]), a pure function of the world seed, the face and the face's height field, so the
//! generator knows them before any town is founded.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use dwell_world::blueprint::{SHIPPED, TownRules, TownSite};
use dwell_world::{Face, FaceFrame};

use crate::terrain::TerrainGenerator;

/// The ground a town site needs at a face-local column of `g`'s face: its height (face-local m)
/// where it is dry land (no sea, lake, river or stream, inside the world) within `towns.heightM`
/// above sea level, clear of the shrines and the portals, else None.
pub fn town_ground(g: &TerrainGenerator, x: i32, z: i32) -> Option<f64> {
    let [lo, hi] = SHIPPED.towns.height_m;
    let sea = dwell_world::SEA_LEVEL_M;
    let c = g.column_at(x, z);
    let h = f64::from(c.height);
    let dry = !c.outside
        && c.water == crate::terrain::Water::None
        && !c.stream
        && h >= sea + lo
        && h <= sea + hi;
    // Clear of the resurrection shrines (their clear ground, and a margin) and the portals.
    (dry && !g
        .shrines()
        .is_some_and(|sh| sh.crowds(g, x, z, crate::TOWN_SHRINE_MARGIN_M)))
    .then_some(h)
}

/// A face's town sites as its generator sees them: sited once per grid cell and cached.
#[derive(Debug)]
pub struct Towns {
    world_seed: u64,
    face: Face,
    rules: TownRules,
    /// Each plan's clear box about its site's centre (`TownRules::plan_box`).
    boxes: Vec<[i32; 4]>,
    /// The furthest any plan's clear box reaches from its site's centre (m).
    reach: i32,
    cells: RwLock<HashMap<[i32; 2], Option<TownSite>>>,
}

impl Towns {
    /// The shipped rules' towns of `face` in the world of `world_seed` (the server's).
    pub fn new(world_seed: u64, face: Face) -> Arc<Self> {
        Self::with_rules(world_seed, face, SHIPPED.towns)
    }

    /// [`Self::new`] with these rules (tests).
    pub fn with_rules(world_seed: u64, face: Face, rules: TownRules) -> Arc<Self> {
        let boxes: Vec<[i32; 4]> = (0..rules.plans.len()).map(|p| rules.plan_box(p)).collect();
        let reach = boxes
            .iter()
            .flat_map(|b| b.iter().map(|v| v.abs()))
            .max()
            .unwrap_or(0);
        Arc::new(Self {
            world_seed,
            face,
            rules,
            boxes,
            reach,
            cells: RwLock::new(HashMap::new()),
        })
    }

    /// The site in grid cell `cell`, if any.
    pub fn in_cell(&self, g: &TerrainGenerator, cell: [i32; 2]) -> Option<TownSite> {
        if let Some(t) = self.cells.read().expect("town cells").get(&cell) {
            return *t;
        }
        let ground = |x: i32, z: i32| town_ground(g, x, z);
        let found = self.rules.site_in(
            self.world_seed,
            FaceFrame { face: self.face },
            cell,
            &ground,
        );
        self.cells.write().expect("town cells").insert(cell, found);
        found
    }

    /// A site's clear box (face-local columns [x0, z0, x1, z1], inclusive).
    pub fn clear_box(&self, t: &TownSite) -> [i32; 4] {
        let [x0, z0, x1, z1] = self.boxes[t.plan];
        [t.x + x0, t.z + z0, t.x + x1, t.z + z1]
    }

    /// True when columns [x0, x1] × [z0, z1] reach any town site's clear box.
    pub fn box_reaches_clear(
        &self,
        g: &TerrainGenerator,
        (x0, z0): (i32, i32),
        (x1, z1): (i32, i32),
    ) -> bool {
        if self.boxes.is_empty() {
            return false;
        }
        let r = self.reach;
        let lo = self.rules.cell_of(x0 - r, z0 - r);
        let hi = self.rules.cell_of(x1 + r, z1 + r);
        for k in lo[1]..=hi[1] {
            for i in lo[0]..=hi[0] {
                if let Some(t) = self.in_cell(g, [i, k]) {
                    let [bx0, bz0, bx1, bz1] = self.clear_box(&t);
                    if bx0 <= x1 && bx1 >= x0 && bz0 <= z1 && bz1 >= z0 {
                        return true;
                    }
                }
            }
        }
        false
    }
}
