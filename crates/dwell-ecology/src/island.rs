//! The island the ecology runs on: a coarse face-local grid (`island.cellM`) of walkable land with
//! each cell's most forage, sampled once from the face's world map (or from any ground sampler, for
//! tests and fixture worlds).

use crate::data::{Data, Diet};
use dwell_worldgen::worldmap::{NO_LAKE, WorldMap};

/// What a sampler says of the ground at a cell's centre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ground {
    /// Sea or lake: no forage, not walked.
    Water,
    Land {
        height_m: f64,
        slope_deg: f64,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Island {
    /// `n` × `n` cells of `cell` m; cell (i, j) is centred on face-local (x0 + i·cell, x0 + j·cell).
    pub n: usize,
    pub cell: f64,
    pub x0: f64,
    pub walkable: Vec<bool>,
    /// The most forage each cell holds (0 on water).
    pub grazing_max: Vec<f64>,
}

impl Island {
    /// An island of `n` × `n` cells of the data's size from `x0`, each cell's ground sampled at its
    /// centre.
    pub fn from_ground(
        data: &Data,
        n: usize,
        x0: f64,
        sample: impl Fn(f64, f64) -> Ground,
    ) -> Self {
        let cell = data.island.cell_m;
        let mut walkable = vec![false; n * n];
        let mut grazing_max = vec![0.0; n * n];
        for c in 0..n * n {
            let (x, z) = (x0 + (c % n) as f64 * cell, x0 + (c / n) as f64 * cell);
            if let Ground::Land {
                height_m,
                slope_deg,
            } = sample(x, z)
            {
                walkable[c] = true;
                grazing_max[c] = data.grazing_for(height_m, slope_deg);
            }
        }
        Self {
            n,
            cell,
            x0,
            walkable,
            grazing_max,
        }
    }

    /// The island of a face's world map: its sea and lakes are water, its land graded by the
    /// eroded height and slope.
    pub fn from_world_map(map: &WorldMap, data: &Data) -> Self {
        let cell = data.island.cell_m;
        let span = map.n as f64 * map.cell;
        let n = (span / cell).ceil().max(1.0) as usize;
        let x0 = map.x0 - map.cell / 2.0 + cell / 2.0;
        Self::from_ground(data, n, x0, |x, z| {
            let c = map.cell_at(x, z);
            if map.is_sea(c) || map.lake[c] != NO_LAKE {
                return Ground::Water;
            }
            let (h, gx, gz) = map.height_at(x, z);
            Ground::Land {
                height_m: h,
                slope_deg: gx.hypot(gz).atan().to_degrees(),
            }
        })
    }

    /// The cell holding face-local (x, z), if it is on the grid.
    pub fn cell_at(&self, x: f64, z: f64) -> Option<usize> {
        let i = ((x - self.x0) / self.cell + 0.5).floor();
        let j = ((z - self.x0) / self.cell + 0.5).floor();
        let n = self.n as f64;
        (i >= 0.0 && j >= 0.0 && i < n && j < n).then(|| j as usize * self.n + i as usize)
    }

    pub fn centre(&self, c: usize) -> (f64, f64) {
        (
            self.x0 + (c % self.n) as f64 * self.cell,
            self.x0 + (c / self.n) as f64 * self.cell,
        )
    }

    pub fn is_walkable(&self, x: f64, z: f64) -> bool {
        self.cell_at(x, z).is_some_and(|c| self.walkable[c])
    }

    /// The most head of `species` the island holds: for a grazer, the forage regrown a day when
    /// grazed bare over its feed a head; for a hunter, its prey's capacity over the prey it needs a
    /// head. Breeding slows toward it, so the island-wide total stays under it (floors aside).
    pub fn carrying_capacity(&self, data: &Data, species: usize) -> f64 {
        match data.species[species].diet {
            Diet::Graze { per_head_per_day } => {
                self.grazing_max.iter().sum::<f64>() * data.regrow_per_day / per_head_per_day
            }
            Diet::Prey {
                species: prey,
                prey_per_head,
                ..
            } => {
                prey.iter()
                    .map(|&p| self.carrying_capacity(data, p))
                    .sum::<f64>()
                    / prey_per_head
            }
        }
    }
}
