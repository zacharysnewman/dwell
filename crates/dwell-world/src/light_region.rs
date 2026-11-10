//! One chunk's flood light computed on its own, from a region of voxels around it (meant for a
//! mesh worker, off the main thread): the same fixed point [`super::LightMap`] keeps, for the
//! chunk and its one-cell apron (the mesher's padded layout).
//!
//! Why a bounded region is exact: every step light takes loses at least a level (`falloff` ≥ 1)
//! except sky light falling straight down at full strength. So a cell's light comes from a
//! source fewer than `LIGHT_MAX` steps away, or from a straight-down full-sky column; a region of
//! [`REGION_APRON`] cells around the chunk holds every such path to the padded cells, and the
//! columns entering it through its top or bottom are given by the caller
//! ([`LightRegion::sky_in`]: whether the held cell just outside is at full sky light). Which
//! chunks are held (light lives only in held chunks; the sky enters at the top of what is held)
//! is a 27-bit mask: the region spans at most the chunk's 26 neighbours.

use super::{Channel, DIRS, LIGHT_MAX, OPEN, StateId, pack, table_rules};
use crate::{CHUNK_SIZE, ChunkCoord, up_of_y};

const N: i32 = CHUNK_SIZE;
/// The cells around the chunk the region holds on every side.
pub const REGION_APRON: i32 = LIGHT_MAX as i32;
/// The region's side in cells.
pub const REGION: i32 = N + 2 * REGION_APRON;
/// Cells in a region.
pub const REGION_VOLUME: usize = (REGION * REGION * REGION) as usize;

/// The input of [`LightRegion::light`].
#[derive(Clone, Debug, PartialEq)]
pub struct LightRegion {
    pub coord: ChunkCoord,
    /// The region's voxels, x fastest, then z, then y, chunk-local −`REGION_APRON` ..
    /// `N + REGION_APRON` (cells of chunks not held are ignored).
    pub voxels: Vec<StateId>,
    /// Which of the 27 chunks around (and including) it are held: bit
    /// `(dx + 1) + 3(dy + 1) + 9(dz + 1)`.
    pub held: u32,
    /// Per region column (x + REGION·z): whether the held cell just under the region's bottom
    /// row (`[0]`) and just over its top row (`[1]`) is at full sky light.
    pub sky_in: [Vec<bool>; 2],
}

#[inline]
const fn ri(x: i32, y: i32, z: i32) -> usize {
    (x + REGION * (z + REGION * y)) as usize
}

/// The held bit of chunk-local cell `l` (in any of the 27 chunks).
#[inline]
fn held_bit(l: [i32; 3]) -> u32 {
    let d = l.map(|v| v.div_euclid(N));
    1 << ((d[0] + 1) + 3 * (d[1] + 1) + 9 * (d[2] + 1))
}

impl LightRegion {
    /// The region of chunk `coord`: `held` says which chunks are held, `voxel(world cell)` gives
    /// each held cell's state, and `full_sky(world cell)` whether a held cell just outside the
    /// region's top or bottom is at full sky light.
    pub fn gather(
        coord: ChunkCoord,
        mut held: impl FnMut(ChunkCoord) -> bool,
        mut voxel: impl FnMut([i32; 3]) -> StateId,
        mut full_sky: impl FnMut([i32; 3]) -> bool,
    ) -> Self {
        let mut mask = 0;
        for b in 0..27 {
            let (dx, dy, dz) = (b % 3 - 1, (b / 3) % 3 - 1, b / 9 - 1);
            if held(ChunkCoord::new(coord.x + dx, coord.y + dy, coord.z + dz)) {
                mask |= 1 << b;
            }
        }
        let o = [coord.x * N, coord.y * N, coord.z * N];
        let a = REGION_APRON;
        let mut voxels = vec![0; REGION_VOLUME];
        for y in 0..REGION {
            for z in 0..REGION {
                for x in 0..REGION {
                    let l = [x - a, y - a, z - a];
                    if mask & held_bit(l) != 0 {
                        voxels[ri(x, y, z)] = voxel([o[0] + l[0], o[1] + l[1], o[2] + l[2]]);
                    }
                }
            }
        }
        let mut sky_in = [Vec::new(), Vec::new()];
        for (k, y) in [-a - 1, N + a].into_iter().enumerate() {
            sky_in[k] = (0..REGION * REGION)
                .map(|i| {
                    let l = [i % REGION - a, y, i / REGION - a];
                    mask & held_bit(l) != 0 && full_sky([o[0] + l[0], o[1] + l[1], o[2] + l[2]])
                })
                .collect();
        }
        Self {
            coord,
            voxels,
            held: mask,
            sky_in,
        }
    }

    /// The chunk's light with a one-cell apron (the mesher's padded layout: x fastest, then z,
    /// then y, each −1..=32), cells of chunks not held [`OPEN`]: equal to
    /// [`super::LightMap::padded`] over the same held chunks.
    pub fn light(&self) -> Vec<u8> {
        let a = REGION_APRON;
        let oy = self.coord.y * N;
        let held = |x: i32, y: i32, z: i32| self.held & held_bit([x - a, y - a, z - a]) != 0;
        let up = |y: i32| up_of_y(oy + y - a);
        let mut table: Vec<(u8, u8)> = Vec::new();
        let mut rules = vec![(0u8, 0u8); REGION_VOLUME];
        let mut is_held = vec![false; REGION_VOLUME];
        for y in 0..REGION {
            for z in 0..REGION {
                for x in 0..REGION {
                    let i = ri(x, y, z);
                    if held(x, y, z) {
                        is_held[i] = true;
                        let id = self.voxels[i];
                        if id != 0 {
                            rules[i] = table_rules(&mut table, id);
                        }
                    }
                }
            }
        }
        // `LightMap::passed`: the light `from_v` gives a cell with filter `f` and face-local up
        // `u` it enters along `DIRS[d]`.
        let passed = |ch: Channel, from_v: u8, d: usize, f: u8, u: i32| -> u8 {
            let vertical = d / 2 == 1;
            if ch == Channel::Sky && vertical && from_v == LIGHT_MAX && f == 0 && DIRS[d][1] == -u {
                return LIGHT_MAX;
            }
            let t = &super::LIGHT;
            let fall = if vertical {
                t.falloff_vertical
            } else {
                t.falloff_across
            };
            from_v.saturating_sub(fall.max(f))
        };
        let mut levels = [vec![0u8; REGION_VOLUME], vec![0u8; REGION_VOLUME]];
        for (k, ch) in [Channel::Sky, Channel::Block].into_iter().enumerate() {
            let level = &mut levels[k];
            let mut buckets: [Vec<u32>; LIGHT_MAX as usize + 1] = Default::default();
            for y in 0..REGION {
                for z in 0..REGION {
                    for x in 0..REGION {
                        let i = ri(x, y, z);
                        if !is_held[i] {
                            continue;
                        }
                        let (f, e) = rules[i];
                        // `LightMap::source`, with the sky entering through the region's top or
                        // bottom where the cell over it there is at full sky light.
                        let s = match ch {
                            Channel::Block => e,
                            Channel::Sky if f >= LIGHT_MAX => 0,
                            Channel::Sky => {
                                let u = up(y);
                                let ay = y + u;
                                let open = !held(x, ay, z)
                                    || (!(0..REGION).contains(&ay)
                                        && self.sky_in[usize::from(ay >= REGION)]
                                            [(x + REGION * z) as usize]);
                                if open {
                                    passed(ch, LIGHT_MAX, if u > 0 { 3 } else { 2 }, f, u)
                                } else {
                                    0
                                }
                            }
                        };
                        if s > 0 {
                            level[i] = s;
                            buckets[usize::from(s)].push(i as u32);
                        }
                    }
                }
            }
            // Brightest first, as `LightMap::run_from`'s addition pass.
            for lv in (1..=LIGHT_MAX).rev() {
                while let Some(i) = buckets[usize::from(lv)].pop() {
                    let i = i as usize;
                    if level[i] != lv {
                        continue;
                    }
                    let c = i as i32;
                    let (x, z, y) = (c % REGION, (c / REGION) % REGION, c / (REGION * REGION));
                    for (d, dir) in DIRS.iter().enumerate() {
                        let (nx, ny, nz) = (x + dir[0], y + dir[1], z + dir[2]);
                        if !(0..REGION).contains(&nx)
                            || !(0..REGION).contains(&ny)
                            || !(0..REGION).contains(&nz)
                        {
                            continue;
                        }
                        let n = ri(nx, ny, nz);
                        if !is_held[n] || level[n] >= lv {
                            continue;
                        }
                        let (f, _) = rules[n];
                        if f >= LIGHT_MAX {
                            continue;
                        }
                        let nv = passed(ch, lv, d, f, up(ny));
                        if nv > level[n] {
                            level[n] = nv;
                            buckets[usize::from(nv)].push(n as u32);
                        }
                    }
                }
            }
        }
        let pad = N + 2;
        let mut out = vec![OPEN; (pad * pad * pad) as usize];
        for py in 0..pad {
            for pz in 0..pad {
                for px in 0..pad {
                    let i = ri(px - 1 + a, py - 1 + a, pz - 1 + a);
                    if is_held[i] {
                        out[(px + pad * (pz + pad * py)) as usize] =
                            pack(levels[0][i], levels[1][i]);
                    }
                }
            }
        }
        out
    }
}
