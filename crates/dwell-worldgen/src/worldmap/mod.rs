//! The world map (ADR 0049, docs/WORLD_GENERATION.md §3): one face's hydrology map, kept compact
//! for chunk generation. Computed once per face seed (`hydrology`: the base field sampled on a
//! 100 m grid, mountain ranges lifted over it by the uplift layer, eroded, its lakes filled and
//! its rivers traced), then sampled by every chunk, LOD section and tool of that face.
//!
//! - **What it holds** (`WorldMap`, serialised by `codec`): the eroded height per cell, the sea, the
//!   flow direction (D8, to the cell's receiver: the current's direction for moving water later),
//!   the lakes (their cells and level) and the rivers (polylines with their surface, width, depth
//!   and discharge, and how each ends: the sea, a lake or another river; deltas and estuaries).
//! - **What is derived on load** (not stored): the rivers' smoothed channels with a spatial index
//!   (`channels.rs`) and each lake's signed distance to its shore (`shores.rs`).
//! - **Where it lives** (`cache.rs`): one per face seed per process, computed on first use or
//!   installed from bytes (the browser builds it in a worker and keeps it in IndexedDB; a native
//!   process keeps a copy on disk). Any copy of a seed's map is bit-identical: the computation is
//!   deterministic natively and in WASM (`hydrology`, ADR 0010) and the codec is lossless.
//!
//! Face-local, like everything the generator does: a face's map is its own generator's.

pub mod cache;
pub mod channels;
pub mod codec;
pub mod shores;

use crate::hydrology::{self, End, HydrologyMap, MouthKind, NEIGHBOURS_D8, NONE};

pub use cache::{build_bytes, for_seed, for_settings, install, set_cache_dir};

use crate::settings::MapSettings;
pub use channels::{Channels, Hit};
pub use shores::Shores;

/// Changes whenever the sources or data a map is computed from change (`build.rs`): a cached map
/// is only read by the build that wrote it.
pub const FINGERPRINT: u64 = include!(concat!(env!("OUT_DIR"), "/fingerprint.rs"));

/// The flags' sea bit; the low nibble is the flow direction (an index into `NEIGHBOURS_D8`, or
/// `NO_FLOW`).
pub const SEA: u8 = 0x80;
pub const NO_FLOW: u8 = 0x0f;
/// No lake (`WorldMap::lake`), no river (`MapRiver::joins`), no lake (`MapRiver::from_lake`).
pub const NO_LAKE: u16 = 0;
pub const NO_RIVER: u32 = u32::MAX;

/// The lowest height a cell keeps (m above sea): the sea floor never lies deeper (the core stops
/// it sooner).
pub const FLOOR: f32 = -40.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapLake {
    /// Its flat water surface (m above sea level).
    pub level: f32,
    /// Its cell the outlet drains it from, and the cell it drains to.
    pub entry: u32,
    pub outlet: u32,
}

/// How a river ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RiverEnd {
    Sea,
    Lake(u16),
    /// It joins river `joins`.
    Joins(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mouth {
    None,
    Delta,
    Estuary,
}

/// A vertex of a river (face-local m; `surface` above sea level, m).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapVertex {
    pub x: f32,
    pub z: f32,
    pub surface: f32,
    pub width: f32,
    pub depth: f32,
    pub discharge: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MapRiver {
    /// The lake it flows out of (its index + 1), or `NO_LAKE`.
    pub from_lake: u16,
    pub end: RiverEnd,
    pub mouth: Mouth,
    pub path: Vec<MapVertex>,
}

/// One face's world map.
#[derive(Debug)]
pub struct WorldMap {
    /// The face generator's seed it was computed from.
    pub seed: u64,
    /// The hash of the world settings it was computed with (`WorldSettings::map_hash`; 0: the
    /// data files' own): a slot in its bytes and its cache key.
    pub settings: u64,
    /// `n` × `n` cells of `cell` m; cell (i, j) is centred on face-local (x0 + i·cell, x0 + j·cell).
    pub n: usize,
    pub cell: f64,
    pub x0: f64,
    /// The eroded ground (m above sea level; the sea floor at sea), at least `FLOOR`.
    pub height: Vec<f32>,
    /// `SEA` and the flow direction.
    pub flags: Vec<u8>,
    /// The lake each cell lies in (index + 1), or `NO_LAKE`.
    pub lake: Vec<u16>,
    pub lakes: Vec<MapLake>,
    pub rivers: Vec<MapRiver>,
    /// Derived on load.
    pub channels: Channels,
    pub shores: Shores,
}

impl WorldMap {
    /// The map of face seed `seed` by the default profile (`hydrology.json`), computed now.
    pub fn compute(seed: u64) -> Self {
        Self::compute_with(seed, &MapSettings::default())
    }

    /// The map of face seed `seed` under a world's settings (`settings.rs`), computed now.
    pub fn compute_with(seed: u64, s: &MapSettings) -> Self {
        let mut m = Self::from_hydrology(
            seed,
            &hydrology::generate_seed_with(seed, &s.hydrology, s.island),
        );
        m.settings = s.hash;
        m
    }

    /// The kept map of a hydrology map.
    pub fn from_hydrology(seed: u64, m: &HydrologyMap) -> Self {
        let n = m.grid.n;
        let len = m.grid.len();
        let mut flags = vec![0u8; len];
        let mut lake = vec![NO_LAKE; len];
        for i in 0..len {
            let r = m.receiver[i];
            let dir = if m.sea[i] || r == NONE {
                NO_FLOW
            } else {
                let (dx, dz) = (
                    (r as usize % n) as i32 - (i % n) as i32,
                    (r as usize / n) as i32 - (i / n) as i32,
                );
                NEIGHBOURS_D8
                    .iter()
                    .position(|&(x, z, _)| (x, z) == (dx, dz))
                    .map_or(NO_FLOW, |k| k as u8)
            };
            flags[i] = dir | if m.sea[i] { SEA } else { 0 };
            if m.lake_of[i] != NONE {
                lake[i] = (m.lake_of[i] + 1) as u16;
            }
        }
        // Which river each river cell belongs to: a river ending on another's cell joins it.
        let mut owner = vec![NO_RIVER; len];
        for (k, r) in m.rivers.iter().enumerate() {
            for &c in &r.cells {
                if owner[c as usize] == NO_RIVER {
                    owner[c as usize] = k as u32;
                }
            }
        }
        let cell_of = |x: f64, z: f64| {
            let i = ((x - m.x0) / m.cell).round() as usize;
            let j = ((z - m.x0) / m.cell).round() as usize;
            j * n + i
        };
        let mouth_of = |k: usize| {
            m.mouths
                .iter()
                .find(|mo| mo.river as usize == k)
                .map_or(Mouth::None, |mo| match mo.kind {
                    MouthKind::Delta => Mouth::Delta,
                    MouthKind::Estuary => Mouth::Estuary,
                })
        };
        let rivers = m
            .rivers
            .iter()
            .enumerate()
            .map(|(k, r)| MapRiver {
                from_lake: r.from_lake.map_or(NO_LAKE, |l| (l + 1) as u16),
                end: match r.end {
                    End::Sea => RiverEnd::Sea,
                    End::Lake(l) => RiverEnd::Lake((l + 1) as u16),
                    End::Joins => {
                        let last = r.path[r.path.len() - 1];
                        RiverEnd::Joins(owner[cell_of(last.x, last.z)])
                    }
                },
                mouth: mouth_of(k),
                path: r
                    .path
                    .iter()
                    .map(|v| MapVertex {
                        x: v.x as f32,
                        z: v.z as f32,
                        surface: v.surface as f32,
                        width: v.width as f32,
                        depth: v.depth as f32,
                        discharge: v.discharge as f32,
                    })
                    .collect(),
            })
            .collect();
        Self::assemble(
            seed,
            n,
            m.cell,
            m.x0,
            // Beyond the disc the field has no height: those cells hold the sea floor, so the
            // bicubic near the rim does not dive.
            m.height
                .iter()
                .zip(&m.original)
                .map(|(&h, &o)| {
                    if o < -1000.0 {
                        crate::terrain::HEIGHT.sea_floor
                    } else {
                        h.max(FLOOR)
                    }
                })
                .collect(),
            flags,
            lake,
            m.lakes
                .iter()
                .map(|l| MapLake {
                    level: l.level as f32,
                    entry: l.entry,
                    outlet: l.outlet,
                })
                .collect(),
            rivers,
        )
    }

    /// A map from its stored parts, with what is derived from them.
    #[allow(clippy::too_many_arguments)]
    pub fn assemble(
        seed: u64,
        n: usize,
        cell: f64,
        x0: f64,
        height: Vec<f32>,
        flags: Vec<u8>,
        lake: Vec<u16>,
        lakes: Vec<MapLake>,
        rivers: Vec<MapRiver>,
    ) -> Self {
        let mut map = Self {
            seed,
            settings: 0,
            n,
            cell,
            x0,
            height,
            flags,
            lake,
            lakes,
            rivers,
            channels: Channels::default(),
            shores: Shores::default(),
        };
        map.channels = Channels::build(&map);
        map.shores = Shores::build(&map);
        map
    }

    /// The map's digest: equal digests on two targets mean bit-identical maps (the codec's
    /// checksum over every stored byte but the build's fingerprint, so it is pinned by tests).
    pub fn digest(&self) -> u64 {
        codec::checksum(&codec::encode_unsigned(self))
    }

    /// The cell index of grid coordinates, clamped to the grid.
    fn at(&self, i: i64, j: i64) -> usize {
        let last = self.n as i64 - 1;
        (j.clamp(0, last) as usize) * self.n + i.clamp(0, last) as usize
    }

    /// The cell whose centre is nearest face-local (x, z).
    pub fn cell_at(&self, x: f64, z: f64) -> usize {
        let i = ((x - self.x0) / self.cell + 0.5).floor() as i64;
        let j = ((z - self.x0) / self.cell + 0.5).floor() as i64;
        self.at(i, j)
    }

    /// The centre of cell `c` (face-local m).
    pub fn centre(&self, c: usize) -> (f64, f64) {
        (
            self.x0 + (c % self.n) as f64 * self.cell,
            self.x0 + (c / self.n) as f64 * self.cell,
        )
    }

    pub fn is_sea(&self, c: usize) -> bool {
        self.flags[c] & SEA != 0
    }

    /// Where water in cell `c` flows (a unit step (dx, dz) to its receiver), if anywhere: the
    /// direction for moving water.
    pub fn flow(&self, c: usize) -> Option<(i32, i32)> {
        let d = self.flags[c] & 0x0f;
        NEIGHBOURS_D8
            .get(usize::from(d))
            .map(|&(dx, dz, _)| (dx, dz))
    }

    /// The eroded height (m above sea level) at face-local (x, z), Catmull-Rom bicubic between the
    /// cells' centres (C1: no creases at the cells' edges), with its gradient (m per m).
    pub fn height_at(&self, x: f64, z: f64) -> (f64, f64, f64) {
        let u = (x - self.x0) / self.cell;
        let v = (z - self.x0) / self.cell;
        let (i, j) = (u.floor(), v.floor());
        let (fu, fv) = (u - i, v - j);
        let (i, j) = (i as i64, j as i64);
        let (wu, du) = (cubic(fu), cubic_slope(fu));
        let (wv, dv) = (cubic(fv), cubic_slope(fv));
        let (mut h, mut hx, mut hz) = (0.0, 0.0, 0.0);
        for b in 0..4 {
            let (mut row, mut row_d) = (0.0, 0.0);
            for a in 0..4 {
                let s = f64::from(self.height[self.at(i - 1 + a as i64, j - 1 + b as i64)]);
                row += wu[a] * s;
                row_d += du[a] * s;
            }
            h += wv[b] * row;
            hx += wv[b] * row_d;
            hz += dv[b] * row;
        }
        (h, hx / self.cell, hz / self.cell)
    }
}

/// Catmull-Rom weights of the four samples around a point `f` (0–1) past the second.
fn cubic(f: f64) -> [f64; 4] {
    let f2 = f * f;
    let f3 = f2 * f;
    [
        0.5 * (-f3 + 2.0 * f2 - f),
        0.5 * (3.0 * f3 - 5.0 * f2 + 2.0),
        0.5 * (-3.0 * f3 + 4.0 * f2 + f),
        0.5 * (f3 - f2),
    ]
}

/// The weights' derivatives in `f`.
fn cubic_slope(f: f64) -> [f64; 4] {
    let f2 = f * f;
    [
        0.5 * (-3.0 * f2 + 4.0 * f - 1.0),
        0.5 * (9.0 * f2 - 10.0 * f),
        0.5 * (-9.0 * f2 + 8.0 * f + 1.0),
        0.5 * (3.0 * f2 - 2.0 * f),
    ]
}
