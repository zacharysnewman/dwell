//! A river's rows as its course gives them (Phase 52 G2, ADR 0059): which surface rows are a
//! chute's, steps too close for a pool between them. Kept from the pool graph (removed with the
//! basins in Phase 53, ADR 0060) for the terrain's chutes.

use super::*;

/// A river surface falling faster than this (m per m: a row every 2.5 m) is a chute (G2): its
/// steps come too close for pools between them, so it has none: its bed is dry but for a stream
/// running down it from the lip of the pool above to the pool at its foot. A pool is a chute's
/// when its reach along the river is shorter than a row over this (`RiverRows`).
pub const STEEP_FALL: f64 = 0.2;

/// A river's pools by row as its course alone gives them (no terrain): which rows are a chute's
/// (`STEEP_FALL`). The terrain and the graph both read it, so they agree.
#[derive(Debug, Default)]
pub struct RiverRows {
    pub steep: dwell_world::hash::HashSet<i32>,
}

/// The flow (m³/s) one lip cell passes: water's weir at a full row's head (`lip_cells`).
pub fn lip_cell_flow() -> f64 {
    dwell_world::flow::sill_flow(dwell_world::flow::water_weir(), 1.0, CELL_H)
}

impl TerrainGenerator {
    /// River `river`'s rows (`RiverRows`), made once.
    pub(super) fn river_rows(&self, river: u32) -> Arc<RiverRows> {
        if let Some(r) = self.rows.read().expect("rows").get(&river) {
            return r.clone();
        }
        let r = Arc::new(self.build_river_rows(river));
        self.rows
            .write()
            .expect("rows")
            .entry(river)
            .or_insert(r)
            .clone()
    }

    fn build_river_rows(&self, river: u32) -> RiverRows {
        let mut out = RiverRows::default();
        let Some(course) = self.map.channels.courses.get(river as usize) else {
            return out;
        };
        if course.len() < 2 {
            return out;
        }
        let row_of = |s: f64| surface_row((SEA_LEVEL_M + s) as f32);
        let mut along = 0.0;
        let mut start: Option<f64> = None;
        let mut row = row_of(course[0].s);
        let chute = CELL_H / STEEP_FALL;
        for i in 1..course.len() {
            let (a, b) = (course[i - 1], course[i]);
            let len = (b.x - a.x).hypot(b.z - a.z);
            let kb = row_of(b.s);
            while row > kb {
                let edge = (f64::from(row) - 0.5) * CELL_H - SEA_LEVEL_M;
                let t = if (a.s - b.s).abs() > 1e-9 {
                    ((a.s - edge) / (a.s - b.s)).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                let at = along + len * t;
                // The pool at `row` (not the head's) ended here: a chute's if shorter than a row.
                if let Some(s0) = start
                    && at - s0 < chute
                {
                    out.steep.insert(row);
                }
                start = Some(at);
                row -= 1;
            }
            along += len;
        }
        out
    }
}
