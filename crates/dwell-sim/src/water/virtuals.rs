//! Virtual pools (docs/proposals/WATER_GRAPH.md §9b, ADR 0059): a generated pool of the world
//! map's pool graph nobody has measured from the blocks. At rest it is implicit: what comes in
//! (its lips in, its river's head) is what its lip passes, by construction. Once something it takes
//! in changes — a measured pool's way out pouring into it, a virtual pool above it off its rest —
//! it holds a volume off its rest (`dv`) over its surface (the graph's estimate), its lip passes
//! by the one law at its level (cells × weir × head^1.5, a row's head at rest), and what it passes
//! feeds the pool below. So a change travels down a river by the law, pool by pool, through
//! country nobody has loaded (replacing the fronts and the seeps). Back at rest it is dropped.
//! Its volume off rest goes into it when it is measured (its drawn level changed in a loaded chunk).

use super::*;
use dwell_world::Face;

/// A virtual pool off its rest.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Virtual {
    /// m³ over (or under) its rest volume.
    pub dv: f64,
    /// Its surface (m²), the graph's estimate.
    pub area: f64,
    /// What its lip passed last tick (m³/s).
    pub q_out: f64,
}

/// How deep under its surface (rows) a cell may lie and be a generated pool's water.
const POOL_DEPTH_ROWS: i32 = 16;

/// The least change a virtual pool keeps: of its level (m), of what comes in (a share of its rest
/// flow), and at least (m³/s).
const VIRTUAL_LEVEL_EPS: f64 = 1e-3;
const VIRTUAL_SHARE: f64 = 0.01;
const VIRTUAL_EPS: f64 = 1e-4;

impl Water {
    /// The generated pool of the graph liquid at cell `c` lies in (on face `f`), if any: the pool
    /// whose water rows hold it.
    pub(super) fn generated_pool(&self, f: FaceFrame, c: [i32; 3]) -> Option<PoolKey> {
        let rivers = self.river_map.as_ref()?;
        // (Its water rows: under its surface, no deeper than a river is.)
        match rivers.pool(f.face, c[0], c[2])? {
            k @ PoolKey::River { row, .. }
                if f.row(c[1]) < row && f.row(c[1]) >= row - POOL_DEPTH_ROWS =>
            {
                Some(k)
            }
            _ => None,
        }
    }

    /// The measured basin holding generated pool `key`, if any.
    pub(super) fn measured(&self, face: Face, key: PoolKey) -> Option<u32> {
        self.basins
            .values()
            .find(|b| b.face.face == face && b.keys.contains(&key))
            .map(|b| b.id)
    }

    /// What pool `key`'s lip passes (m³/s) at its rest level `dv` off rest over `area`: its cells
    /// at a row's head plus that much more (none below its sill).
    fn virtual_out(cells: f64, dv: f64, area: f64) -> f64 {
        let head = (CELL_H + dv / area.max(1.0)).max(0.0);
        cells * dwell_world::flow::sill_flow(dwell_world::flow::water_weir(), 1.0, head)
    }

    /// One water tick of the virtual pools: each off rest (or fed by a measured pool's way out
    /// this tick, `arrived` m³) takes in its lips in (measured pools' by what they poured; virtual
    /// ones' by what they passed last tick; the rest at rest) and its river's head, passes by its
    /// lip (implicitly, so a small pool never overshoots), and what it passes goes on to the pool
    /// below. Returns the virtual pools to measure (their level off rest by `measureM`, the
    /// caller's to do if loaded).
    pub(super) fn step_virtuals(
        &mut self,
        arrived: HashMap<(Face, PoolKey), f64>,
        dt: f64,
    ) -> Vec<(Face, PoolKey)> {
        let Some(rivers) = self.river_map.clone() else {
            return Vec::new();
        };
        let per = dwell_worldgen::terrain::lip_cell_flow();
        // Off rest, poured into, or below a measured pool (whose lip now passes what it pours,
        // nothing included).
        let below_measured: Vec<(Face, PoolKey)> = self
            .basins
            .values()
            .flat_map(|b| {
                let face = b.face.face;
                b.keys
                    .iter()
                    .filter_map(|&k| Some((face, rivers.lip(face, k)?.to)))
                    .collect::<Vec<_>>()
            })
            .filter(|&(face, to)| {
                matches!(to, PoolKey::River { .. }) && self.measured(face, to).is_none()
            })
            .collect();
        let mut keys: Vec<(Face, PoolKey)> = self
            .virtuals
            .keys()
            .copied()
            .chain(arrived.keys().copied())
            .chain(below_measured)
            .collect();
        keys.sort_unstable_by_key(|&(f, k)| (u8::from(f == Face::B), k));
        keys.dedup();
        let last: HashMap<(Face, PoolKey), f64> =
            self.virtuals.iter().map(|(&k, v)| (k, v.q_out)).collect();
        let mut next: HashMap<(Face, PoolKey), Virtual> = HashMap::default();
        let mut downstream: Vec<(Face, PoolKey)> = Vec::new();
        for (face, key) in keys {
            if self.measured(face, key).is_some() {
                continue; // measured since: its volume is the basin's
            }
            let Some(lip) = rivers.lip(face, key) else {
                continue;
            };
            let cells = lip.pours as f64;
            let rest = cells * per;
            let inn = rivers.inflows(face, key);
            let mut q_in = inn.source;
            for (from, _, q) in &inn.lips {
                if self.measured(face, *from).is_some() {
                    continue; // its water came by its ways out (`arrived`)
                }
                q_in += last.get(&(face, *from)).copied().unwrap_or(*q);
            }
            let poured = arrived.get(&(face, key)).copied().unwrap_or(0.0) / dt.max(1e-9);
            q_in += poured;
            let v = self.virtuals.get(&(face, key)).cloned().unwrap_or(Virtual {
                dv: 0.0,
                area: rivers
                    .graph_pool(face, key)
                    .map_or(1.0, |p| p.area.max(1.0)),
                q_out: rest,
            });
            // dv' = dv + dt (q_in − out(dv')): out grows with dv', so bisect.
            let (mut lo, mut hi) = (-v.area * CELL_H, v.dv + dt * q_in);
            for _ in 0..48 {
                let mid = 0.5 * (lo + hi);
                if mid - v.dv - dt * (q_in - Self::virtual_out(cells, mid, v.area)) > 0.0 {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            let dv = 0.5 * (lo + hi);
            let q_out = Self::virtual_out(cells, dv, v.area);
            // Off its rest by a millimetre of level or a hundredth of its flow (a measured pool's
            // faces may pass a hair off its lip's cells at rest: no change worth carrying).
            let moved = (dv / v.area).abs() > VIRTUAL_LEVEL_EPS
                || (q_in - rest).abs() > (VIRTUAL_SHARE * rest).max(VIRTUAL_EPS);
            if moved {
                next.insert((face, key), Virtual { dv, ..v });
                next.get_mut(&(face, key)).expect("virtual").q_out = q_out;
                // What it passes goes on: the pool below takes it in next tick.
                if (q_out - rest).abs() > (VIRTUAL_SHARE * rest).max(VIRTUAL_EPS)
                    && let to @ PoolKey::River { .. } = lip.to
                {
                    downstream.push((face, to));
                }
            }
        }
        // A pool below one now off rest is stepped from the next tick on (at rest until then).
        for k in downstream {
            next.entry(k).or_insert_with(|| {
                let area = rivers.graph_pool(k.0, k.1).map_or(1.0, |p| p.area.max(1.0));
                let rest = rivers.lip(k.0, k.1).map_or(0.0, |l| l.pours as f64 * per);
                Virtual {
                    dv: 0.0,
                    area,
                    q_out: rest,
                }
            });
        }
        self.virtuals = next;
        self.virtuals
            .iter()
            .filter(|(_, v)| (v.dv / v.area).abs() >= FLOW.found_snap_m)
            .map(|(&k, _)| k)
            .collect()
    }

    /// Measures virtual pool `key` from the blocks (its drawn level moved), if its lip's crest is
    /// loaded: found as generated, its volume off rest put into it.
    pub(super) fn measure_virtual(
        &mut self,
        world: &mut VoxelWorld,
        face: Face,
        key: PoolKey,
        writes: &mut Vec<([i32; 3], StateId)>,
    ) {
        let Some(rivers) = self.river_map.clone() else {
            return;
        };
        let (Some(lip), PoolKey::River { row, .. }) = (rivers.lip(face, key), key) else {
            return;
        };
        let f = FaceFrame::of(face);
        let c = f.cell([lip.at.0 as i32, row - 1, lip.at.1 as i32]);
        if world.find(chunk_of(c[0], c[1], c[2])).is_none() || self.owner.contains_key(&c) {
            return;
        }
        let Some(v) = self.virtuals.remove(&(face, key)) else {
            return;
        };
        match self.discover(world, None, c, true) {
            Some(id) => {
                if let Some(b) = self.basins.get_mut(&id) {
                    b.volume = (b.volume + v.dv).max(0.0);
                    b.active = true;
                }
                self.rebuild(world, id, writes);
            }
            None => {
                self.virtuals.insert((face, key), v);
            }
        }
    }
}
