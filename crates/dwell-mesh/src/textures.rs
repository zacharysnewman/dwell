//! Procedural block textures from tiled (periodic) noise (`client/src/render/textures.ts`): the
//! atlas layout the chunk mesher's `tiles` refer to, and each tile's average colour, the flat
//! colour of LOD faces. Every tile is seamless; tiles sit in one atlas, each in a gutter of wrapped
//! texels so mipmapping never bleeds. Tiles are named by the block registry's texture names; the
//! painters are prototype content, like the blocks (ARCHITECTURE.md §6.1). The noise reproduces the
//! TS bit for bit (JS int32 arithmetic).

use std::sync::OnceLock;

/// Texels per tile edge; every noise lattice size below divides it.
pub const TILE: usize = 32;
/// Wrapped texels around each tile.
pub const GUTTER: usize = 16;
pub const CELL: usize = TILE + 2 * GUTTER;
pub const ATLAS_CELLS: usize = 8;
pub const ATLAS_SIZE: usize = CELL * ATLAS_CELLS;
/// Tile slots in the atlas (cells × cells), the length of [`tile_grids`].
pub const ATLAS_TILES: usize = ATLAS_CELLS * ATLAS_CELLS;

/// The tiles in atlas order (the registry's texture names).
pub const TILE_ORDER: [&str; 9] = [
    "plain",
    "grass",
    "grassSide",
    "dirt",
    "stone",
    "sand",
    "water",
    "ladder",
    "grid",
];

/// A tile's rectangle in the atlas, in 0–1 texture coordinates (v = 0 is the atlas's row 0).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TileRect {
    pub u0: f64,
    pub v0: f64,
    pub u1: f64,
    pub v1: f64,
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
    }
}

/// Hash of lattice point (i, j) in layer `k` → [0, 1) (JS: `| 0`, `Math.imul`, `>>>`).
fn hash(i: i64, j: i64, k: i64, seed: i64) -> f64 {
    let mut h = (i * 374_761_393 + j * 668_265_263 + k * 2_147_483_647 + seed * 144_269_504) as i32;
    h = (h ^ ((h as u32) >> 13) as i32).wrapping_mul(1_274_126_177);
    h ^= ((h as u32) >> 16) as i32;
    f64::from(h as u32) / 4_294_967_296.0
}

fn smooth(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

/// Value noise at texel (x, y) over a lattice of `cells` × `cells` per tile, wrapping at the tile
/// edge. In [0, 1].
fn tiled_noise(x: f64, y: f64, cells: i64, layer: i64, seed: i64) -> f64 {
    let fx = (x / TILE as f64) * cells as f64;
    let fy = (y / TILE as f64) * cells as f64;
    let x0 = fx.floor();
    let y0 = fy.floor();
    let tx = smooth(fx - x0);
    let ty = smooth(fy - y0);
    let (x0, y0) = (x0 as i64, y0 as i64);
    let h = |i: i64, j: i64| hash(i.rem_euclid(cells), j.rem_euclid(cells), layer, seed);
    let a = h(x0, y0) + (h(x0 + 1, y0) - h(x0, y0)) * tx;
    let b = h(x0, y0 + 1) + (h(x0 + 1, y0 + 1) - h(x0, y0 + 1)) * tx;
    a + (b - a) * ty
}

/// Fractal sum of tiled noise: lattices of 4, 8, 16 and 32 cells per tile. In [0, 1].
fn tiled_fbm(x: f64, y: f64, seed: i64) -> f64 {
    let mut sum = 0.0;
    let mut norm = 0.0;
    let mut amplitude = 1.0;
    let mut octave = 0;
    let mut cells = 4;
    while cells <= TILE as i64 {
        sum += tiled_noise(x, y, cells, octave, seed) * amplitude;
        norm += amplitude;
        amplitude *= 0.5;
        octave += 1;
        cells *= 2;
    }
    sum / norm
}

type Rgb = [f64; 3];

const fn rgb(hex: u32) -> Rgb {
    [
        ((hex >> 16) & 0xff) as f64,
        ((hex >> 8) & 0xff) as f64,
        (hex & 0xff) as f64,
    ]
}
fn mix(a: Rgb, b: Rgb, t: f64) -> Rgb {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}
fn scale(c: Rgb, s: f64) -> Rgb {
    [c[0] * s, c[1] * s, c[2] * s]
}

const GRASS: Rgb = rgb(0x8dbf3f);
const GRASS_HIGH: Rgb = rgb(0xb5cf4f);
const GRASS_LOW: Rgb = rgb(0x5f9a3a);
const DIRT: Rgb = rgb(0x8a5a3a);
const STONE: Rgb = rgb(0xbbafa6);
const STONE_COOL: Rgb = rgb(0x9c95a8);
const STONE_FLECK: Rgb = rgb(0xd6c6b4);
const SAND: Rgb = rgb(0xeed9a4);
const WATER: Rgb = rgb(0x2f86e0);
const RAIL: Rgb = rgb(0xa0703a);

fn grass(x: i64, y: i64) -> Rgb {
    let (fx, fy) = (x as f64, y as f64);
    let n = tiled_fbm(fx, fy, 11);
    let patches = tiled_noise(fx, fy, 4, 7, 12);
    let blade = hash(x, y, 9, 13);
    let lit = mix(GRASS, GRASS_HIGH, 0.55 * patches);
    let mut c = mix(GRASS_LOW, lit, (0.35 + 0.9 * n).min(1.0));
    if blade > 0.9 {
        c = scale(c, 0.82);
    } else if blade < 0.06 {
        c = scale(c, 1.1);
    }
    c
}

fn dirt(x: i64, y: i64) -> Rgb {
    let n = tiled_fbm(x as f64, y as f64, 21);
    let pebble = hash(x, y, 3, 22);
    let mut c = scale(DIRT, 0.72 + 0.45 * n);
    if pebble > 0.95 {
        c = scale(c, 1.25);
    }
    c
}

/// Dirt with a ragged grass fringe along the top edge (y = TILE − 1 is the top).
fn grass_side(x: i64, y: i64) -> Rgb {
    let depth = 4.0 + 5.0 * tiled_noise(x as f64, 0.0, 8, 5, 31);
    if ((TILE as i64 - 1 - y) as f64) < depth {
        grass(x, y)
    } else {
        dirt(x, y)
    }
}

fn stone(x: i64, y: i64) -> Rgb {
    let (fx, fy) = (x as f64, y as f64);
    let n = tiled_fbm(fx, fy, 41);
    let tint = tiled_noise(fx, fy, 4, 2, 42);
    let fleck = hash(x, y, 5, 44);
    let mut c = scale(mix(STONE, STONE_COOL, 0.8 * tint), 0.78 + 0.34 * n);
    if fleck > 0.92 {
        c = mix(c, STONE_COOL, 0.5);
    } else if fleck < 0.05 {
        c = STONE_FLECK;
    }
    c
}

fn sand(x: i64, y: i64) -> Rgb {
    let n = tiled_fbm(x as f64, y as f64, 51);
    let grain = hash(x, y, 1, 52);
    let mut c = scale(SAND, 0.86 + 0.2 * n);
    if grain > 0.9 {
        c = scale(c, 0.9);
    }
    c
}

fn water(x: i64, y: i64) -> Rgb {
    let (fx, fy) = (x as f64, y as f64);
    let ripple = tiled_noise(fx, fy, 4, 5, 151);
    let fine = tiled_fbm(fx, fy, 152);
    scale(WATER, 0.85 + 0.15 * ripple + 0.12 * fine)
}

fn ladder(x: i64, y: i64) -> Rgb {
    let grain = 0.8 + 0.3 * tiled_noise(x as f64, y as f64 * 0.25, 16, 6, 161);
    let rail = (3..=6).contains(&x) || (25..=28).contains(&x);
    let rung = (2..=4).contains(&(y % 8));
    if rail {
        return scale(RAIL, grain);
    }
    if rung {
        return scale(RAIL, grain * 0.9);
    }
    scale(RAIL, 0.3 + 0.08 * tiled_fbm(x as f64, y as f64, 162))
}

/// The grid lines of each atlas tile slot (index cx + cy × `ATLAS_CELLS`, `TILE_ORDER` order):
/// `[every, strength, 0, 0]`, all zero for none. A tile's grid is the registry's `grid` look
/// option of the states drawn with it (an untextured state with the `plain` tile); `gen.mjs`
/// rejects a tile whose states disagree, so the terrain shader can find a face's grid from its
/// tile rectangle with nothing added to the vertex.
pub fn tile_grids() -> [[f32; 4]; ATLAS_TILES] {
    let mut out = [[0.0; 4]; ATLAS_TILES];
    for s in &dwell_blocks::STATES {
        let Some(g) = s.grid else { continue };
        let names = s
            .textures
            .map_or(["plain"; 3], |t| [t.top, t.side, t.bottom]);
        for name in names {
            if let Some(i) = TILE_ORDER.iter().position(|t| *t == name) {
                out[i] = [f32::from(g.every), g.strength, 0.0, 0.0];
            }
        }
    }
    out
}

/// How much darker a `grid` tile's edge texels are than its white face.
pub const GRID_LINE: f64 = 0.88;

/// White with a faint line along every block edge: the first texel row and column, so each edge
/// gets one texel (1/32 block) of line, from the block on its positive side. Mipmapping averages
/// the line into the face with distance, so it fades out without shimmering.
fn grid(x: i64, y: i64) -> Rgb {
    let edge = x == 0 || y == 0;
    let v = if edge { 255.0 * GRID_LINE } else { 255.0 };
    [v, v, v]
}

/// The painter of tile `name`; an unknown name paints plain white (the TS has no painter for it).
fn painter(name: &str) -> fn(i64, i64) -> Rgb {
    match name {
        "grass" => grass,
        "grassSide" => grass_side,
        "dirt" => dirt,
        "stone" => stone,
        "sand" => sand,
        "water" => water,
        "ladder" => ladder,
        "grid" => grid,
        _ => |_, _| [255.0, 255.0, 255.0],
    }
}

fn byte(v: f64) -> u8 {
    crate::js_round(v.clamp(0.0, 255.0)) as u8
}

/// The atlas: RGBA8, `ATLAS_SIZE`², row 0 at v = 0 (no vertical flip on upload).
pub fn build_atlas() -> Vec<u8> {
    let mut data = vec![0u8; ATLAS_SIZE * ATLAS_SIZE * 4];
    for (index, name) in TILE_ORDER.iter().enumerate() {
        let paint = painter(name);
        let ox = (index % ATLAS_CELLS) * CELL;
        let oy = (index / ATLAS_CELLS) * CELL;
        for py in 0..CELL {
            for px in 0..CELL {
                let tx = (px as i64 - GUTTER as i64).rem_euclid(TILE as i64);
                let ty = (py as i64 - GUTTER as i64).rem_euclid(TILE as i64);
                let [r, g, b] = paint(tx, ty);
                let o = ((oy + py) * ATLAS_SIZE + ox + px) * 4;
                data[o] = byte(r);
                data[o + 1] = byte(g);
                data[o + 2] = byte(b);
                data[o + 3] = 255;
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
    let paint = painter(name);
    let (mut r, mut g, mut b) = (0.0, 0.0, 0.0);
    for y in 0..TILE as i64 {
        for x in 0..TILE as i64 {
            let [pr, pg, pb] = paint(x, y);
            r += srgb_to_linear(byte(pr));
            g += srgb_to_linear(byte(pg));
            b += srgb_to_linear(byte(pb));
        }
    }
    let n = (TILE * TILE) as f64;
    (u32::from(linear_to_srgb_byte(r / n)) << 16)
        | (u32::from(linear_to_srgb_byte(g / n)) << 8)
        | u32::from(linear_to_srgb_byte(b / n))
}

/// A tile's average colour (`0xRRGGBB`, sRGB): the mean of its texels in linear light, as a
/// mipmapped sRGB texture averages from afar — the flat colour of LOD faces. Computed from the
/// painters once per tile.
pub fn average_tile_color(name: &str) -> u32 {
    static AVERAGES: OnceLock<[u32; TILE_ORDER.len()]> = OnceLock::new();
    let table = AVERAGES.get_or_init(|| TILE_ORDER.map(compute_average));
    match TILE_ORDER.iter().position(|t| *t == name) {
        Some(i) => table[i],
        None => compute_average(name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_matches_js_int32_arithmetic() {
        // Values of the TS hash, computed with node.
        assert_eq!(hash(0, 0, 0, 0), 0.0);
        assert_eq!(hash(5, 7, 3, 22), 0.132_120_329_653_844_24);
        assert_eq!(hash(31, 2, 9, 162), 0.258_803_753_182_292);
        assert_eq!(hash(-1, 3, 2, 99), 0.504_417_388_932_779_4);
    }

    #[test]
    fn tiles_are_seamless_and_rects_in_the_atlas() {
        for x in 0..TILE as i64 {
            let a = tiled_noise(x as f64, 3.0, 8, 1, 7);
            let b = tiled_noise((x + TILE as i64) as f64, 3.0, 8, 1, 7);
            assert_eq!(a, b);
        }
        let r = tile_rect("grass");
        assert_eq!(r.u0, (CELL + GUTTER) as f64 / ATLAS_SIZE as f64);
        assert_eq!(r.v0, GUTTER as f64 / ATLAS_SIZE as f64);
        assert_eq!(build_atlas().len(), ATLAS_SIZE * ATLAS_SIZE * 4);
    }

    #[test]
    fn the_grid_tile_is_white_with_a_faint_line_at_its_edges() {
        let last = TILE as i64 - 1;
        let line = 255.0 * GRID_LINE;
        for i in 0..TILE as i64 {
            for p in [(0, i), (i, 0)] {
                assert_eq!(grid(p.0, p.1), [line; 3], "{p:?}");
            }
        }
        // One texel per edge: the far side is face, as is the interior.
        assert_eq!(grid(last, 5), [255.0; 3]);
        assert_eq!(grid(1, 1), [255.0; 3]);
        assert_eq!(grid(16, 30), [255.0; 3]);
        // From afar (the average, the LOD colour) it is still nearly white.
        let avg = average_tile_color("grid");
        assert!(avg >> 16 > 0xf0 && avg >> 16 < 0xff, "{avg:06x}");
        assert!(tile_rect("grid").u1 <= 1.0 && tile_rect("grid").v1 <= 1.0);
    }

    #[test]
    fn the_tile_grid_table_comes_from_the_registrys_grid_look_option() {
        let table = tile_grids();
        // Every state's tiles carry its grid (or none), so a face finds it from its tile alone.
        for s in &dwell_blocks::STATES {
            let names = s
                .textures
                .map_or(["plain"; 3], |t| [t.top, t.side, t.bottom]);
            let want = s
                .grid
                .map_or([0.0; 4], |g| [f32::from(g.every), g.strength, 0.0, 0.0]);
            for name in names {
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
        assert_eq!(table[0], [0.0; 4], "plain: no grid");
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
    static ATLAS: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    ATLAS.get_or_init(build_atlas)
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
