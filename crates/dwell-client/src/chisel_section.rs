//! Chisel's 3D section view and relief shading (ADR 0074, the 3D section, 0.52.19; `client/src/chisel/section.ts`): a
//! square of the real generator's columns (`TerrainGenerator::column_lod`, the detail preview's
//! path) sampled every `texel` m, their heights snapped to the ½ m cells, turned into a blocky
//! stepped mesh (one top quad a column, walls down to lower neighbours, the top colour per
//! column, a baked sun shadow) and a water mesh at each wet column's level; and the 2D views'
//! hill shade from a height grid. Face-local throughout (+y up): Chisel previews face A.

use dwell_worldgen::terrain::{TerrainGenerator, Water};

use crate::lod_map::{srgb_byte, top_rgb};

/// The sun the section and the relief shade light with: from the north-west, 35° up (a map's
/// conventional hill-shade sun), face-local, unit length.
pub const SUN: [f64; 3] = [-0.579_228, 0.573_576, -0.579_228];

/// The relief shade's floor and ceiling (a factor on the texel's colour; flat ground is 1).
pub const RELIEF_MIN: f64 = 0.45;
pub const RELIEF_MAX: f64 = 1.3;

/// A section's columns: `n` × `n` of `texel` m from face-local (x0, z0) (on the texel grid), rows
/// of x from z0.
#[derive(Clone, Debug)]
pub struct Section {
    pub x0: i64,
    pub z0: i64,
    pub n: usize,
    pub texel: i64,
    /// The ground's top (m, face-local), snapped up to the ½ m cell its top voxel fills;
    /// NaN beyond the disc.
    pub height: Vec<f32>,
    /// The water's surface where the column is wet (sea, lake or a river channel), else NaN.
    pub water: Vec<f32>,
    /// The top block's colour (sRGB `0xRRGGBB`).
    pub rgb: Vec<u32>,
    /// The structures standing on it (trees, boulders; ADR 0074 B5) as simple boxes, when the
    /// square is at most [`TREES_UP_TO`] m a side.
    pub trees: Vec<TreeBox>,
}

/// The largest section (m a side) that shows its trees: their placement asks the ground per
/// candidate, about as many columns as a section of 512 a side.
pub const TREES_UP_TO: i64 = 1024;

/// A placed structure drawn as two boxes (face-local metres): its trunk (the cells under its
/// canopy in the anchor's column; empty for a boulder) and its canopy (every cell of its commonest
/// block), each `[min, max]` with its colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TreeBox {
    pub trunk: Option<([f32; 3], [f32; 3], u32)>,
    pub canopy: ([f32; 3], [f32; 3], u32),
}

fn rgb_of(state: dwell_blocks::StateId) -> u32 {
    let c = top_rgb(state).map(srgb_byte);
    (u32::from(c[0]) << 16) | (u32::from(c[1]) << 8) | u32::from(c[2])
}

/// The section's structures as boxes (`TerrainGenerator::trees_sparse` over its square).
fn tree_boxes(g: &TerrainGenerator, x0: i64, z0: i64, side: i64) -> Vec<TreeBox> {
    use dwell_world::m_of_row;
    let lo = (x0 as i32, z0 as i32);
    let hi = ((x0 + side - 1) as i32, (z0 + side - 1) as i32);
    let mut out = Vec::new();
    for t in g.trees_sparse(lo, hi) {
        let p = &g.planters()[t.planter as usize];
        let placed = p.place(&t);
        let canopy = p.canopy();
        let (mut mn, mut mx) = ([i32::MAX; 3], [i32::MIN; 3]);
        let mut other = None;
        for (c, m) in placed.cells() {
            if m == canopy {
                for a in 0..3 {
                    mn[a] = mn[a].min(c[a]);
                    mx[a] = mx[a].max(c[a]);
                }
            } else {
                other.get_or_insert(m);
            }
        }
        if mn[0] > mx[0] {
            continue;
        }
        let y = |row: i32| m_of_row(row) as f32;
        let a = t.anchor;
        let trunk = (mn[1] > a[1]).then(|| {
            (
                [a[0] as f32, y(a[1]), a[2] as f32],
                [a[0] as f32 + 1.0, y(mn[1]), a[2] as f32 + 1.0],
                other.map_or(0x6b4a2b, rgb_of),
            )
        });
        out.push(TreeBox {
            trunk,
            canopy: (
                [mn[0] as f32, y(mn[1]), mn[2] as f32],
                [mx[0] as f32 + 1.0, y(mx[1] + 1), mx[2] as f32 + 1.0],
                rgb_of(canopy),
            ),
        });
    }
    out
}

/// A column's ground height on the ½ m cells: its top voxel's top (`Column::height`'s top voxel is
/// the row ceil(height ÷ ½) − 1).
pub fn cell_top(h: f32) -> f32 {
    (h * 2.0).ceil() / 2.0
}

/// Samples the section from (x0, z0) (snapped to the texel grid).
pub fn section(g: &TerrainGenerator, x0: i64, z0: i64, n: usize, texel: i64) -> Section {
    let texel = texel.max(1);
    let (gx0, gz0) = (x0.div_euclid(texel), z0.div_euclid(texel));
    let mut s = Section {
        x0: gx0 * texel,
        z0: gz0 * texel,
        n,
        texel,
        height: Vec::with_capacity(n * n),
        water: Vec::with_capacity(n * n),
        rgb: Vec::with_capacity(n * n),
        trees: Vec::new(),
    };
    let side = n as i64 * texel;
    if side <= TREES_UP_TO {
        s.trees = tree_boxes(g, s.x0, s.z0, side);
    }
    let min_half = 0.5 * texel as f64;
    for j in 0..n as i64 {
        for i in 0..n as i64 {
            let (x, z) = (s.x0 + i * texel + texel / 2, s.z0 + j * texel + texel / 2);
            let col = g.column_lod(x, z, texel);
            if col.outside {
                s.height.push(f32::NAN);
                s.water.push(f32::NAN);
                s.rgb.push(0);
                continue;
            }
            let mut h = cell_top(col.height);
            let mut level = if col.water != Water::None {
                col.level
            } else {
                f32::NAN
            };
            // A river's channel the column lies in: its bed under the bank, water a cell down.
            if level.is_nan()
                && let Some(r) = g.river_hit(x, z, min_half)
                && r.d < r.half
            {
                level = h - 0.5;
                h = cell_top((f64::from(h) - r.depth.max(1.0)) as f32);
            }
            let c = top_rgb(g.top_state(&col)).map(srgb_byte);
            s.height.push(h);
            s.water.push(level);
            s.rgb
                .push((u32::from(c[0]) << 16) | (u32::from(c[1]) << 8) | u32::from(c[2]));
        }
    }
    s
}

/// A mesh for the page: vertices of four u32 (x, y, z as f32 bits, local metres from the
/// section's corner; then `0xNNRRGGBB`, N the face: 0 up, 1 +x, 2 −x, 3 +z, 4 −z) and u32
/// triangle indices.
#[derive(Clone, Debug, Default)]
pub struct Mesh {
    pub verts: Vec<u32>,
    pub index: Vec<u32>,
}

impl Mesh {
    fn quad(&mut self, corners: [[f32; 3]; 4], rgb: u32, face: u32) {
        let base = (self.verts.len() / 4) as u32;
        for c in corners {
            self.verts.extend([
                c[0].to_bits(),
                c[1].to_bits(),
                c[2].to_bits(),
                (face << 24) | rgb,
            ]);
        }
        self.index
            .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    pub fn vertex(&self, k: usize) -> [f32; 3] {
        [0, 1, 2].map(|a| f32::from_bits(self.verts[k * 4 + a]))
    }
}

/// Whether the column (i, j) lies in its terrain's shadow: a march toward the sun over the
/// heights, `steps` columns at most.
fn shadowed(s: &Section, i: usize, j: usize, steps: usize) -> bool {
    let h0 = f64::from(s.height[j * s.n + i]);
    let t = s.texel as f64;
    let horiz = (SUN[0] * SUN[0] + SUN[2] * SUN[2]).sqrt();
    let rise = SUN[1] / horiz;
    let (dx, dz) = (SUN[0] / horiz, SUN[2] / horiz);
    for k in 1..=steps {
        let (fx, fz) = (i as f64 + dx * k as f64, j as f64 + dz * k as f64);
        let (ci, cj) = (fx.round(), fz.round());
        if ci < 0.0 || cj < 0.0 || ci >= s.n as f64 || cj >= s.n as f64 {
            return false;
        }
        let h = f64::from(s.height[cj as usize * s.n + ci as usize]);
        if h > h0 + rise * k as f64 * t {
            return true;
        }
    }
    false
}

fn scaled(rgb: u32, k: f64) -> u32 {
    dwell_game::minimap::scale_rgb(rgb, k)
}

/// The section's ground (blocky: a top quad a column, a wall down to each lower neighbour, down to
/// the section's floor at its edge) and water (a quad at each wet column's level) meshes.
pub fn section_mesh(s: &Section) -> (Mesh, Mesh) {
    let n = s.n;
    let t = s.texel as f32;
    let floor = s
        .height
        .iter()
        .copied()
        .filter(|h| !h.is_nan())
        .fold(f32::INFINITY, f32::min)
        - 4.0;
    let mut ground = Mesh::default();
    let mut water = Mesh::default();
    let at = |i: i64, j: i64| -> f32 {
        if i < 0 || j < 0 || i >= n as i64 || j >= n as i64 {
            return floor;
        }
        let h = s.height[j as usize * n + i as usize];
        if h.is_nan() { floor } else { h }
    };
    // The shadow's reach: 200 m of march (a tall wall's shadow), at most 128 columns.
    let steps = ((200.0 / t) as usize).clamp(8, 128);
    for j in 0..n {
        for i in 0..n {
            let k = j * n + i;
            let h = s.height[k];
            if h.is_nan() {
                continue;
            }
            let (x0, z0) = (i as f32 * t, j as f32 * t);
            let (x1, z1) = (x0 + t, z0 + t);
            let lit = if shadowed(s, i, j, steps) { 0.62 } else { 1.0 };
            let top = scaled(s.rgb[k], lit);
            ground.quad([[x0, h, z0], [x0, h, z1], [x1, h, z1], [x1, h, z0]], top, 0);
            let wall = scaled(s.rgb[k], 0.85);
            let (ii, jj) = (i as i64, j as i64);
            let e = at(ii + 1, jj);
            if e < h {
                ground.quad(
                    [[x1, h, z0], [x1, h, z1], [x1, e, z1], [x1, e, z0]],
                    wall,
                    1,
                );
            }
            let w = at(ii - 1, jj);
            if w < h {
                ground.quad(
                    [[x0, h, z1], [x0, h, z0], [x0, w, z0], [x0, w, z1]],
                    wall,
                    2,
                );
            }
            let so = at(ii, jj + 1);
            if so < h {
                ground.quad(
                    [[x1, h, z1], [x0, h, z1], [x0, so, z1], [x1, so, z1]],
                    wall,
                    3,
                );
            }
            let no = at(ii, jj - 1);
            if no < h {
                ground.quad(
                    [[x0, h, z0], [x1, h, z0], [x1, no, z0], [x0, no, z0]],
                    wall,
                    4,
                );
            }
            let l = s.water[k];
            if !l.is_nan() && l > h {
                water.quad(
                    [[x0, l, z0], [x0, l, z1], [x1, l, z1], [x1, l, z0]],
                    0x3a6f9a,
                    0,
                );
            }
        }
    }
    // The structures: a box each for trunk and canopy, lit from the top, darker on the sides.
    let (ox, oz) = (s.x0 as f32, s.z0 as f32);
    for t in &s.trees {
        for (mn, mx, rgb) in t.trunk.into_iter().chain([t.canopy]) {
            let [x0, y0, z0] = [mn[0] - ox, mn[1], mn[2] - oz];
            let [x1, y1, z1] = [mx[0] - ox, mx[1], mx[2] - oz];
            let side = scaled(rgb, 0.8);
            ground.quad(
                [[x0, y1, z0], [x0, y1, z1], [x1, y1, z1], [x1, y1, z0]],
                rgb,
                0,
            );
            ground.quad(
                [[x1, y1, z0], [x1, y1, z1], [x1, y0, z1], [x1, y0, z0]],
                side,
                1,
            );
            ground.quad(
                [[x0, y1, z1], [x0, y1, z0], [x0, y0, z0], [x0, y0, z1]],
                side,
                2,
            );
            ground.quad(
                [[x1, y1, z1], [x0, y1, z1], [x0, y0, z1], [x1, y0, z1]],
                side,
                3,
            );
            ground.quad(
                [[x0, y1, z0], [x1, y1, z0], [x1, y0, z0], [x0, y0, z0]],
                side,
                4,
            );
        }
    }
    (ground, water)
}

/// The relief (hill-shade) factor of each inner texel of a `(n + 2)` × `(n + 2)` height grid of
/// `texel` m (rows of x; NaN where there is no height: no shade there): the sun's light on the
/// slope by central differences, over a flat texel's, clamped to [`RELIEF_MIN`, `RELIEF_MAX`].
pub fn hillshade(h: &[f64], n: usize, texel: f64) -> Vec<f64> {
    let m = n + 2;
    let mut out = Vec::with_capacity(n * n);
    for j in 0..n {
        for i in 0..n {
            let c = h[(j + 1) * m + i + 1];
            if c.is_nan() {
                out.push(1.0);
                continue;
            }
            let at = |di: usize, dj: usize| {
                let v = h[(j + dj) * m + i + di];
                if v.is_nan() { c } else { v }
            };
            let dx = (at(2, 1) - at(0, 1)) / (2.0 * texel);
            let dz = (at(1, 2) - at(1, 0)) / (2.0 * texel);
            let len = (dx * dx + 1.0 + dz * dz).sqrt();
            let lit = ((-dx * SUN[0] + SUN[1] - dz * SUN[2]) / len).max(0.0);
            out.push((lit / SUN[1]).clamp(RELIEF_MIN, RELIEF_MAX));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_worldgen::bifacial::BifacialTerrain;

    fn generator() -> TerrainGenerator {
        // face-a-only: Chisel previews face A (face B is its mirror, generated by the same rules).
        BifacialTerrain::height_field(1)
            .local(dwell_world::Face::A)
            .clone()
    }

    #[test]
    fn the_section_shows_its_trees_as_boxes_on_the_ground() {
        // face-a-only: Chisel previews face A (the placements' faces are dwell-worldgen's
        // tests/trees.rs).
        let g = generator();
        // Find a square with pines: the same trees the chunks stand.
        let (side, texel) = (256i64, 4i64);
        let n = (side / texel) as usize;
        let (x0, z0) = (-2000..2000)
            .step_by(256)
            .flat_map(|x| (-2000..2000).step_by(256).map(move |z| (x, z)))
            .find(|&(x, z)| !g.trees_in((x, z), (x + 255, z + 255)).is_empty())
            .expect("pines somewhere");
        let s = section(&g, i64::from(x0), i64::from(z0), n, texel);
        let trees = g.trees_in((x0, z0), (x0 + 255, z0 + 255));
        assert_eq!(s.trees.len(), trees.len(), "one box pair a tree");
        for (b, t) in s.trees.iter().zip(&trees) {
            let (mn, mx, _) = b.canopy;
            assert!(mx[1] > mn[1] && mn[1] >= dwell_world::m_of_row(t.anchor[1]) as f32);
        }
        let (ground, _) = section_mesh(&s);
        let bare = Section {
            trees: Vec::new(),
            ..s.clone()
        };
        let (plain, _) = section_mesh(&bare);
        assert!(
            ground.index.len() > plain.index.len(),
            "the boxes are meshed"
        );
        // A section larger than TREES_UP_TO leaves them out.
        let big = section(&g, i64::from(x0), i64::from(z0), 300, texel);
        assert!(big.trees.is_empty());
    }

    #[test]
    fn the_section_covers_its_square_at_its_resolution_with_the_generators_heights() {
        // face-a-only: Chisel previews face A.
        let g = generator();
        let (x0, z0, n, texel) = (-301, 117, 40, 4);
        let s = section(&g, x0, z0, n, texel);
        assert_eq!((s.x0, s.z0), (-304, 116), "snapped to the texel grid");
        assert_eq!(s.height.len(), n * n);
        assert_eq!(s.rgb.len(), n * n);
        for (i, j) in [(0, 0), (n - 1, 0), (0, n - 1), (n - 1, n - 1), (17, 23)] {
            let x = s.x0 + i as i64 * texel + texel / 2;
            let z = s.z0 + j as i64 * texel + texel / 2;
            let col = g.column_lod(x, z, texel);
            let h = s.height[j * n + i];
            if s.water[j * n + i].is_nan() || col.water != Water::None {
                assert_eq!(h, cell_top(col.height), "column ({x}, {z})");
            }
            assert_eq!(h * 2.0, (h * 2.0).round(), "½ m cells");
        }
    }

    #[test]
    fn the_meshs_top_vertices_sit_at_the_sections_heights() {
        // face-a-only: Chisel previews face A.
        let s = section(&generator(), 0, 0, 24, 2);
        let (ground, _) = section_mesh(&s);
        // Every top quad (face 0) is a column's: its four corners at that column's height.
        let mut tops = 0;
        for q in 0..ground.verts.len() / 16 {
            let face = ground.verts[q * 16 + 3] >> 24;
            if face != 0 {
                continue;
            }
            let v = ground.vertex(q * 4);
            let (i, j) = ((v[0] / 2.0) as usize, (v[2] / 2.0) as usize);
            for c in 0..4 {
                assert_eq!(ground.vertex(q * 4 + c)[1], s.height[j * 24 + i]);
            }
            tops += 1;
        }
        let land = s.height.iter().filter(|h| !h.is_nan()).count();
        assert_eq!(tops, land, "one top quad a column");
        assert_eq!(ground.index.len() / 6, ground.verts.len() / 16);
    }

    #[test]
    fn a_step_gets_a_wall_and_its_shadow() {
        // face-a-only: a hand-made section, no generator.
        let n = 16;
        let mut s = Section {
            x0: 0,
            z0: 0,
            n,
            texel: 1,
            height: vec![10.0; n * n],
            water: vec![f32::NAN; n * n],
            rgb: vec![0x808080; n * n],
            trees: Vec::new(),
        };
        // A tower at (8, 8), the sun from −x −z: its shadow falls toward +x +z.
        s.height[8 * n + 8] = 30.0;
        let (g, _) = section_mesh(&s);
        let walls = (0..g.verts.len() / 16)
            .filter(|q| g.verts[q * 16 + 3] >> 24 != 0)
            .count();
        // The tower's four, and the section's edge walls (4 sides × 16 columns).
        assert_eq!(walls, 4 + 4 * n);
        assert!(
            shadowed(&s, 9, 9, 16),
            "behind the tower, away from the sun"
        );
        assert!(!shadowed(&s, 7, 7, 16), "the sunny side");
    }

    #[test]
    fn the_relief_toggle_shades_the_preview_tile() {
        // face-a-only: Chisel previews face A.
        use crate::chisel::{Overlay, preview, preview_shaded};
        let g = generator();
        let (n, texel) = (48, 16);
        let plain = preview(&g, -384, -384, n, texel, Overlay::Blocks);
        assert_eq!(
            preview_shaded(&g, -384, -384, n, texel, Overlay::Blocks, false),
            plain
        );
        let lit = preview_shaded(&g, -384, -384, n, texel, Overlay::Blocks, true);
        let changed = (0..(n * n) as usize)
            .filter(|k| lit[k * 4..k * 4 + 3] != plain[k * 4..k * 4 + 3])
            .count();
        assert!(changed > (n * n) as usize / 4, "{changed} texels shaded");
    }

    #[test]
    fn the_hill_shade_is_darker_on_the_slope_away_from_the_sun() {
        // face-a-only: a hand-made grid.
        let n = 8;
        let m = n + 2;
        // A ridge along z, peaking at the middle column: the −x side faces the sun.
        let h: Vec<f64> = (0..m * m)
            .map(|k| 20.0 - 2.0 * ((k % m) as f64 - 4.5).abs())
            .collect();
        let s = hillshade(&h, n, 1.0);
        let (sunny, shaded) = (s[3 * n + 1], s[3 * n + 6]);
        assert!(
            shaded < sunny,
            "lee {shaded} not darker than the sunny side {sunny}"
        );
        let flat = hillshade(&vec![5.0; m * m], n, 1.0);
        assert!(flat.iter().all(|f| (f - 1.0).abs() < 1e-9), "flat is 1");
    }
}
