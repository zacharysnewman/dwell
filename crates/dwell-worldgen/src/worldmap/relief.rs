//! A world map's relief, rivers and lakes in metres (BUGS.md #77, BIG_MAP.md §5): what a bigger
//! world's island must keep of today's — its heights, its ranges per area, rivers as wide as their
//! discharge makes them and no speckle of lakes — measured the same way at every cell size.

use super::{NO_LAKE, WorldMap};

/// Height (m above sea level) over which land counts toward a range.
pub const RANGE_ABOVE_M: f32 = 1000.0;
/// The least area (km²) a range's connected high ground covers.
pub const RANGE_MIN_KM2: f64 = 20.0;
/// A peak: land above `RANGE_ABOVE_M` that is the highest within this many metres (in metres, so
/// a coarse map's merged high ground still counts its summits).
pub const PEAK_RADIUS_M: f64 = 5000.0;

/// One map's measures.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Relief {
    pub land_km2: f64,
    /// The highest land (m above sea level).
    pub max_height: f64,
    /// The share of the land above 200 m and above 1 500 m.
    pub above_200: f64,
    pub above_1500: f64,
    /// Connected high ground (8-neighbour cells above `RANGE_ABOVE_M`) of at least `RANGE_MIN_KM2`.
    pub ranges: u32,
    /// Summits: land above `RANGE_ABOVE_M`, the highest within `PEAK_RADIUS_M`.
    pub peaks: u32,
    /// The widest river vertex and the widest carved channel (m).
    pub river_width: f64,
    pub channel_width: f64,
    /// The discharge (m³/s) where the channel is widest.
    pub channel_q: f64,
    /// The lakes' cells' share of the land, and how many lakes.
    pub lake_share: f64,
    pub lakes: usize,
}

impl Relief {
    pub fn of(m: &WorldMap) -> Self {
        let n = m.n;
        let cell_km2 = m.cell * m.cell / 1e6;
        let land: Vec<bool> = (0..m.height.len()).map(|c| !m.is_sea(c)).collect();
        let count = land.iter().filter(|&&l| l).count() as f64;
        let mut max_height = 0.0f64;
        let (mut a200, mut a1500, mut lake) = (0usize, 0usize, 0usize);
        for (c, &is_land) in land.iter().enumerate() {
            if !is_land {
                continue;
            }
            let h = m.height[c];
            max_height = max_height.max(f64::from(h));
            a200 += usize::from(h > 200.0);
            a1500 += usize::from(h > 1500.0);
            lake += usize::from(m.lake[c] != NO_LAKE);
        }
        let mut seen = vec![false; land.len()];
        let mut ranges = 0;
        let mut stack = Vec::new();
        for start in 0..land.len() {
            if seen[start] || !land[start] || m.height[start] <= RANGE_ABOVE_M {
                continue;
            }
            seen[start] = true;
            stack.push(start);
            let mut cells = 0usize;
            while let Some(c) = stack.pop() {
                cells += 1;
                let (x, z) = ((c % n) as i64, (c / n) as i64);
                for dz in -1..=1 {
                    for dx in -1..=1 {
                        let (nx, nz) = (x + dx, z + dz);
                        if nx < 0 || nz < 0 || nx >= n as i64 || nz >= n as i64 {
                            continue;
                        }
                        let k = nz as usize * n + nx as usize;
                        if !seen[k] && land[k] && m.height[k] > RANGE_ABOVE_M {
                            seen[k] = true;
                            stack.push(k);
                        }
                    }
                }
            }
            if cells as f64 * cell_km2 >= RANGE_MIN_KM2 {
                ranges += 1;
            }
        }
        let r = (PEAK_RADIUS_M / m.cell).ceil() as i64;
        let mut peaks = 0;
        for (c, &is_land) in land.iter().enumerate() {
            let h = m.height[c];
            if !is_land || h <= RANGE_ABOVE_M {
                continue;
            }
            let (x, z) = ((c % n) as i64, (c / n) as i64);
            let mut top = true;
            'scan: for dz in -r..=r {
                for dx in -r..=r {
                    let (nx, nz) = (x + dx, z + dz);
                    if dx * dx + dz * dz > r * r
                        || nx < 0
                        || nz < 0
                        || nx >= n as i64
                        || nz >= n as i64
                    {
                        continue;
                    }
                    let k = nz as usize * n + nx as usize;
                    let o = m.height[k];
                    if o > h || (o == h && k < c) {
                        top = false;
                        break 'scan;
                    }
                }
            }
            peaks += u32::from(top);
        }
        let river_width = m
            .rivers
            .iter()
            .flat_map(|r| r.path.iter())
            .map(|v| f64::from(v.width))
            .fold(0.0, f64::max);
        let (channel_width, channel_q) =
            m.channels
                .courses
                .iter()
                .flatten()
                .fold(
                    (0.0, 0.0),
                    |b: (f64, f64), p| if p.w > b.0 { (p.w, p.q) } else { b },
                );
        Self {
            land_km2: count * cell_km2,
            max_height,
            above_200: a200 as f64 / count.max(1.0),
            above_1500: a1500 as f64 / count.max(1.0),
            ranges,
            peaks,
            river_width,
            channel_width,
            channel_q,
            lake_share: lake as f64 / count.max(1.0),
            lakes: m.lakes.len(),
        }
    }
}
