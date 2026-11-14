//! The island the ecology runs on: a coarse face-local grid (`island.cellM`) of walkable land with
//! each cell's most forage, sampled once from the face's world map (or from any ground sampler, for
//! tests and fixture worlds). Only the land is stored, its forage as f32 (docs/ECOLOGY.md §6).

use crate::data::{Data, Diet};
use dwell_worldgen::worldmap::{NO_LAKE, WorldMap};
use std::mem::size_of;

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

/// A run of land cells in one row of the grid: cells `i0..i0 + len` of the row, land cells
/// `first..first + len`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Run {
    i0: u32,
    len: u32,
    first: u32,
}

/// No ground class fits the cell (land, but no forage and no refuge).
const NO_GROUND: u8 = u8::MAX;

/// The ecology's island: only its land is stored (docs/ECOLOGY.md §6, "Cost"). The grid is
/// `n` × `n` cells of `cell` m, cell (i, j) centred on face-local (x0 + i·cell, x0 + j·cell); its
/// land cells are numbered in row-major order (`0..cells()`), each row's land kept as runs, so a
/// cell's index is found by a search in its row. Water is not stored: off the land is not walked
/// and holds nothing.
#[derive(Clone, Debug, PartialEq)]
pub struct Island {
    pub n: usize,
    pub cell: f64,
    pub x0: f64,
    /// Each row's runs: `runs[rows[j]..rows[j + 1]]`.
    rows: Vec<u32>,
    runs: Vec<Run>,
    /// The most forage each land cell holds (deer-days; f32: plenty for a grid at 200 m).
    pub grazing_max: Vec<f32>,
    /// Each land cell's ground class (an index into `refuges`; [`NO_GROUND`] none).
    ground: Vec<u8>,
    /// Each ground class's refuge (the data's): the share of a group there hunters can't take.
    refuges: Vec<f64>,
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
        assert!(n <= u32::MAX as usize, "an island of {n} cells a side");
        assert!(
            data.grounds.len() < NO_GROUND as usize,
            "too many ground classes"
        );
        let cell = data.island.cell_m;
        let mut rows = Vec::with_capacity(n + 1);
        let mut runs: Vec<Run> = Vec::new();
        let mut grazing_max = Vec::new();
        let mut ground = Vec::new();
        for j in 0..n {
            rows.push(runs.len() as u32);
            let mut open = false;
            for i in 0..n {
                let (x, z) = (x0 + i as f64 * cell, x0 + j as f64 * cell);
                let Ground::Land {
                    height_m,
                    slope_deg,
                } = sample(x, z)
                else {
                    open = false;
                    continue;
                };
                if !open {
                    runs.push(Run {
                        i0: i as u32,
                        len: 0,
                        first: ground.len() as u32,
                    });
                    open = true;
                }
                runs.last_mut().expect("a run is open").len += 1;
                let class = data
                    .grounds
                    .iter()
                    .position(|g| height_m <= g.max_height_m && slope_deg <= g.max_slope_deg);
                grazing_max.push(class.map_or(0.0, |k| data.grounds[k].max) as f32);
                ground.push(class.map_or(NO_GROUND, |k| k as u8));
            }
        }
        rows.push(runs.len() as u32);
        let mut island = Self {
            n,
            cell,
            x0,
            rows,
            runs,
            grazing_max,
            ground,
            refuges: data.grounds.iter().map(|g| g.refuge).collect(),
        };
        island.shrink();
        island
    }

    fn shrink(&mut self) {
        self.runs.shrink_to_fit();
        self.grazing_max.shrink_to_fit();
        self.ground.shrink_to_fit();
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

    /// Scales each cell's most forage by the grass's climate share at its centre (0..1;
    /// docs/ECOLOGY.md §10, `dwell_worldgen::grass::far_share`), so the far forage regrows toward
    /// the same climate cap the near tall grass grows to: a cell's forage over its ground's most is
    /// its tall grass over its cap.
    pub fn scale_grazing(&mut self, share: impl Fn(f64, f64) -> f64) {
        for c in 0..self.cells() {
            if self.grazing_max[c] > 0.0 {
                let (x, z) = self.centre(c);
                self.grazing_max[c] =
                    (f64::from(self.grazing_max[c]) * share(x, z).clamp(0.0, 1.0)) as f32;
            }
        }
    }

    /// The land cells (the ones stored: `0..cells()`).
    pub fn cells(&self) -> usize {
        self.grazing_max.len()
    }

    /// The land cell at grid column `i`, row `j`, if that cell is land.
    pub fn index(&self, i: usize, j: usize) -> Option<usize> {
        if i >= self.n || j >= self.n {
            return None;
        }
        let row = &self.runs[self.rows[j] as usize..self.rows[j + 1] as usize];
        let i = i as u32;
        let k = row.partition_point(|r| r.i0 + r.len <= i);
        let r = row.get(k)?;
        (r.i0 <= i).then(|| (r.first + i - r.i0) as usize)
    }

    /// The grid column and row of land cell `c`.
    pub fn grid_of(&self, c: usize) -> (usize, usize) {
        let c = c as u32;
        let k = self.runs.partition_point(|r| r.first + r.len <= c);
        let r = self.runs[k];
        let j = self.rows.partition_point(|&s| s as usize <= k) - 1;
        ((r.i0 + c - r.first) as usize, j)
    }

    /// The grid column and row holding face-local (x, z), if it is on the grid.
    pub fn grid_at(&self, x: f64, z: f64) -> Option<(usize, usize)> {
        let i = ((x - self.x0) / self.cell + 0.5).floor();
        let j = ((z - self.x0) / self.cell + 0.5).floor();
        let n = self.n as f64;
        (i >= 0.0 && j >= 0.0 && i < n && j < n).then_some((i as usize, j as usize))
    }

    /// The land cell holding face-local (x, z), if it is land on the grid.
    pub fn cell_at(&self, x: f64, z: f64) -> Option<usize> {
        let (i, j) = self.grid_at(x, z)?;
        self.index(i, j)
    }

    /// The centre of grid column `i`, row `j` (face-local).
    pub fn grid_centre(&self, i: usize, j: usize) -> (f64, f64) {
        (
            self.x0 + i as f64 * self.cell,
            self.x0 + j as f64 * self.cell,
        )
    }

    /// The centre of land cell `c` (face-local).
    pub fn centre(&self, c: usize) -> (f64, f64) {
        let (i, j) = self.grid_of(c);
        self.grid_centre(i, j)
    }

    /// Land cell `c`'s refuge (its ground's).
    pub fn refuge(&self, c: usize) -> f64 {
        self.refuges
            .get(self.ground[c] as usize)
            .copied()
            .unwrap_or(0.0)
    }

    /// The refuge at face-local (x, z) (0 off the land).
    pub fn refuge_at(&self, x: f64, z: f64) -> f64 {
        self.cell_at(x, z).map_or(0.0, |c| self.refuge(c))
    }

    pub fn is_walkable(&self, x: f64, z: f64) -> bool {
        self.cell_at(x, z).is_some()
    }

    /// Every land cell's grid index (`j · n + i`), in order: how a version 1 record's whole-grid
    /// forage maps onto the land cells.
    pub fn grid_indices(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.n).flat_map(move |j| {
            self.runs[self.rows[j] as usize..self.rows[j + 1] as usize]
                .iter()
                .flat_map(move |r| (r.i0..r.i0 + r.len).map(move |i| j * self.n + i as usize))
        })
    }

    /// The heap the island holds (bytes).
    pub fn heap_bytes(&self) -> usize {
        self.rows.capacity() * size_of::<u32>()
            + self.runs.capacity() * size_of::<Run>()
            + self.grazing_max.capacity() * size_of::<f32>()
            + self.ground.capacity()
            + self.refuges.capacity() * size_of::<f64>()
    }

    /// The most head of `species` the island holds: for a grazer, the forage regrown a day when
    /// grazed bare over its feed a head; for a hunter, its prey's capacity over the prey it needs a
    /// head. Breeding slows toward it, so the island-wide total stays under it.
    pub fn carrying_capacity(&self, data: &Data, species: usize) -> f64 {
        match data.species[species].diet {
            Diet::Graze { per_head_per_day } => {
                self.grazing_max.iter().map(|&g| f64::from(g)).sum::<f64>() * data.regrow_per_day
                    / per_head_per_day
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
