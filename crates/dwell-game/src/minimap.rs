//! The minimap and the explored grid (docs/adr/0068-minimap-and-explored-map.md; tunables in
//! `shared/ui/map.json` → `map.gen.rs`).
//!
//! - **The tops** ([`scan_tops`]): a face-local, axis-aligned grid of `texelM` columns round the
//!   player, each its topmost cell's colour and height within `scanUpM` above and `scanDownM`
//!   below the eye (water darkened by its depth), read from the client's streamed voxels: what the
//!   player's own face shows (face B's ground on face B, through [`FaceFrame`]). Read again when
//!   the player moves `moveTexels` or every `refreshSeconds` (edits).
//! - **The disc** ([`MapFrame`], [`render_disc`]): the tops sampled into a round RGBA image turned
//!   so the player's heading is up and their right is right (on face B too, where the mirror makes
//!   the face-local frame's left the player's right), height-shaded when the setting is on. Drawn
//!   again only when the player moves `moveTexels` or turns `turnDegrees`.
//! - **Explored** ([`Explored`]): `cellM` cells whose centres have been inside the disc, per face,
//!   each with its average colour; [`Explored::mark`] gives the new or changed ones for the shell
//!   to keep (per world, in the browser) and draw on the explored map.

use dwell_blocks::StateId;
use dwell_world::{CHUNK_SIZE, Chunk, ChunkCoord, FaceFrame, M, VoxelWorld, chunk_of, row_of_m};
use std::collections::HashMap;

/// The map's tunables (`shared/ui/map.json`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapTunables {
    pub minimap_radius_m: f64,
    pub minimap_texel_m: f64,
    pub minimap_size_px: f64,
    pub minimap_move_texels: f64,
    pub minimap_turn_degrees: f64,
    pub minimap_refresh_seconds: f64,
    pub minimap_scan_up_m: f64,
    pub minimap_scan_down_m: f64,
    pub minimap_player_color: u32,
    pub minimap_other_color: u32,
    pub minimap_dot_px: f64,
    pub minimap_edge_color: u32,
    pub height_shading_default: bool,
    pub height_shading_range_m: f64,
    pub height_shading_darkest: f64,
    pub height_shading_brightest: f64,
    pub height_shading_slope: f64,
    pub height_shading_water_depth_m: f64,
    pub explored_cell_m: f64,
    pub big_map_start_px_per_cell: f64,
    pub big_map_min_px_per_cell: f64,
    pub big_map_max_px_per_cell: f64,
    pub big_map_zoom_step: f64,
    pub big_map_fog_color: u32,
}

#[rustfmt::skip]
mod table {
    use super::MapTunables;
    include!("map.gen.rs");
}
pub use table::MAP;

/// The player's map frame: where they are and which way is up and right on the map, face-local
/// (x, z; the same as the world's on both faces).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapFrame {
    pub centre: [f64; 2],
    /// The heading (screen up).
    pub forward: [f64; 2],
    /// The player's right (screen right).
    pub right: [f64; 2],
}

impl MapFrame {
    /// The frame of a player on face `f` at face-local `(x, z)` looking at heading `yaw_deg`
    /// (0 = +Z, as [`FaceFrame::look`]). Their right is the heading turned a quarter clockwise
    /// seen from the face's up; the mirror makes it the face-local frame's other way on face B.
    pub fn new(f: FaceFrame, x: f64, z: f64, yaw_deg: f64) -> Self {
        let (s, c) = yaw_deg.to_radians().sin_cos();
        let forward = [s, c];
        let right = [f.handed(-c), f.handed(s)];
        Self {
            centre: [x, z],
            forward,
            right,
        }
    }

    /// A face-local point (x, z) on the map: metres right of and up from the player.
    pub fn to_screen(&self, p: [f64; 2]) -> [f64; 2] {
        let d = [p[0] - self.centre[0], p[1] - self.centre[1]];
        [
            d[0] * self.right[0] + d[1] * self.right[1],
            d[0] * self.forward[0] + d[1] * self.forward[1],
        ]
    }

    /// The face-local point (x, z) at metres right of and up from the player.
    pub fn to_local(&self, s: [f64; 2]) -> [f64; 2] {
        [
            self.centre[0] + self.right[0] * s[0] + self.forward[0] * s[1],
            self.centre[1] + self.right[1] * s[0] + self.forward[1] * s[1],
        ]
    }
}

/// No top found in a column (open air, or not loaded).
pub const NO_TOP: u32 = u32::MAX;

/// The columns' tops round the player: `n`² texels from face-local (`x0`, `z0`) in steps of
/// `texel_m`, each its colour (`0xRRGGBB`, or [`NO_TOP`]) and height (face-local m).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TopGrid {
    pub x0: f64,
    pub z0: f64,
    pub n: usize,
    pub texel_m: f64,
    pub colour: Vec<u32>,
    pub height: Vec<f32>,
    /// The face-local eye height it was read from.
    pub eye_y: f64,
}

impl TopGrid {
    /// The texel holding face-local (x, z), if inside.
    pub fn index(&self, x: f64, z: f64) -> Option<usize> {
        let i = ((x - self.x0) / self.texel_m).floor();
        let k = ((z - self.z0) / self.texel_m).floor();
        let n = self.n as f64;
        (i >= 0.0 && k >= 0.0 && i < n && k < n).then(|| k as usize * self.n + i as usize)
    }
}

/// Reads the tops of the columns within `radius` of face-local `(x, eye_y, z)` on face `f`
/// (every `texel` m), from the top of the scan down; `colour_of` gives a state's top colour, or
/// None for what the map sees through (air). A column whose cells are not loaded has no top.
pub fn scan_tops(
    world: &VoxelWorld,
    f: FaceFrame,
    [x, eye_y, z]: [f64; 3],
    t: &MapTunables,
    colour_of: &dyn Fn(StateId) -> Option<u32>,
) -> TopGrid {
    let texel = t.minimap_texel_m;
    let n = (2.0 * t.minimap_radius_m / texel).ceil() as usize;
    let x0 = ((x - t.minimap_radius_m) / texel).floor() * texel;
    let z0 = ((z - t.minimap_radius_m) / texel).floor() * texel;
    let top_row = row_of_m(eye_y + t.minimap_scan_up_m);
    let bottom_row = row_of_m(eye_y - t.minimap_scan_down_m);
    let water_rows = row_of_m(t.height_shading_water_depth_m).max(1);
    let mut colour = vec![NO_TOP; n * n];
    let mut height = vec![f32::NAN; n * n];
    for k in 0..n {
        for i in 0..n {
            let cx = (x0 + (i as f64 + 0.5) * texel).floor() as i32;
            let cz = (z0 + (k as f64 + 0.5) * texel).floor() as i32;
            let mut row = top_row;
            // The chunk the row is in, looked up once per chunk down the column; a chunk not
            // held (all air, or not streamed) is skipped whole.
            let mut held: Option<(ChunkCoord, Option<&Chunk>)> = None;
            while row >= bottom_row {
                let [wx, wy, wz] = f.cell([cx, row, cz]);
                let coord = chunk_of(wx, wy, wz);
                let chunk = match held {
                    Some((c, ch)) if c == coord => ch,
                    _ => {
                        let ch = world.find(coord);
                        held = Some((coord, ch));
                        ch
                    }
                };
                let Some(chunk) = chunk else {
                    // Down to the next chunk's first row, face-locally.
                    let ly = wy.rem_euclid(CHUNK_SIZE);
                    row -= if f.is_b() { CHUNK_SIZE - ly } else { ly + 1 };
                    continue;
                };
                let m = chunk.get(
                    wx.rem_euclid(CHUNK_SIZE),
                    wy.rem_euclid(CHUNK_SIZE),
                    wz.rem_euclid(CHUNK_SIZE),
                );
                if m != M.air
                    && let Some(c) = colour_of(m)
                {
                    let info = dwell_blocks::info(m);
                    let mut c = c;
                    if info.liquid {
                        // Darkened by the depth under it, to the deepest at `waterDepthM`.
                        let mut depth = 1;
                        while depth < water_rows {
                            let [wx, wy, wz] = f.cell([cx, row - depth, cz]);
                            if !dwell_blocks::info(world.get_voxel_if_loaded([wx, wy, wz])).liquid {
                                break;
                            }
                            depth += 1;
                        }
                        c = scale_rgb(c, 1.0 - 0.55 * f64::from(depth) / f64::from(water_rows));
                    }
                    colour[k * n + i] = c;
                    height[k * n + i] = dwell_world::m_of_row(row + 1) as f32;
                    break;
                }
                row -= 1;
            }
        }
    }
    TopGrid {
        x0,
        z0,
        n,
        texel_m: texel,
        colour,
        height,
        eye_y,
    }
}

/// `0xRRGGBB` scaled in brightness by `k` (clamped).
pub fn scale_rgb(c: u32, k: f64) -> u32 {
    let ch = |s: u32| ((f64::from((c >> s) & 0xff) * k).round().clamp(0.0, 255.0) as u32) << s;
    ch(16) | ch(8) | ch(0)
}

/// The height shading's brightness at a texel of height `h` with neighbours' rise `dx`, `dz`
/// (m per texel), for an eye at `eye_y`.
pub fn shade(t: &MapTunables, h: f64, eye_y: f64, dx: f64, dz: f64) -> f64 {
    let rel = ((h - eye_y) / t.height_shading_range_m).clamp(-1.0, 1.0);
    let hill = -(dx + dz) * t.height_shading_slope;
    (1.0 + 0.3 * rel + hill).clamp(t.height_shading_darkest, t.height_shading_brightest)
}

/// The disc: `px`² RGBA pixels covering the minimap's radius, turned by `frame`, shaded when
/// `shading`; transparent outside the circle and where there is no top.
pub fn render_disc(
    grid: &TopGrid,
    frame: &MapFrame,
    t: &MapTunables,
    px: usize,
    shading: bool,
    out: &mut Vec<u8>,
) {
    out.clear();
    out.resize(px * px * 4, 0);
    let r = t.minimap_radius_m;
    let per_px = 2.0 * r / px as f64;
    let half = px as f64 / 2.0;
    let height_at = |x: f64, z: f64| -> Option<f64> {
        grid.index(x, z)
            .map(|i| f64::from(grid.height[i]))
            .filter(|h| h.is_finite())
    };
    for py in 0..px {
        for qx in 0..px {
            let sx = (qx as f64 + 0.5 - half) * per_px;
            let sy = (half - (py as f64 + 0.5)) * per_px;
            if sx * sx + sy * sy > r * r {
                continue;
            }
            let [lx, lz] = frame.to_local([sx, sy]);
            let Some(i) = grid.index(lx, lz) else {
                continue;
            };
            let c = grid.colour[i];
            if c == NO_TOP {
                continue;
            }
            let c = if shading {
                let h = f64::from(grid.height[i]);
                let tx = grid.texel_m;
                let dx = height_at(lx + tx, lz).map_or(0.0, |n| n - h);
                let dz = height_at(lx, lz + tx).map_or(0.0, |n| n - h);
                scale_rgb(c, shade(t, h, grid.eye_y, dx, dz))
            } else {
                c
            };
            let o = (py * px + qx) * 4;
            out[o] = (c >> 16) as u8;
            out[o + 1] = (c >> 8) as u8;
            out[o + 2] = c as u8;
            out[o + 3] = 255;
        }
    }
}

/// When the disc is drawn again: the player moved `moveTexels` or turned `turnDegrees` since it
/// was, and the tops read again: moved `moveTexels` or `refreshSeconds` passed.
#[derive(Clone, Debug, Default)]
pub struct Minimap {
    pub grid: TopGrid,
    grid_at: Option<([f64; 3], f64, FaceFrame)>,
    drawn_at: Option<([f64; 2], f64, bool)>,
    pub image: Vec<u8>,
    pub explored: Explored,
}

/// What [`Minimap::update`] did this call.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Updated {
    pub scanned: bool,
    pub drawn: bool,
}

impl Minimap {
    /// The disc's size in pixels (a texel each).
    pub fn pixels(t: &MapTunables) -> usize {
        (2.0 * t.minimap_radius_m / t.minimap_texel_m).ceil() as usize
    }

    /// Brings the disc up to date for a player at world `pos` looking at face-local `yaw_deg`.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        world: &VoxelWorld,
        pos: [f64; 3],
        yaw_deg: f64,
        now_s: f64,
        shading: bool,
        t: &MapTunables,
        colour_of: &dyn Fn(StateId) -> Option<u32>,
    ) -> Updated {
        let f = FaceFrame::at_height(pos[1]);
        let local = f.point(pos);
        let step = t.minimap_move_texels * t.minimap_texel_m;
        let moved =
            |a: [f64; 2], b: [f64; 2]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();
        let mut up = Updated::default();
        let rescan = match self.grid_at {
            None => true,
            Some((at, when, face)) => {
                face != f
                    || moved([at[0], at[2]], [local[0], local[2]]) >= step
                    || (at[1] - local[1]).abs() >= 2.0
                    || now_s - when >= t.minimap_refresh_seconds
            }
        };
        if rescan {
            self.grid = scan_tops(world, f, local, t, colour_of);
            self.grid_at = Some((local, now_s, f));
            self.explored.mark(f, &self.grid, [local[0], local[2]], t);
            up.scanned = true;
        }
        let redraw = up.scanned
            || match self.drawn_at {
                None => true,
                Some((at, yaw, shaded)) => {
                    shaded != shading
                        || moved(at, [local[0], local[2]]) >= step
                        || angle_between(yaw, yaw_deg) >= t.minimap_turn_degrees
                }
            };
        if redraw {
            let frame = MapFrame::new(f, local[0], local[2], yaw_deg);
            render_disc(
                &self.grid,
                &frame,
                t,
                Self::pixels(t),
                shading,
                &mut self.image,
            );
            self.drawn_at = Some(([local[0], local[2]], yaw_deg, shading));
            up.drawn = true;
        }
        up
    }
}

/// The smallest angle between two headings (degrees).
pub fn angle_between(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(360.0);
    d.min(360.0 - d)
}

/// An explored cell: its face (0 A, 1 B) and its index along face-local x and z.
pub type CellKey = (u8, i32, i32);

/// The explored cells this session has seen, each with the colour last reported.
#[derive(Clone, Debug, Default)]
pub struct Explored {
    pub cells: HashMap<CellKey, u32>,
    /// New or changed since the last [`Explored::take`].
    fresh: Vec<(CellKey, u32)>,
}

impl Explored {
    /// Marks the cells whose centres are inside the disc round `centre` (face-local x, z) on face
    /// `f`, each its average top colour in `grid`; a cell already known is reported again only
    /// when its colour changed visibly.
    pub fn mark(&mut self, f: FaceFrame, grid: &TopGrid, centre: [f64; 2], t: &MapTunables) {
        let cell = t.explored_cell_m;
        let r = t.minimap_radius_m;
        let face = u8::from(f.is_b());
        let (i0, i1) = (
            ((centre[0] - r) / cell).floor() as i32,
            ((centre[0] + r) / cell).floor() as i32,
        );
        let (k0, k1) = (
            ((centre[1] - r) / cell).floor() as i32,
            ((centre[1] + r) / cell).floor() as i32,
        );
        for k in k0..=k1 {
            for i in i0..=i1 {
                let cx = (f64::from(i) + 0.5) * cell;
                let cz = (f64::from(k) + 0.5) * cell;
                if (cx - centre[0]).powi(2) + (cz - centre[1]).powi(2) > r * r {
                    continue;
                }
                let Some(c) = average(grid, f64::from(i) * cell, f64::from(k) * cell, cell) else {
                    continue;
                };
                let key = (face, i, k);
                let known = self.cells.get(&key).copied();
                if known.is_none_or(|old| rgb_distance(old, c) > 12) {
                    self.cells.insert(key, c);
                    self.fresh.push((key, c));
                }
            }
        }
    }

    /// The cells new or changed since the last call.
    pub fn take(&mut self) -> Vec<(CellKey, u32)> {
        std::mem::take(&mut self.fresh)
    }
}

/// The average colour of the grid's tops in the square from (x, z), `size` m a side.
fn average(grid: &TopGrid, x: f64, z: f64, size: f64) -> Option<u32> {
    let mut sum = [0u64; 3];
    let mut count = 0u64;
    let steps = (size / grid.texel_m).round().max(1.0) as usize;
    for b in 0..steps {
        for a in 0..steps {
            let px = x + (a as f64 + 0.5) * grid.texel_m;
            let pz = z + (b as f64 + 0.5) * grid.texel_m;
            if let Some(i) = grid.index(px, pz)
                && grid.colour[i] != NO_TOP
            {
                let c = grid.colour[i];
                sum[0] += u64::from((c >> 16) & 0xff);
                sum[1] += u64::from((c >> 8) & 0xff);
                sum[2] += u64::from(c & 0xff);
                count += 1;
            }
        }
    }
    (count > 0).then(|| {
        let ch = |s: u64| (s / count) as u32;
        (ch(sum[0]) << 16) | (ch(sum[1]) << 8) | ch(sum[2])
    })
}

fn rgb_distance(a: u32, b: u32) -> u32 {
    [16, 8, 0]
        .iter()
        .map(|&s| ((a >> s) & 0xff).abs_diff((b >> s) & 0xff))
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "minimap_tests.rs"]
mod tests;
