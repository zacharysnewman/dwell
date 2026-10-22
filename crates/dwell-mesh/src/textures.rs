//! The block texture atlas: the layout the chunk mesher's `tiles` refer to, and each tile's average
//! colour, the flat colour of LOD faces. Every tile is seamless; tiles sit in one atlas, each in a
//! gutter of wrapped texels so mipmapping never bleeds. Tiles are named by the block registry's
//! texture names, plus the engine's utility tiles (`plain`, `missing`).
//!
//! The texels are the texture set (ADR 0040), baked at build time by `shared/textures/gen.mjs` from
//! data (`shared/textures/materials.json`, docs/PAINTERLY_TEXTURES.md) into `painterly.rgba`. It
//! paints every atlas tile, the utility tiles (`plain`, `grid`, `missing`) included. The materials
//! are prototype content, like the blocks (ARCHITECTURE.md §6.1).

use std::sync::OnceLock;

#[path = "painterly.gen.rs"]
mod painterly;
pub use painterly::{PAINTERLY_TILE, PAINTERLY_TILES};

/// The baked texel (x, y) of tile `name` (sRGB RGBA8). A name the set doesn't paint takes the
/// `plain` tile's (white).
fn texel(name: &str, x: i64, y: i64) -> [u8; 4] {
    let i = PAINTERLY_TILES
        .iter()
        .position(|t| *t == name)
        .or_else(|| PAINTERLY_TILES.iter().position(|t| *t == "plain"))
        .expect("the texture set paints the plain tile");
    let o = ((i * TILE + y as usize) * TILE + x as usize) * 4;
    let t = &painterly::PAINTERLY_TEXELS[o..o + 4];
    [t[0], t[1], t[2], t[3]]
}

/// The baked texel (x, y) of tile `name` (sRGB RGBA8, row 0 the tile's bottom): tests read tiles.
pub fn texel_of(name: &str, x: i64, y: i64) -> [u8; 4] {
    texel(name, x, y)
}

/// Texels per tile edge.
pub const TILE: usize = 32;
/// Wrapped texels around each tile.
pub const GUTTER: usize = 16;
pub const CELL: usize = TILE + 2 * GUTTER;
pub const ATLAS_CELLS: usize = 8;
pub const ATLAS_SIZE: usize = CELL * ATLAS_CELLS;
/// Tile slots in the atlas (cells × cells), the length of [`tile_grids`].
pub const ATLAS_TILES: usize = ATLAS_CELLS * ATLAS_CELLS;

/// The tiles in atlas order (the registry's texture names).
pub const TILE_ORDER: [&str; 23] = [
    "plain",
    "grass",
    "grassSide",
    "dirt",
    "stone",
    "sand",
    "water",
    "ladder",
    "grid",
    "logSide",
    "logTop",
    "leaves",
    "tallGrass",
    // A dirt side under grass (dirt's `sideUnder`): the turf's skirt over the dirt's top edge.
    "dirtSkirt",
    // The torch's plain stick, its two cells (docs/LIGHTS.md; its flame is the flame effect, §6).
    "torch",
    "torchTop",
    // The bedrock between the faces (docs/BIFACIAL_WORLD.md §2).
    "bedrock",
    // Snowy grass (its top, its side's fringe, dirt's skirt under it) and snowy leaves (ADR 0049).
    "snow",
    "snowSide",
    "dirtSnowSkirt",
    "snowyLeavesTop",
    "snowyLeavesSide",
    // The fallback look of a state whose look or textures this build lacks (synced from a server,
    // ADR 0038): a magenta and black checker, never content.
    MISSING_TILE,
];

/// The fallback tile (ADR 0038 decision 7).
pub const MISSING_TILE: &str = "missing";

/// Whether the atlas has tile `name`.
pub fn has_tile(name: &str) -> bool {
    TILE_ORDER.contains(&name)
}

/// A tile's rectangle in the atlas, in 0–1 texture coordinates (v = 0 is the atlas's row 0).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TileRect {
    pub u0: f64,
    pub v0: f64,
    pub u1: f64,
    pub v1: f64,
    /// Whether the tile hangs from its block's top edge ([`tile_hangs`]): a side face then shows the
    /// tile's top on every block; any other tile's v runs in world metres.
    pub hang: bool,
}

/// The rectangle of tile `name`. Like the TS (`indexOf` of −1), an unknown name lands one cell
/// before the atlas.
pub fn tile_rect(name: &str) -> TileRect {
    let index = TILE_ORDER
        .iter()
        .position(|t| *t == name)
        .map_or(-1, |i| i as i64);
    let cells = ATLAS_CELLS as i64;
    let cx = index % cells;
    let cy = index.div_euclid(cells);
    let u0 = (cx * CELL as i64 + GUTTER as i64) as f64 / ATLAS_SIZE as f64;
    let v0 = (cy * CELL as i64 + GUTTER as i64) as f64 / ATLAS_SIZE as f64;
    TileRect {
        u0,
        v0,
        u1: u0 + TILE as f64 / ATLAS_SIZE as f64,
        v1: v0 + TILE as f64 / ATLAS_SIZE as f64,
        hang: tile_hangs(name),
    }
}

/// The grid lines of each atlas tile slot (index cx + cy × `ATLAS_CELLS`, `TILE_ORDER` order):
/// `[every, strength, glow, 0]` (zero for none; the glow is [`tile_emissive`]). A tile's grid is the registry's `grid` look
/// option of the states drawn with it (an untextured state with the `plain` tile); `gen.mjs`
/// rejects a tile whose states disagree, so the terrain shader can find a face's grid from its
/// tile rectangle with nothing added to the vertex.
pub fn tile_grids() -> [[f32; 4]; ATLAS_TILES] {
    let mut out = [[0.0; 4]; ATLAS_TILES];
    for (i, name) in TILE_ORDER.iter().enumerate() {
        out[i][2] = tile_emissive(name);
    }
    for s in &dwell_blocks::STATES {
        let Some(g) = s.grid else { continue };
        let names = s
            .textures
            .map_or(["plain"; 3], |t| [t.top, t.side, t.bottom]);
        for name in names {
            if let Some(i) = TILE_ORDER.iter().position(|t| *t == name) {
                out[i][0] = f32::from(g.every);
                out[i][1] = g.strength;
            }
        }
    }
    out
}

/// How much of its colour tile `name` gives off unlit (its material's `emissive`, docs/LIGHTS.md):
/// a torch's flame; 0 for most.
pub fn tile_emissive(name: &str) -> f32 {
    PAINTERLY_TILES
        .iter()
        .position(|t| *t == name)
        .map_or(0.0, |i| painterly::PAINTERLY_EMISSIVE[i])
}

/// Whether tile `name` hangs from its block's top edge (its material's `hang`, a turf skirt): a side
/// face shows the tile's top half on every ½ m block, so it never merges up or down. Every other
/// tile runs in world metres, whatever quad its face is merged into.
pub fn tile_hangs(name: &str) -> bool {
    PAINTERLY_TILES
        .iter()
        .position(|t| *t == name)
        .is_some_and(|i| painterly::PAINTERLY_HANG[i])
}

/// Whether tile `name` has transparent texels (it is drawn cutout or translucent).
pub fn tile_has_alpha(name: &str) -> bool {
    (0..TILE as i64).any(|y| (0..TILE as i64).any(|x| texel(name, x, y)[3] < 255))
}

/// The atlas: RGBA8, `ATLAS_SIZE`², row 0 at v = 0 (no vertical flip on upload).
pub fn build_atlas() -> Vec<u8> {
    let mut data = vec![0u8; ATLAS_SIZE * ATLAS_SIZE * 4];
    for (index, name) in TILE_ORDER.iter().enumerate() {
        let ox = (index % ATLAS_CELLS) * CELL;
        let oy = (index / ATLAS_CELLS) * CELL;
        for py in 0..CELL {
            for px in 0..CELL {
                let tx = (px as i64 - GUTTER as i64).rem_euclid(TILE as i64);
                let ty = (py as i64 - GUTTER as i64).rem_euclid(TILE as i64);
                let o = ((oy + py) * ATLAS_SIZE + ox + px) * 4;
                data[o..o + 4].copy_from_slice(&texel(name, tx, ty));
            }
        }
    }
    data
}

/// An sRGB byte (0–255) as linear light (0–1).
pub fn srgb_to_linear(byte: u8) -> f64 {
    let c = f64::from(byte) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Linear light (0–1) as an sRGB byte (0–255).
pub fn linear_to_srgb_byte(v: f64) -> u8 {
    let c = if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    };
    crate::js_round(c.clamp(0.0, 1.0) * 255.0) as u8
}

fn compute_average(name: &str) -> u32 {
    tile_rows_average(name, 0, TILE)
}

/// The average colour (`0xRRGGBB`, sRGB, in linear light) of rows `from..to` of tile `name`: row 0
/// is the tile's bottom edge on a side face, row `TILE − 1` its top (a side's texture hangs from
/// the face's top edge, so a riser of one ½ m cell shows the top half's rows).
pub fn tile_rows_average(name: &str, from: usize, to: usize) -> u32 {
    let (mut r, mut g, mut b, mut n) = (0.0, 0.0, 0.0, 0.0);
    // Weighted by coverage: a cutout tile's transparent texels don't count.
    for y in from.min(TILE) as i64..to.min(TILE) as i64 {
        for x in 0..TILE as i64 {
            let [pr, pg, pb, alpha] = texel(name, x, y);
            let a = f64::from(alpha) / 255.0;
            r += srgb_to_linear(pr) * a;
            g += srgb_to_linear(pg) * a;
            b += srgb_to_linear(pb) * a;
            n += a;
        }
    }
    let n = if n > 0.0 { n } else { 1.0 };
    (u32::from(linear_to_srgb_byte(r / n)) << 16)
        | (u32::from(linear_to_srgb_byte(g / n)) << 8)
        | u32::from(linear_to_srgb_byte(b / n))
}

/// A tile's average colour (`0xRRGGBB`, sRGB): the mean of its covered texels in linear light, as a
/// mipmapped sRGB texture averages from afar — the flat colour of LOD faces. Computed once per tile.
pub fn average_tile_color(name: &str) -> u32 {
    static AVERAGES: OnceLock<[u32; TILE_ORDER.len()]> = OnceLock::new();
    let table = AVERAGES.get_or_init(|| TILE_ORDER.map(compute_average));
    match TILE_ORDER.iter().position(|t| *t == name) {
        Some(i) => table[i],
        None => compute_average(name),
    }
}

/// A small picture of a block state for the hotbar: its side texture, or its flat colour
/// (`TILE` × `TILE` RGBA8, sRGB).
pub fn swatch(state: dwell_blocks::StateId) -> Vec<u8> {
    let style = crate::style::material_style(state);
    let Some(textures) = style.textures else {
        let [r, g, b] = [
            (style.color >> 16) as u8,
            (style.color >> 8) as u8,
            style.color as u8,
        ];
        return [r, g, b, 255].repeat(TILE * TILE);
    };
    let atlas = shared_atlas();
    let rect = tile_rect(textures.side);
    let (x0, y0) = (
        (rect.u0 * ATLAS_SIZE as f64).round() as usize,
        (rect.v0 * ATLAS_SIZE as f64).round() as usize,
    );
    let mut out = Vec::with_capacity(TILE * TILE * 4);
    // Top row first: the picture's top is the tile's highest v (the atlas's row y0 + TILE − 1).
    for y in 0..TILE {
        let at = ((y0 + TILE - 1 - y) * ATLAS_SIZE + x0) * 4;
        out.extend_from_slice(&atlas[at..at + TILE * 4]);
    }
    out
}

fn shared_atlas() -> &'static [u8] {
    static ATLAS: OnceLock<Vec<u8>> = OnceLock::new();
    ATLAS.get_or_init(build_atlas)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha(name: &str, x: i64, y: i64) -> u8 {
        texel(name, x, y)[3]
    }

    #[test]
    fn the_set_paints_every_atlas_tile_at_the_atlas_size() {
        assert_eq!(PAINTERLY_TILE, TILE);
        assert_eq!(
            painterly::PAINTERLY_TEXELS.len(),
            PAINTERLY_TILES.len() * TILE * TILE * 4
        );
        // Every atlas tile is painted (the utility tiles too), and nothing else is.
        for name in TILE_ORDER {
            assert!(PAINTERLY_TILES.contains(&name), "{name} is not painted");
        }
        for name in PAINTERLY_TILES {
            assert!(has_tile(name), "{name} is not an atlas tile");
        }
        for s in &dwell_blocks::STATES {
            if let Some(t) = s.textures {
                for name in [t.top, t.side, t.bottom] {
                    assert!(has_tile(name), "{name}");
                }
            }
        }
    }

    #[test]
    fn tiles_sit_in_the_atlas_with_wrapped_gutters() {
        let r = tile_rect("grass");
        assert_eq!(r.u0, (CELL + GUTTER) as f64 / ATLAS_SIZE as f64);
        assert_eq!(r.v0, GUTTER as f64 / ATLAS_SIZE as f64);
        let atlas = build_atlas();
        assert_eq!(atlas.len(), ATLAS_SIZE * ATLAS_SIZE * 4);
        let at = |x: usize, y: usize| &atlas[(y * ATLAS_SIZE + x) * 4..][..4];
        for name in TILE_ORDER {
            let r = tile_rect(name);
            let (x0, y0) = (
                (r.u0 * ATLAS_SIZE as f64).round() as usize,
                (r.v0 * ATLAS_SIZE as f64).round() as usize,
            );
            for (x, y) in [(0, 0), (5, 17), (TILE - 1, TILE - 1)] {
                assert_eq!(
                    at(x0 + x, y0 + y),
                    texel(name, x as i64, y as i64),
                    "{name}"
                );
            }
            // The gutter repeats the tile: a texel just left of (below) the tile is its last
            // column's (row's).
            assert_eq!(at(x0 - 1, y0 + 3), at(x0 + TILE - 1, y0 + 3), "{name}");
            assert_eq!(at(x0 + 3, y0 - 1), at(x0 + 3, y0 + TILE - 1), "{name}");
        }
    }

    #[test]
    fn the_utility_tiles_are_plain_white_a_faint_grid_and_a_magenta_checker() {
        let last = TILE as i64 - 1;
        for (x, y) in [(0, 0), (9, 21), (last, last)] {
            assert_eq!(texel("plain", x, y), [255; 4]);
        }
        // The grid: one texel of line per edge (the bottom row and the left column, so each edge
        // gets one, from the block on its positive side), white elsewhere.
        for i in 0..TILE as i64 {
            for p in [(0, i), (i, 0)] {
                assert_eq!(texel("grid", p.0, p.1), [224, 224, 224, 255], "{p:?}");
            }
        }
        assert_eq!(texel("grid", last, 5), [255; 4]);
        assert_eq!(texel("grid", 1, 1), [255; 4]);
        assert_eq!(texel("grid", 16, 30), [255; 4]);
        // From afar (the average, the LOD colour) it is still nearly white.
        let avg = average_tile_color("grid");
        assert!(avg >> 16 > 0xf0 && avg >> 16 < 0xff, "{avg:06x}");
        assert!(tile_rect("grid").u1 <= 1.0 && tile_rect("grid").v1 <= 1.0);
        // Missing: magenta and black squares of 8 texels, unmistakable.
        for y in 0..TILE as i64 {
            for x in 0..TILE as i64 {
                let want = if ((x / 8) + (y / 8)) % 2 == 0 {
                    [255, 0, 255, 255]
                } else {
                    [0, 0, 0, 255]
                };
                assert_eq!(texel(MISSING_TILE, x, y), want, "({x}, {y})");
            }
        }
        // A name the set doesn't paint takes the plain tile.
        assert_eq!(average_tile_color("nope"), 0xffffff);
    }

    #[test]
    fn cutouts_keep_the_shapes_the_meshes_assume() {
        // Opaque blocks stay opaque; the cutouts have gaps.
        for name in [
            "plain",
            "grass",
            "grassSide",
            "dirt",
            "stone",
            "sand",
            "logSide",
            "logTop",
            "grid",
            "dirtSkirt",
            MISSING_TILE,
        ] {
            assert!(!tile_has_alpha(name), "{name}");
        }
        for name in ["leaves", "tallGrass", "ladder"] {
            assert!(tile_has_alpha(name), "{name}");
        }
        // The ladder: rails and rungs opaque, the gaps transparent, not painted dark.
        assert_eq!(alpha("ladder", 4, 0), 255, "a rail");
        assert_eq!(alpha("ladder", 15, 3), 255, "a rung");
        assert_eq!(alpha("ladder", 15, 6), 0, "a gap");
        let atlas = build_atlas();
        let r = tile_rect("ladder");
        let (x0, y0) = (
            (r.u0 * ATLAS_SIZE as f64).round() as usize,
            (r.v0 * ATLAS_SIZE as f64).round() as usize,
        );
        let at = |x: usize, y: usize| &atlas[((y0 + y) * ATLAS_SIZE + x0 + x) * 4..][..4];
        assert_eq!(at(15, 6)[3], 0, "transparent in the atlas");
        assert_eq!(at(4, 6)[3], 255);
        // The gap keeps the wood's colour (no dark fringe when filtered).
        assert!(at(15, 6)[0] > 0x60, "{:?}", at(15, 6));
        // The flat (LOD) colour is the wood's, not darkened by the gaps.
        let avg = average_tile_color("ladder");
        assert!(avg >> 16 > 0x80, "{avg:06x}");
        // Tall grass: blades, dense at the foot and open at the top; nothing on the top row or the
        // side columns (it doesn't wrap: a cross quad shows exactly one copy).
        let covered = |y: i64| {
            (0..TILE as i64)
                .filter(|&x| alpha("tallGrass", x, y) >= 128)
                .count()
        };
        let share = (0..TILE as i64).map(covered).sum::<usize>() as f64 / (TILE * TILE) as f64;
        assert!((0.12..0.5).contains(&share), "{share}");
        assert!(covered(1) > covered(20) && covered(20) > 0);
        assert_eq!(covered(TILE as i64 - 1), 0);
        for y in 0..TILE as i64 {
            assert_eq!(alpha("tallGrass", 0, y), 0);
            assert_eq!(alpha("tallGrass", TILE as i64 - 1, y), 0);
        }
        // Leaves: mostly covered, with gaps.
        let leaf = (0..TILE as i64)
            .flat_map(|y| (0..TILE as i64).map(move |x| (x, y)))
            .filter(|&(x, y)| alpha("leaves", x, y) >= 128)
            .count() as f64
            / (TILE * TILE) as f64;
        assert!((0.6..0.95).contains(&leaf), "{leaf}");
    }

    #[test]
    fn the_tall_grass_reads_as_the_grass_from_afar() {
        // Its covered texels average near the grass top's: a meadow's LOD colour is one green.
        let avg = average_tile_color("tallGrass");
        let top = average_tile_color("grass");
        for shift in [16, 8, 0] {
            let (a, b) = ((avg >> shift) & 0xff, (top >> shift) & 0xff);
            assert!(a.abs_diff(b) < 0x40, "{avg:06x} vs {top:06x}");
        }
    }

    #[test]
    fn the_tile_grid_table_comes_from_the_registrys_grid_look_option() {
        let table = tile_grids();
        // Every state's tiles carry its grid (or none), so a face finds it from its tile alone.
        for s in &dwell_blocks::STATES {
            let names = s
                .textures
                .map_or(["plain"; 3], |t| [t.top, t.side, t.bottom]);
            for name in names {
                let glow = tile_emissive(name);
                let want = s.grid.map_or([0.0, 0.0, glow, 0.0], |g| {
                    [f32::from(g.every), g.strength, glow, 0.0]
                });
                let i = TILE_ORDER
                    .iter()
                    .position(|t| *t == name)
                    .expect("a known tile");
                assert_eq!(table[i], want, "{} on {name}", s.name);
            }
        }
        // The sandbox floor's tile has its grid; the shader indexes cells as the atlas lays them out.
        let floor = dwell_blocks::info(dwell_blocks::require("dwell:sandbox_floor"));
        let g = floor.grid.expect("the floor's data has a grid");
        let r = tile_rect("grid");
        let cell = (r.u0 * ATLAS_CELLS as f64).floor() as usize
            + (r.v0 * ATLAS_CELLS as f64).floor() as usize * ATLAS_CELLS;
        assert_eq!(table[cell], [f32::from(g.every), g.strength, 0.0, 0.0]);
        // A tile's glow is its material's `emissive` (docs/LIGHTS.md §2; by data alone in
        // shared/textures/gen.test.mjs). None glows in the build: the torch's upper tile is a plain
        // stick, its flame the flame effect (§6).
        let torch = dwell_blocks::info(dwell_blocks::require("dwell:torch_upper"));
        let t = torch.textures.expect("the torch is textured").side;
        assert_eq!(
            table[TILE_ORDER.iter().position(|n| *n == t).unwrap()][2],
            0.0
        );
        assert_eq!(
            table[TILE_ORDER.iter().position(|n| *n == "stone").unwrap()][2],
            0.0
        );
        assert_eq!(table[0], [0.0; 4], "plain: no grid");
    }
}

#[cfg(test)]
mod swatch_tests {
    use super::*;

    #[test]
    fn swatches_are_a_tile_of_the_side_texture_or_the_flat_colour() {
        let grass = swatch(dwell_blocks::require("dwell:grass"));
        assert_eq!(grass.len(), TILE * TILE * 4);
        assert!(
            grass.chunks(4).any(|p| p != &grass[..4]),
            "a texture varies"
        );
        // The swatch's top-left texel is the tile's top row (highest v) in the atlas.
        let stone = swatch(dwell_blocks::require("dwell:stone"));
        let r = tile_rect("stone");
        let (x, y) = (
            (r.u0 * ATLAS_SIZE as f64).round() as usize,
            (r.v1 * ATLAS_SIZE as f64).round() as usize - 1,
        );
        let o = (y * ATLAS_SIZE + x) * 4;
        assert_eq!(stone[..4], shared_atlas()[o..o + 4]);
        let air = swatch(0);
        assert!(air.chunks(4).all(|p| p == [0, 0, 0, 255]), "a flat colour");
    }
}
