//! The minimap and the explored grid (docs/adr/0068-minimap-and-explored-map.md; tunables in
//! `shared/ui/map.json` → `map.gen.rs`).
//!
//! - **The tops** ([`scan_tops`]): a face-local, axis-aligned grid of `texelM` columns round the
//!   player, each its topmost cell's colour and height within `scanUpM` above and `scanDownM`
//!   below the eye (water darkened by its depth), read from the client's streamed voxels: what the
//!   player's own face shows (face B's ground on face B, through [`FaceFrame`]). Read again when
//!   the player moves `moveTexels` (keeping the columns it had, reading the strip it uncovers) and
//!   every `refreshSeconds` (every column, from its last top: edits).
//! - **The disc** ([`MapFrame`], [`render_disc`]): the tops sampled into a round RGBA image turned
//!   so the player's heading is up and their right is right (on face B too, where the mirror makes
//!   the face-local frame's left the player's right), its colours tinted by height when the setting is on. Drawn
//!   again only when the player moves `moveTexels` or turns `turnDegrees`.
//! - **Explored** ([`Explored`]): `cellM` cells whose centres have been inside the disc, per face,
//!   each with its average colour; [`Explored::mark`] gives the new or changed ones for the shell
//!   to keep (per world, in the browser) and draw on the explored map.

use dwell_blocks::StateId;
use dwell_world::{CHUNK_SIZE, Chunk, ChunkCoord, FaceFrame, M, VoxelWorld, chunk_of, row_of_m};
use std::collections::{HashMap, HashSet};

/// The map's tunables (`shared/ui/map.json`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapTunables {
    pub minimap_radius_m: f64,
    pub minimap_texel_m: f64,
    pub minimap_size_px: f64,
    pub minimap_compact_size_px: f64,
    pub minimap_compact_below_px: f64,
    pub minimap_move_texels: f64,
    pub minimap_turn_degrees: f64,
    pub minimap_refresh_seconds: f64,
    pub minimap_scan_up_m: f64,
    pub minimap_scan_down_m: f64,
    pub minimap_player_color: u32,
    pub minimap_other_color: u32,
    pub minimap_dot_px: f64,
    pub minimap_edge_color: u32,
    pub minimap_cpu_update_ms: f64,
    pub height_shading_default: bool,
    pub height_shading_water_depth_m: f64,
    pub height_tint_low_m: f64,
    pub height_tint_high_m: f64,
    pub height_tint_darkest: f64,
    pub height_tint_brightest: f64,
    pub explored_cell_m: f64,
    pub explored_pending_color: u32,
    /// The zoom levels, closest first: (cell side m, span across the panel m).
    pub big_map_levels: &'static [(f64, f64)],
    pub big_map_start_level: f64,
    pub big_map_wheel_step_px: f64,
    pub big_map_pinch_step: f64,
    pub big_map_fog_color: u32,
    pub big_map_panel_fraction: f64,
    pub big_map_phone_margin_px: f64,
    pub big_map_button_px: f64,
    pub big_map_phone_button_px: f64,
    pub big_map_scale_bar_max_px: f64,
    pub big_map_frame_color: u32,
    pub big_map_pad_pan_px_per_s: f64,
    pub big_map_pad_zoom_in: &'static str,
    pub big_map_pad_zoom_out: &'static str,
    pub big_map_pad_zoom_in_alt: &'static str,
    pub big_map_pad_zoom_out_alt: &'static str,
    pub big_map_pad_center: &'static str,
    pub big_map_pad_close: &'static str,
    pub revealed_tile_cells: f64,
    pub revealed_jobs_in_flight: f64,
    pub revealed_background_jobs: f64,
    pub revealed_prebuild_spans: f64,
    pub revealed_budget_ms: f64,
    pub revealed_redraw_ms: f64,
    pub revealed_live_max_cell_m: f64,
    pub revealed_live_tiles_per_frame: f64,
    pub revealed_live_refresh_ms: f64,
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

/// The most rows a column whose ground is over the scan's top is followed up to its surface.
const CLIMB_ROWS: i32 = 256;

/// How far over a column's last top (rows) its next read starts.
const HINT_ROWS: i32 = 8;

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
    /// Whether the top is a liquid's (its colour darkened by its depth).
    pub water: Vec<bool>,
    /// A liquid top's depth (m) down to its bed, as far as `heightShading.waterDepthM` (0 on land).
    pub depth: Vec<f32>,
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
    prev: Option<&TopGrid>,
    reuse: bool,
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
    let mut water = vec![false; n * n];
    let mut depth_m = vec![0f32; n * n];
    for k in 0..n {
        for i in 0..n {
            let (px, pz) = (x0 + (i as f64 + 0.5) * texel, z0 + (k as f64 + 0.5) * texel);
            let last = prev.and_then(|p| p.index(px, pz).map(|j| (p, j)));
            if reuse && let Some((p, j)) = last {
                // A move keeps the columns the last read covered: only the strip it uncovers is
                // read (the browser's whole read was ~5 ms; edits come with the refresh).
                colour[k * n + i] = p.colour[j];
                height[k * n + i] = p.height[j];
                water[k * n + i] = p.water.get(j).copied().unwrap_or(false);
                depth_m[k * n + i] = p.depth.get(j).copied().unwrap_or(0.0);
                continue;
            }
            let cx = (x0 + (i as f64 + 0.5) * texel).floor() as i32;
            let cz = (z0 + (k as f64 + 0.5) * texel).floor() as i32;
            // A column the last read found starts a little over its top then (`HINT_ROWS`), not
            // at the scan's top: most columns barely change between reads a texel apart (in the
            // browser the whole scan from the top cost ~6 ms a read). Ground that rose past it
            // is followed up below, as a hill over the scan's top is.
            let start = last
                .map(|(p, j)| p.height[j])
                .filter(|h| h.is_finite())
                .map_or(top_row, |h| {
                    (row_of_m(f64::from(h)) + HINT_ROWS).clamp(bottom_row, top_row)
                });
            if let Some(top) = column_top(
                world, f, cx, cz, start, bottom_row, water_rows, colour_of, false,
            ) {
                colour[k * n + i] = top.colour;
                height[k * n + i] = top.height;
                water[k * n + i] = top.water;
                depth_m[k * n + i] = top.depth;
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
        water,
        depth: depth_m,
        eye_y,
    }
}

/// A column's top as the maps read it.
struct Top {
    colour: u32,
    height: f32,
    water: bool,
    depth: f32,
}

/// The top of face-local column (`cx`, `cz`) on face `f`, read down from row `start` (ground at
/// `start` is followed up to its surface) to `bottom_row`; a liquid's colour darkened by its depth
/// to `water_rows`. None: no top loaded there.
#[allow(clippy::too_many_arguments)]
fn column_top(
    world: &VoxelWorld,
    f: FaceFrame,
    cx: i32,
    cz: i32,
    start: i32,
    bottom_row: i32,
    water_rows: i32,
    colour_of: &dyn Fn(StateId) -> Option<u32>,
    strict: bool,
) -> Option<Top> {
    // Strict (the big map's tiles, read far from the player): a top whose cell above starts a
    // chunk not held is unknown, not the top (that chunk may be ground still streaming: a hill's
    // stone showed under its unloaded grass in 0.50.76's browser check).
    let unsure = |row: i32| {
        strict && {
            let a = f.cell([cx, row, cz]);
            let b = f.cell([cx, row + 1, cz]);
            let cb = chunk_of(b[0], b[1], b[2]);
            chunk_of(a[0], a[1], a[2]) != cb && world.find(cb).is_none()
        }
    };
    let mut row = start;
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
        if m != M.air && row == start {
            // Ground over the scan's top (a hill above the eye): up to its surface, not
            // its inside (the 0.50.37 browser check showed hills as their stone).
            let mut up = row;
            let mut top = m;
            while up < start + CLIMB_ROWS {
                let [wx, wy, wz] = f.cell([cx, up + 1, cz]);
                let above = world.get_voxel_if_loaded([wx, wy, wz]);
                if above == M.air || colour_of(above).is_none() {
                    break;
                }
                up += 1;
                top = above;
            }
            if unsure(up) {
                return None;
            }
            if let Some(c) = colour_of(top) {
                return Some(Top {
                    colour: c,
                    height: dwell_world::m_of_row(up + 1) as f32,
                    water: false,
                    depth: 0.0,
                });
            }
            return None;
        }
        if m != M.air
            && let Some(c) = colour_of(m)
        {
            if unsure(row) {
                return None;
            }
            let info = dwell_blocks::info(m);
            let mut c = c;
            let mut wet = None;
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
                wet = Some((f64::from(depth) * dwell_world::CELL_H) as f32);
            }
            return Some(Top {
                colour: c,
                height: dwell_world::m_of_row(row + 1) as f32,
                water: wet.is_some(),
                depth: wet.unwrap_or(0.0),
            });
        }
        row -= 1;
    }
    None
}

/// The big map's closest levels from the loaded terrain (ADR 0068; owner, 2026-11-09: "big map
/// closest zooms should use 1m"): `n`² cells of `cell_m` m from face-local cell (`x0`, `z0`) on
/// face `f`, each its column at the cell's centre read as the minimap reads it (scanned from
/// `minimap.scanUpM` over `eye_y` to `scanDownM` under it), as a tile's RGBA rows of x from its
/// least z (alpha 0 where nothing is loaded: the generator's tile shows there) and then its i16 LE
/// [`height_code`]s (`i16::MIN` none; water's its bed's). None when no cell has a loaded top.
#[allow(clippy::too_many_arguments)]
pub fn scan_tile(
    world: &VoxelWorld,
    f: FaceFrame,
    x0: i32,
    z0: i32,
    n: usize,
    cell_m: i32,
    eye_y: f64,
    t: &MapTunables,
    colour_of: &dyn Fn(StateId) -> Option<u32>,
) -> Option<Vec<u8>> {
    let top_row = row_of_m(eye_y + t.minimap_scan_up_m);
    let bottom_row = row_of_m(eye_y - t.minimap_scan_down_m);
    let water_rows = row_of_m(t.height_shading_water_depth_m).max(1);
    let mut out = vec![0u8; n * n * 6];
    let mut any = false;
    for k in 0..n {
        for i in 0..n {
            let j = k * n + i;
            let cx = x0 + i as i32 * cell_m + cell_m / 2;
            let cz = z0 + k as i32 * cell_m + cell_m / 2;
            let code = match column_top(
                world, f, cx, cz, top_row, bottom_row, water_rows, colour_of, true,
            ) {
                Some(top) => {
                    any = true;
                    let c = top.colour;
                    out[j * 4..j * 4 + 4].copy_from_slice(&[
                        (c >> 16) as u8,
                        (c >> 8) as u8,
                        c as u8,
                        255,
                    ]);
                    let h = f64::from(top.height - top.depth);
                    height_code(h, top.water).clamp(i32::from(i16::MIN) + 1, i32::from(i16::MAX))
                        as i16
                }
                None => i16::MIN,
            };
            let o = n * n * 4 + j * 2;
            out[o..o + 2].copy_from_slice(&code.to_le_bytes());
        }
    }
    any.then_some(out)
}

/// `0xRRGGBB` scaled in brightness by `k` (clamped).
pub fn scale_rgb(c: u32, k: f64) -> u32 {
    let ch = |s: u32| ((f64::from((c >> s) & 0xff) * k).round().clamp(0.0, 255.0) as u32) << s;
    ch(16) | ch(8) | ch(0)
}

/// The maps' look (`heightTint`; owner, 2026-11-09: block colours "lightly tinted by height",
/// after the height bands hid the sand): `rgb` scaled in brightness by face-local height `h`'s
/// height above sea level, `darkest` at or below `lowM`, `brightest` at or above `highM`, linear
/// between, so a cell keeps its block's hue. Water (`h` its bed's) keeps its colour: the
/// generator and the minimap's scan already darken it by its depth.
pub fn tint_rgb(t: &MapTunables, rgb: u32, h: f64, water: bool) -> u32 {
    if water {
        return rgb & 0xff_ffff;
    }
    let s = ((h - dwell_world::SEA_LEVEL_M - t.height_tint_low_m)
        / (t.height_tint_high_m - t.height_tint_low_m))
        .clamp(0.0, 1.0);
    let k = t.height_tint_darkest + (t.height_tint_brightest - t.height_tint_darkest) * s;
    scale_rgb(rgb, k)
}

/// No height in a cell (a fogged or pending cell; [`height_code`]).
pub const NO_HEIGHT: i32 = i32::MIN;

/// A cell's height as the shell keeps it: half-metres times two, plus 1 for water (a water cell's
/// height is its bed's).
pub fn height_code(h: f64, water: bool) -> i32 {
    ((h * 2.0).round() as i32) * 2 + i32::from(water)
}

/// A [`height_code`]'s height (m) and whether it is water; None for [`NO_HEIGHT`].
pub fn decode_height(code: i32) -> Option<(f64, bool)> {
    (code != NO_HEIGHT).then(|| (f64::from(code >> 1) / 2.0, code & 1 == 1))
}

/// The big map's cells tinted by height (`shadeMap`, `client/src/ui/minimap.ts`): a `w` × `h`
/// grid in screen order (rows from the top, columns from the left) inside a border of one cell, so
/// `(w + 2) × (h + 2)` colours (`0xRRGGBB`; top byte 0xff: nothing there, drawn transparent) and
/// [`height_code`]s; out, the `w` × `h` cells' RGBA. A cell with a height is its colour
/// [`tint_rgb`]ed by it alone (no neighbours: no lighting); one without keeps its colour. The
/// border is the shell's layout, unread since the hill-shade went (kept for the stair-step shading
/// docs/FUTURE.md names).
pub fn shade_grid(t: &MapTunables, colours: &[u32], codes: &[i32], w: usize, h: usize) -> Vec<u8> {
    let mut out = vec![0u8; w * h * 4];
    let stride = w + 2;
    if colours.len() < stride * (h + 2) || codes.len() < stride * (h + 2) {
        return out;
    }
    for r in 0..h {
        for c in 0..w {
            let j = (r + 1) * stride + c + 1;
            let col = colours[j];
            if col >> 24 == 0xff {
                continue;
            }
            let rgb = decode_height(codes[j])
                .map_or(col & 0xff_ffff, |(hm, wet)| tint_rgb(t, col, hm, wet));
            let o = (r * w + c) * 4;
            out[o] = (rgb >> 16) as u8;
            out[o + 1] = (rgb >> 8) as u8;
            out[o + 2] = rgb as u8;
            out[o + 3] = 255;
        }
    }
    out
}

/// The disc: `px`² RGBA pixels covering the minimap's radius, turned by `frame`, its blocks'
/// colours tinted by height ([`tint_rgb`], the big map's look) when `shading`, else plain; transparent outside the circle.
/// Where the tops found nothing (ground past the scan's reach below, a valley seen from a hilltop
/// or high flight, or not loaded) a pixel takes `under`'s colour and [`height_code`] for its
/// face-local (x, z) (the generator's 1 m top, [`Generated`]),
/// tinted by that height when `shading` and it has one; else it is transparent.
#[allow(clippy::too_many_arguments)]
pub fn render_disc(
    grid: &TopGrid,
    frame: &MapFrame,
    t: &MapTunables,
    px: usize,
    shading: bool,
    under: &dyn Fn(f64, f64) -> Option<(u32, i32)>,
    out: &mut Vec<u8>,
) {
    out.clear();
    out.resize(px * px * 4, 0);
    let r = t.minimap_radius_m;
    let per_px = 2.0 * r / px as f64;
    let half = px as f64 / 2.0;
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
            let c = if c == NO_TOP {
                let Some((c, code)) = under(lx, lz) else {
                    continue;
                };
                match decode_height(code) {
                    Some((h, wet)) if shading => tint_rgb(t, c, h, wet),
                    _ => c,
                }
            } else if shading && grid.height[i].is_finite() {
                let wet = grid.water.get(i).copied().unwrap_or(false);
                let bed = f64::from(grid.height[i])
                    - f64::from(grid.depth.get(i).copied().unwrap_or(0.0));
                tint_rgb(t, c, bed, wet)
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
    /// The generator's 1 m tops under the disc, where the loaded terrain shows nothing.
    pub generated: Generated,
    /// When every column was last read whole.
    full_at: f64,
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
            // The refresh (or a new face) reads every column again, for edits.
            let full = self.grid_at.is_none_or(|(_, when, face)| {
                face != f || now_s - when >= t.minimap_refresh_seconds
            });
            // A move keeps what the last read covered (same face, same eye height: else
            // `rescan` read whole); the refresh reads every column from its last top.
            let same = self.grid_at.is_some_and(|(_, _, face)| face == f);
            let hint = same.then_some(&self.grid);
            self.grid = scan_tops(world, f, local, t, colour_of, hint, !full);
            if full {
                self.full_at = now_s;
            }
            self.grid_at = Some((local, if full { now_s } else { self.full_at }, f));
            self.explored.mark(f, &self.grid, [local[0], local[2]], t);
            up.scanned = true;
        }
        let redraw = up.scanned
            || match self.drawn_at {
                None => true,
                Some((at, yaw, shaded)) => {
                    self.generated.changed
                        || shaded != shading
                        || moved(at, [local[0], local[2]]) >= step
                        || angle_between(yaw, yaw_deg) >= t.minimap_turn_degrees
                }
            };
        if redraw {
            let frame = MapFrame::new(f, local[0], local[2], yaw_deg);
            let face = u8::from(f.is_b());
            // Never coarser than the disc's 1 m (owner, 2026-11-10: "It shouldn't ever be
            // coarse"): where the tops found nothing, the generator's 1 m top, not an explored cell.
            let generated = &self.generated;
            render_disc(
                &self.grid,
                &frame,
                t,
                Self::pixels(t),
                shading,
                &|x, z| generated.at(face, x, z, t),
                &mut self.image,
            );
            self.generated.changed = false;
            self.drawn_at = Some(([local[0], local[2]], yaw_deg, shading));
            up.drawn = true;
        }
        up
    }
}

/// The generator's tops at 1 m under the minimap (ADR 0068; owner, 2026-11-10: the minimap
/// "should always show the full width and height of what's around you" and "shouldn't ever be
/// coarse"): the big map's closest level's tiles (`bigMap.levels[0]`, 1 m cells, generated in
/// the job workers and kept in the browser's tile store), handed over by the shell
/// ([`Generated::fill`]) as [`Generated::wants`] names them: those within the disc's radius and a
/// tile's margin of the player, so a sprint finds its next ones already there. Where the loaded
/// terrain's tops found nothing (not loaded yet, only the LOD drawn, or past the scan's reach) the
/// disc shows these, never an 8 m explored cell.
#[derive(Clone, Debug, Default)]
pub struct Generated {
    tiles: HashMap<TileKey, Vec<u8>>,
    /// A tile came since the disc was drawn.
    pub changed: bool,
}

impl Generated {
    /// The cell side of the tiles (the big map's closest level's).
    pub fn cell_m(t: &MapTunables) -> f64 {
        t.big_map_levels.first().map_or(1.0, |l| l.0)
    }

    fn span(t: &MapTunables) -> f64 {
        t.revealed_tile_cells * Self::cell_m(t)
    }

    /// The tiles within the disc's radius and a tile of face `face`'s face-local (x, z) not yet
    /// held, the nearest first; tiles past three tiles of the radius (or on the other face) are
    /// let go.
    pub fn wants(&mut self, face: u8, x: f64, z: f64, t: &MapTunables) -> Vec<TileKey> {
        let span = Self::span(t);
        let reach = t.minimap_radius_m + span;
        let d2 = |ti: i32, tk: i32| {
            // The tile's nearest point to (x, z).
            let nx = x.clamp(f64::from(ti) * span, f64::from(ti + 1) * span) - x;
            let nz = z.clamp(f64::from(tk) * span, f64::from(tk + 1) * span) - z;
            nx * nx + nz * nz
        };
        let keep = reach + 2.0 * span;
        self.tiles
            .retain(|&(f, ti, tk), _| f == face && d2(ti, tk) <= keep * keep);
        let (i0, i1) = (
            ((x - reach) / span).floor() as i32,
            ((x + reach) / span).floor() as i32,
        );
        let (k0, k1) = (
            ((z - reach) / span).floor() as i32,
            ((z + reach) / span).floor() as i32,
        );
        let mut out = Vec::new();
        for tk in k0..=k1 {
            for ti in i0..=i1 {
                if d2(ti, tk) <= reach * reach && !self.tiles.contains_key(&(face, ti, tk)) {
                    out.push((face, ti, tk));
                }
            }
        }
        out.sort_by(|a, b| d2(a.1, a.2).total_cmp(&d2(b.1, b.2)));
        out
    }

    /// Tile `key`'s bytes: its RGBA, then its i16 LE height codes ([`tile_cell`]'s layout).
    pub fn fill(&mut self, key: TileKey, bytes: Vec<u8>) {
        self.tiles.insert(key, bytes);
        self.changed = true;
    }

    /// Whether tile `key` is held.
    pub fn has(&self, key: TileKey) -> bool {
        self.tiles.contains_key(&key)
    }

    /// The generator's top at face `face`'s face-local (x, z): its colour and [`height_code`].
    pub fn at(&self, face: u8, x: f64, z: f64, t: &MapTunables) -> Option<(u32, i32)> {
        let cell = Self::cell_m(t);
        let n = t.revealed_tile_cells as i32;
        let (i, k) = ((x / cell).floor() as i32, (z / cell).floor() as i32);
        let key = (face, i.div_euclid(n), k.div_euclid(n));
        let j = (k.rem_euclid(n) * n + i.rem_euclid(n)) as usize;
        tile_cell(self.tiles.get(&key)?, (n * n) as usize, j)
    }
}

/// The smallest angle between two headings (degrees).
pub fn angle_between(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(360.0);
    d.min(360.0 - d)
}

/// An explored cell: its face (0 A, 1 B) and its index along face-local x and z.
pub type CellKey = (u8, i32, i32);

/// A tile of generated explored-cell colours: its face (0 A, 1 B) and its index along face-local
/// x and z, `revealed.tileCells` cells a side (`dwell-client` `map_reveal`).
pub type TileKey = (u8, i32, i32);

/// Where an explored cell's colour came from, worst first: a better source replaces a worse one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    /// Nothing yet: `explored.pendingColor`.
    Pending,
    /// The terrain generator's column top at the cell's centre (a generated tile).
    Generated,
    /// The loaded terrain's tops under the disc, averaged.
    Terrain,
}

/// The explored cells this session has seen, each with the colour last reported and its source.
/// A cell is explored once its centre is within the minimap's radius of the player, a 2D distance
/// on their face alone (owner, 2026-11-09: "a simple 2D exploration"), whatever the height between
/// them and whether its terrain is loaded. Its colour is the loaded terrain's where the disc's tops
/// see it, else the generator's (tiles asked for with [`Explored::ask`], generated in the job
/// workers, given with [`Explored::fill`]), else `explored.pendingColor`; reported again as a
/// better source gives it a different colour.
#[derive(Clone, Debug, Default)]
pub struct Explored {
    pub cells: HashMap<CellKey, (u32, Source)>,
    /// Each cell's [`height_code`] from the same source as its colour (none: [`NO_HEIGHT`]).
    pub heights: HashMap<CellKey, i32>,
    /// New or changed since the last [`Explored::take`].
    fresh: Vec<(CellKey, u32)>,
    /// Cells still [`Source::Pending`] whose tile has not come back yet.
    pending: HashSet<CellKey>,
    /// The tiles generated this session (None: no terrain to generate from).
    tiles: HashMap<TileKey, Option<Vec<u8>>>,
    /// The tiles asked for and not yet given.
    asked: HashSet<TileKey>,
}

impl Explored {
    /// Marks the cells whose centres are within the minimap's radius of `centre` (face-local x,
    /// z) on face `f`: each its average top colour in `grid` where the grid has tops in it, else
    /// its generated tile's colour, else pending. A known cell is reported again only when a
    /// source as good or better gives it a visibly different colour.
    pub fn mark(&mut self, f: FaceFrame, grid: &TopGrid, centre: [f64; 2], t: &MapTunables) {
        let cell = t.explored_cell_m;
        let r = t.minimap_radius_m;
        let face = u8::from(f.is_b());
        let tile = t.revealed_tile_cells as i32;
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
                let key = (face, i, k);
                let seen = average(grid, f64::from(i) * cell, f64::from(k) * cell, cell);
                let (c, code, source) = match seen {
                    Some((c, code)) => (c, code, Source::Terrain),
                    None if self.cells.contains_key(&key) => continue,
                    None => match self.generated(key, tile) {
                        Some((c, code)) => (c, code, Source::Generated),
                        None => {
                            if !self.tiles.contains_key(&tile_of(key, tile)) {
                                self.pending.insert(key);
                            }
                            (t.explored_pending_color, NO_HEIGHT, Source::Pending)
                        }
                    },
                };
                self.set(key, c, code, source);
            }
        }
    }

    /// Gives `key` colour `c` from `source`: a worse source than the cell's never replaces it, and
    /// the cell is reported when new or when its colour changes visibly.
    fn set(&mut self, key: CellKey, c: u32, code: i32, source: Source) {
        let old_code = self.heights.get(&key).copied().unwrap_or(NO_HEIGHT);
        // A height a metre or more off (or newly known) is reported as a colour change is.
        let moved = match (decode_height(old_code), decode_height(code)) {
            (Some((a, wa)), Some((b, wb))) => (a - b).abs() >= 1.0 || wa != wb,
            (None, None) => false,
            _ => true,
        };
        match self.cells.get(&key).copied() {
            None => {
                self.cells.insert(key, (c, source));
                self.heights.insert(key, code);
                self.fresh.push((key, c));
            }
            Some((_, was)) if source < was => {}
            Some((old, _)) if rgb_distance(old, c) > 12 || moved => {
                self.cells.insert(key, (c, source));
                self.heights.insert(key, code);
                self.fresh.push((key, c));
            }
            Some((old, _)) => {
                self.cells.insert(key, (old, source));
            }
        }
        if source > Source::Pending {
            self.pending.remove(&key);
        }
    }

    /// The generated colour and [`height_code`] of cell `key`, if its tile has come back with
    /// one there.
    fn generated(&self, key: CellKey, tile: i32) -> Option<(u32, i32)> {
        let tk = tile_of(key, tile);
        let bytes = self.tiles.get(&tk)?.as_ref()?;
        let j = ((key.2 - tk.2 * tile) * tile + (key.1 - tk.1 * tile)) as usize;
        tile_cell(bytes, (tile * tile) as usize, j)
    }

    /// The [`height_code`] the cell was last reported with ([`NO_HEIGHT`]: none).
    pub fn height_of(&self, key: CellKey) -> i32 {
        self.heights.get(&key).copied().unwrap_or(NO_HEIGHT)
    }

    /// The tiles to generate for the pending cells, each asked for once, at most
    /// `revealed.jobsInFlight` outstanding (so chunk streaming keeps its workers).
    pub fn ask(&mut self, t: &MapTunables) -> Vec<TileKey> {
        let tile = t.revealed_tile_cells as i32;
        let room = (t.revealed_jobs_in_flight as usize).saturating_sub(self.asked.len());
        let mut out: Vec<TileKey> = Vec::new();
        for &key in &self.pending {
            if out.len() >= room {
                break;
            }
            let tk = tile_of(key, tile);
            if !self.asked.contains(&tk) && !self.tiles.contains_key(&tk) && !out.contains(&tk) {
                out.push(tk);
            }
        }
        self.asked.extend(out.iter().copied());
        out
    }

    /// A generated tile (`revealed.tileCells`² cells' RGBA, rows of x from its least z; None: no
    /// terrain to generate from): its pending cells take their colours from it.
    pub fn fill(&mut self, key: TileKey, rgba: Option<Vec<u8>>, t: &MapTunables) {
        let tile = t.revealed_tile_cells as i32;
        self.asked.remove(&key);
        self.tiles.insert(key, rgba);
        let inside: Vec<CellKey> = self
            .pending
            .iter()
            .copied()
            .filter(|&c| tile_of(c, tile) == key)
            .collect();
        for c in inside {
            self.pending.remove(&c);
            if let Some((colour, code)) = self.generated(c, tile) {
                self.set(c, colour, code, Source::Generated);
            }
        }
    }

    /// The colour of face `face`'s explored cell holding face-local (x, z), unless still pending.
    pub fn colour_at(&self, face: u8, x: f64, z: f64, t: &MapTunables) -> Option<u32> {
        let cell = t.explored_cell_m;
        let key = (face, (x / cell).floor() as i32, (z / cell).floor() as i32);
        self.cells
            .get(&key)
            .filter(|&&(_, s)| s > Source::Pending)
            .map(|&(c, _)| c)
    }

    /// [`Self::colour_at`] with the cell's [`height_code`].
    pub fn cell_at(&self, face: u8, x: f64, z: f64, t: &MapTunables) -> Option<(u32, i32)> {
        let cell = t.explored_cell_m;
        let key = (face, (x / cell).floor() as i32, (z / cell).floor() as i32);
        self.colour_at(face, x, z, t)
            .map(|c| (c, self.height_of(key)))
    }

    /// The cells new or changed since the last call.
    pub fn take(&mut self) -> Vec<(CellKey, u32)> {
        std::mem::take(&mut self.fresh)
    }
}

/// The tile holding cell `key`, `tile` cells a side.
fn tile_of((face, i, k): CellKey, tile: i32) -> TileKey {
    (face, i.div_euclid(tile), k.div_euclid(tile))
}

/// Cell `j` of a generated tile of `cells` cells (`dwell-client` `map_reveal`): its colour and
/// [`height_code`], from the tile's RGBA and, after it, its i16 LE height codes (absent in a tile
/// without heights: [`NO_HEIGHT`]). None where the tile has no ground (alpha 0).
pub fn tile_cell(bytes: &[u8], cells: usize, j: usize) -> Option<(u32, i32)> {
    let p = bytes.get(j * 4..j * 4 + 4)?;
    if p[3] != 255 {
        return None;
    }
    let c = (u32::from(p[0]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[2]);
    let o = cells * 4 + j * 2;
    let code = bytes
        .get(o..o + 2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .filter(|&v| v != i16::MIN)
        .map_or(NO_HEIGHT, i32::from);
    Some((c, code))
}

/// The average colour of the grid's tops in the square from (x, z), `size` m a side, and their
/// average height's [`height_code`] (water when most of them are; a water top's height its bed's).
fn average(grid: &TopGrid, x: f64, z: f64, size: f64) -> Option<(u32, i32)> {
    if grid.n == 0 {
        return None;
    }
    let mut sum = [0u64; 3];
    let mut count = 0u64;
    let (mut hsum, mut hcount, mut wet) = (0.0, 0u64, 0u64);
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
                if grid.height.get(i).is_some_and(|h| h.is_finite()) {
                    let depth = grid.depth.get(i).copied().unwrap_or(0.0);
                    hsum += f64::from(grid.height[i] - depth);
                    hcount += 1;
                }
                wet += u64::from(grid.water.get(i).copied().unwrap_or(false));
            }
        }
    }
    (count > 0).then(|| {
        let ch = |s: u64| (s / count) as u32;
        let code = if hcount > 0 {
            height_code(hsum / hcount as f64, wet * 2 > count)
        } else {
            NO_HEIGHT
        };
        ((ch(sum[0]) << 16) | (ch(sum[1]) << 8) | ch(sum[2]), code)
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
