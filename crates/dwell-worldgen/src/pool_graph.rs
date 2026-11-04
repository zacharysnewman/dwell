//! The pool graph (docs/proposals/WATER_GRAPH.md, G1; ADR 0059): a face's rivers as data, derived
//! from the world map, the source of truth for its water. A river is a chain of **pools**, each a
//! reach of it with one level (a row of cells), joined by **lips** where it steps down a row: the
//! cells of the crest the generator carves there (`LipPlan`) and what they pour into. A river's
//! head is a **source** (its discharge in whole lip cells, `step_flows`); a river joining another
//! ends in a lip into the other's pool (or is one pool with it, at one level); one reaching the sea
//! ends in it. Every lip of a river passes the same flow down to a junction, which adds the joining
//! river's: the graph is balanced by construction, and `RiverGraph::unbalanced` says where it is
//! not.
//!
//! Built per river on demand from the channels' courses (deterministic per seed, no chunk loaded),
//! and cached.

use super::*;
use crate::worldmap::RiverEnd;

/// A pool of the graph: a river's reach at one level (`row`, the first dry row: the water fills
/// the rows under it), or the sea.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PoolKey {
    River { river: u32, row: i32 },
    Sea,
}

/// A lip of the graph: where pool `from` pours into pool `to`, a column of its crest (`at`), the
/// lip's cells (`want`, its river's flow in cells), the cells of the pool below its crest pours
/// into (`pours`) and how many its band is beside (`room`).
#[derive(Clone, Debug, PartialEq)]
pub struct GraphLip {
    pub from: PoolKey,
    pub to: PoolKey,
    pub at: (i64, i64),
    pub want: usize,
    pub pours: usize,
    pub room: usize,
    /// How fast the river's surface falls there (m per m): a step steeper than `STEEP_FALL` is
    /// part of a chute, not a pool's lip (G2).
    pub fall: f64,
}

/// A river surface falling faster than this (m per m: a row every 2.5 m) is a chute (G2): its
/// steps come too close for pools between them.
pub const STEEP_FALL: f64 = 0.2;

/// A pool of a river: its key, its reach along the river's course (m from its head), an estimate of
/// its surface (m²: the reach's length times the channel's width), and what comes in at its head
/// besides the lips into it (the river's source, m³/s; 0 below its head).
#[derive(Clone, Debug, PartialEq)]
pub struct GraphPool {
    pub key: PoolKey,
    pub reach: (f64, f64),
    pub area: f64,
    pub source: f64,
}

/// One river's pools (head first) and lips (in the order they step down), and the steps whose lip
/// wasn't found on the ground (where along the river, the row it steps from, how fast it falls).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RiverGraph {
    pub river: u32,
    pub pools: Vec<GraphPool>,
    pub lips: Vec<GraphLip>,
    pub missing: Vec<(f64, i32, f64)>,
}

/// The flow (m³/s) one lip cell passes: water's weir at a full row's head (`lip_cells`).
pub fn lip_cell_flow() -> f64 {
    dwell_world::flow::sill_flow(dwell_world::flow::water_weir(), 1.0, CELL_H)
}

impl RiverGraph {
    /// Its lips that don't pour into exactly their cells (each with what it pours and wants).
    pub fn unbalanced(&self) -> Vec<&GraphLip> {
        self.lips.iter().filter(|l| l.pours != l.want).collect()
    }

    /// [`Self::unbalanced`] but for the chutes' steps (`STEEP_FALL`).
    pub fn unbalanced_pools(&self) -> Vec<&GraphLip> {
        self.lips
            .iter()
            .filter(|l| l.pours != l.want && l.fall < STEEP_FALL)
            .collect()
    }
}

impl TerrainGenerator {
    /// The graph of river `river` (built once, then cached).
    pub fn river_graph(&self, river: u32) -> Arc<RiverGraph> {
        if let Some(g) = self.graphs.lock().expect("graphs").get(&river) {
            return g.clone();
        }
        let g = Arc::new(self.build_river_graph(river));
        self.graphs
            .lock()
            .expect("graphs")
            .entry(river)
            .or_insert(g)
            .clone()
    }

    /// The rivers of this face's map.
    pub fn river_count(&self) -> u32 {
        self.map.rivers.len() as u32
    }

    /// The pool full-detail column (x, z) lies in, if a river's channel (`PoolKey::River` by its
    /// river and row; the sea at the sea's level).
    pub fn pool_at(&self, x: i64, z: i64) -> Option<PoolKey> {
        let l = self.channel_col(x, z)?;
        Some(PoolKey::River {
            river: l.river,
            row: l.row,
        })
    }

    /// The face-local column whose meander-warped position is (wx, wz) (the channels' space): the
    /// warp undone by a few fixed-point steps (it is smooth: ±2 × `meander` m over
    /// `meanderWavelength`).
    fn unwarp(&self, wx: f64, wz: f64) -> (i64, i64) {
        let (mut x, mut z) = (wx, wz);
        for _ in 0..8 {
            let (ax, az) = self.warp(
                self.meander_seeds,
                x.round() as i64,
                z.round() as i64,
                S.river_meander,
                S.river_meander_wavelength,
            );
            x += wx - ax;
            z += wz - az;
        }
        (x.round() as i64, z.round() as i64)
    }

    /// A column of river `river`'s lip band at row `row` near face-local (x, z): the nearest, by
    /// rings out to `reach` columns.
    fn band_near(
        &self,
        river: u32,
        row: i32,
        (x, z): (i64, i64),
        reach: i64,
    ) -> Option<(i64, i64)> {
        for r in 0..=reach {
            let mut best: Option<(i64, (i64, i64))> = None;
            for dx in -r..=r {
                for dz in -r..=r {
                    if dx.abs().max(dz.abs()) != r {
                        continue;
                    }
                    let c = (x + dx, z + dz);
                    if self
                        .lip_col(c.0, c.1)
                        .is_some_and(|l| l.band && l.river == river && l.row == row)
                    {
                        let d = dx * dx + dz * dz;
                        if best.is_none_or(|(bd, bc)| (d, c) < (bd, bc)) {
                            best = Some((d, c));
                        }
                    }
                }
            }
            if let Some((_, c)) = best {
                return Some(c);
            }
        }
        None
    }

    fn build_river_graph(&self, river: u32) -> RiverGraph {
        let mut g = RiverGraph {
            river,
            ..RiverGraph::default()
        };
        let Some(course) = self.map.channels.courses.get(river as usize) else {
            return g;
        };
        let Some(r) = self.map.rivers.get(river as usize) else {
            return g;
        };
        if course.len() < 2 {
            return g;
        }
        let row_of = |s: f64| surface_row((SEA_LEVEL_M + s) as f32);
        // Along the course: each point's distance from the head.
        let mut along = vec![0.0; course.len()];
        for i in 1..course.len() {
            let (a, b) = (course[i - 1], course[i]);
            along[i] = along[i - 1] + (b.x - a.x).hypot(b.z - a.z);
        }
        let sea_row = row_of(CELL_H);
        let key = |row: i32| {
            if row <= sea_row {
                PoolKey::Sea
            } else {
                PoolKey::River { river, row }
            }
        };
        let mut start = 0.0;
        let mut row = row_of(course[0].s);
        let mut width = course[0].w;
        let mut source = course[0].q;
        for i in 1..course.len() {
            let (a, b) = (course[i - 1], course[i]);
            width = width.max(b.w);
            let kb = row_of(b.s);
            while row > kb && key(row) != PoolKey::Sea {
                // The step from `row` down a row: where the surface crosses its rounding edge.
                let edge = (f64::from(row) - 0.5) * CELL_H - SEA_LEVEL_M;
                let t = if (a.s - b.s).abs() > 1e-9 {
                    ((a.s - edge) / (a.s - b.s)).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                let at = along[i - 1] + (along[i] - along[i - 1]) * t;
                g.pools.push(GraphPool {
                    key: key(row),
                    reach: (start, at),
                    area: (at - start).max(0.0) * width,
                    source,
                });
                source = 0.0;
                start = at;
                width = b.w;
                let slope = (a.s - b.s) / (along[i] - along[i - 1]).max(1e-9);
                // Its band lies upstream of the crossing, within `lipReach`.
                let (px, pz) = (a.x + (b.x - a.x) * t, a.z + (b.z - a.z) * t);
                let near = self.unwarp(px, pz);
                let reach = (0.5 * b.w).ceil() as i64 + 6;
                match self.band_near(river, row, near, reach) {
                    Some(c) => {
                        let plan = self.lip_plan(c.0, c.1);
                        g.lips.push(GraphLip {
                            from: key(row),
                            to: key(row - 1),
                            at: plan.crest.iter().min().copied().unwrap_or(c),
                            want: plan.want,
                            pours: plan.pours,
                            room: plan.room,
                            fall: slope,
                        });
                    }
                    None => g.missing.push((at, row, slope)),
                }
                row -= 1;
            }
            if key(row) == PoolKey::Sea {
                break;
            }
        }
        let end = along[course.len() - 1];
        if key(row) != PoolKey::Sea {
            g.pools.push(GraphPool {
                key: key(row),
                reach: (start, end),
                area: (end - start).max(0.0) * width,
                source,
            });
            // Its end: the sea, or the river it joins (its pool there; a lip into it where this
            // one stands higher: the junction's step, found by the lips' band as any step).
            match r.end {
                RiverEnd::Joins(j) => {
                    let last = course[course.len() - 1];
                    let at = self.unwarp(last.x, last.z);
                    let into = self
                        .pool_near_other(j, at)
                        .unwrap_or(PoolKey::River { river: j, row });
                    if let PoolKey::River { row: jr, .. } = into
                        && jr < row
                    {
                        let reach = (0.5 * last.w).ceil() as i64 + 8;
                        match self.band_near(river, row, at, reach) {
                            Some(c) => {
                                let plan = self.lip_plan(c.0, c.1);
                                g.lips.push(GraphLip {
                                    from: key(row),
                                    to: into,
                                    at: plan.crest.iter().min().copied().unwrap_or(c),
                                    want: plan.want,
                                    pours: plan.pours,
                                    room: plan.room,
                                    fall: 0.0,
                                });
                            }
                            None => g.missing.push((end, row, 0.0)),
                        }
                    }
                }
                RiverEnd::Sea | RiverEnd::Lake(_) => {}
            }
        }
        g
    }

    /// River `other`'s pool nearest face-local column `at` (where a river joins it).
    fn pool_near_other(&self, other: u32, (x, z): (i64, i64)) -> Option<PoolKey> {
        for r in 0..=16i64 {
            for dx in -r..=r {
                for dz in -r..=r {
                    if dx.abs().max(dz.abs()) != r {
                        continue;
                    }
                    if let Some(l) = self.channel_col(x + dx, z + dz)
                        && l.river == other
                    {
                        return Some(PoolKey::River {
                            river: other,
                            row: l.row,
                        });
                    }
                }
            }
        }
        None
    }
}

impl TerrainGenerator {
    /// A map of the lips around (x, z) for diagnosis: each column's row (relative to `row0`),
    /// `b` band, `c` crest of its lip, `.` bank.
    #[doc(hidden)]
    pub fn debug_lips(&self, x: i64, z: i64, r: i64, row0: i32) -> String {
        let mut out = String::new();
        for dz in -r..=r {
            for dx in -r..=r {
                let (cx, cz) = (x + dx, z + dz);
                let s = match self.lip_col(cx, cz) {
                    None => "  .".to_string(),
                    Some(l) => {
                        let tag = if l.band {
                            if self.lip_plan(cx, cz).crest.contains(&(cx, cz)) {
                                "c"
                            } else {
                                "b"
                            }
                        } else {
                            " "
                        };
                        format!("{:>2}{tag}", l.row - row0)
                    }
                };
                out.push_str(&s);
            }
            out.push('\n');
        }
        out
    }
}
