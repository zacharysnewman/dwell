//! The flood light at mesh time (docs/DAY_NIGHT.md §10): each vertex of a cell face takes the
//! average sky and block light of the cells it touches in the layer the face looks into (smooth
//! lighting: the front cell, the two beside it and the corner, as the corner occlusion reads
//! them), leaving out cells that block light and hold none, and the corner behind two such sides.
//! Shaped faces and plants take the cell they look into. The light comes with the mesh job
//! (`dwell_world::light::LightMap::padded`); a job without it is lit as open sky.

use dwell_world::light::{OPEN, block_of, sky_of};

use crate::mesher::{PAD, PADDED_VOLUME, padded_index};
use crate::sky::Occupancy;

/// A corner's light is kept in quarter levels (an average of up to four cells): 0–60.
pub(crate) const QUARTERS: f32 = 60.0;

/// The packed light of the padded block, face-local (a face-B chunk's rows turned over).
pub(crate) struct Flood {
    cells: Option<Vec<u8>>,
}

impl Flood {
    pub(crate) fn new(light: &[u8], mirror: bool) -> Self {
        if light.len() < PADDED_VOLUME {
            return Self { cells: None };
        }
        if !mirror {
            return Self {
                cells: Some(light[..PADDED_VOLUME].to_vec()),
            };
        }
        let layer = PAD * PAD;
        let mut out = vec![OPEN; PADDED_VOLUME];
        for y in 0..PAD {
            let (from, to) = (y * layer, (PAD - 1 - y) * layer);
            out[to..to + layer].copy_from_slice(&light[from..from + layer]);
        }
        Self { cells: Some(out) }
    }

    /// The packed light of chunk-local cell `c` (−1..=32 per axis).
    fn packed(&self, c: [i32; 3]) -> u8 {
        match &self.cells {
            Some(v) if c.iter().all(|&a| (-1..=crate::PAD as i32 - 2).contains(&a)) => {
                v[padded_index(c[0], c[1], c[2])]
            }
            _ => OPEN,
        }
    }

    /// Cell `c`'s sky and block level as shares of 15.
    pub(crate) fn cell(&self, c: [i32; 3]) -> [f32; 2] {
        let p = self.packed(c);
        [f32::from(sky_of(p)) / 15.0, f32::from(block_of(p)) / 15.0]
    }

    /// The light at a shaped face's point `p` (in the cell, 0–1) with cell-space normal `n`: the
    /// cell half a cell out along the normal.
    pub(crate) fn at_point(&self, cell: [i32; 3], p: [f64; 3], n: [f64; 3]) -> [f32; 2] {
        let c: [i32; 3] = std::array::from_fn(|a| cell[a] + (p[a] + 0.5 * n[a]).floor() as i32);
        self.cell(c)
    }

    /// The sky and block light, in quarter levels, of the corner toward `du` along `u` and `dv`
    /// along `v` of a face whose front cell is `front` (`Occupancy::corner_ao`'s cells).
    pub(crate) fn corner(
        &self,
        occ: &Occupancy,
        front: [i32; 3],
        u: usize,
        du: i32,
        v: usize,
        dv: i32,
    ) -> (u8, u8) {
        if self.cells.is_none() {
            return (QUARTERS as u8, 0);
        }
        let at = |a: i32, b: i32| {
            let mut c = front;
            c[u] += a;
            c[v] += b;
            c
        };
        let (s1, s2, corner) = (at(du, 0), at(0, dv), at(du, dv));
        // A cell counts unless it blocks light and holds none (a solid's inside).
        let counts = |c: [i32; 3]| !occ.solid(c) || self.packed(c) != 0;
        let (a, b) = (counts(s1), counts(s2));
        let c = (a || b) && counts(corner);
        let cells = [(front, true), (s1, a), (s2, b), (corner, c)];
        let n = cells.iter().filter(|(_, on)| *on).count() as u32;
        let (mut sky, mut block) = (0u32, 0u32);
        for (c, _) in cells.into_iter().filter(|(_, on)| *on) {
            let p = self.packed(c);
            sky += u32::from(sky_of(p));
            block += u32::from(block_of(p));
        }
        let q = |sum: u32| ((sum * 4 + n / 2) / n) as u8;
        (q(sky), q(block))
    }
}
