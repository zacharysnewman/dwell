//! Procedural block textures from tiled (periodic) noise (`client/src/render/textures.ts`): the
//! atlas layout the chunk mesher's `tiles` refer to, and each tile's average colour, the flat
//! colour of LOD faces. Every tile is seamless; tiles sit in one atlas, each in a gutter of wrapped
//! texels so mipmapping never bleeds. Tiles are named by the block registry's texture names; the
//! painters are prototype content, like the blocks (ARCHITECTURE.md §6.1). The noise reproduces the
//! TS bit for bit (JS int32 arithmetic).
//!
//! Two texture sets fill the same tiles ([`TextureSet`], ADR 0040): the **classic** painters below,
//! and the **painterly** set, baked at build time by `shared/textures/gen.mjs` from data
//! (`shared/textures/materials.json`, docs/PAINTERLY_TEXTURES.md) into `painterly.rgba`. A tile the
//! painterly set doesn't paint (the utility tiles: `plain`, `grid`, `missing`) keeps its classic
//! painter in both. The set is chosen once, before the atlas is built (`?textures=painterly`).

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

#[path = "painterly.gen.rs"]
mod painterly;
pub use painterly::{PAINTERLY_TILE, PAINTERLY_TILES};

/// Which texture set paints the atlas's tiles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextureSet {
    /// The procedural noise painters (the look players have).
    #[default]
    Classic,
    /// The painterly storybook set (docs/ART_DIRECTION.md), behind `?textures=painterly` until the
    /// owner approves it; flipping the default is moving `#[default]` here.
    Painterly,
}

impl TextureSet {
    pub const ALL: [TextureSet; 2] = [TextureSet::Classic, TextureSet::Painterly];

    /// The set's name (`classic`, `painterly`), as `?textures=` takes it.
    pub fn name(self) -> &'static str {
        match self {
            TextureSet::Classic => "classic",
            TextureSet::Painterly => "painterly",
        }
    }

    pub fn parse(name: &str) -> Option<TextureSet> {
        Self::ALL.into_iter().find(|s| s.name() == name)
    }

    /// The set a page's query (`?textures=painterly`) asks for; the default for none or an unknown
    /// name.
    pub fn from_query(search: &str) -> TextureSet {
        search
            .trim_start_matches('?')
            .split('&')
            .filter_map(|kv| kv.split_once('='))
            .find(|(k, _)| *k == "textures")
            .and_then(|(_, v)| TextureSet::parse(v))
            .unwrap_or_default()
    }

    pub(crate) fn index(self) -> usize {
        self as usize
    }
}

static SET: AtomicU8 = AtomicU8::new(TextureSet::Classic as u8);

/// Chooses the texture set the atlas, the tiles' average colours (the LOD colours) and the hotbar
/// swatches use. Call it before any of them is first built (each is computed once per set): on the
/// page at start-up and in each meshing worker before its first job.
pub fn set_texture_set(set: TextureSet) {
    SET.store(set as u8, Ordering::Relaxed);
}

/// The texture set in use ([`set_texture_set`]).
pub fn texture_set() -> TextureSet {
    if SET.load(Ordering::Relaxed) == TextureSet::Painterly as u8 {
        TextureSet::Painterly
    } else {
        TextureSet::Classic
    }
}

/// The painterly texel (x, y) of tile `name`, if the painterly set paints it.
fn painterly_texel(name: &str, x: i64, y: i64) -> Option<[u8; 4]> {
    let i = PAINTERLY_TILES.iter().position(|t| *t == name)?;
    let o = ((i * TILE + y as usize) * TILE + x as usize) * 4;
    let t = &painterly::PAINTERLY_TEXELS[o..o + 4];
    Some([t[0], t[1], t[2], t[3]])
}

/// Texel (x, y) of tile `name` in `set`: its colour (sRGB, 0–255) and alpha (0–255).
fn texel(set: TextureSet, name: &str, x: i64, y: i64) -> (Rgb, f64) {
    if set == TextureSet::Painterly
        && let Some([r, g, b, a]) = painterly_texel(name, x, y)
    {
        return ([f64::from(r), f64::from(g), f64::from(b)], f64::from(a));
    }
    (painter(name)(x, y), alpha(name, x, y))
}

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
pub const TILE_ORDER: [&str; 15] = [
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

/// Magenta and black squares, 4 × 4 per tile: what a block this build cannot draw looks like.
fn missing(x: i64, y: i64) -> Rgb {
    if ((x / 8) + (y / 8)) % 2 == 0 {
        [255.0, 0.0, 255.0]
    } else {
        [0.0, 0.0, 0.0]
    }
}

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
const BARK: Rgb = rgb(0x7a5236);
const WOOD: Rgb = rgb(0xc09a62);
const LEAVES: Rgb = rgb(0x6fa83a);
const LEAVES_HIGH: Rgb = rgb(0xa9c94a);

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

/// Bark: vertical streaks (noise varying along x only, stretched along y).
fn log_side(x: i64, y: i64) -> Rgb {
    let (fx, fy) = (x as f64, y as f64);
    let streak = tiled_noise(fx, fy * 0.125, 16, 4, 91);
    let n = tiled_fbm(fx, fy, 92);
    scale(BARK, 0.7 + 0.35 * streak + 0.15 * n)
}

/// Cut wood with growth rings around the tile centre, and a bark rim.
fn log_top(x: i64, y: i64) -> Rgb {
    let half = (TILE / 2) as f64;
    let dx = x as f64 - half + 0.5;
    let dy = y as f64 - half + 0.5;
    if dx.abs().max(dy.abs()) > half - 3.0 {
        return log_side(x, y);
    }
    let ring = 0.5 + 0.5 * (dx.hypot(dy) * 1.3).cos();
    scale(
        WOOD,
        0.8 + 0.15 * ring + 0.1 * tiled_fbm(x as f64, y as f64, 93),
    )
}

/// Leaves; the gaps between them are transparent (`leaves_alpha`: the leaves draw cutout).
fn leaves(x: i64, y: i64) -> Rgb {
    let n = tiled_fbm(x as f64, y as f64, 101);
    mix(scale(LEAVES, 0.95), LEAVES_HIGH, (n - 0.3).max(0.0) * 1.5)
}

fn leaves_alpha(x: i64, y: i64) -> f64 {
    if hash(x, y, 2, 102) > 0.8 { 0.0 } else { 255.0 }
}

/// The ladder's rails (two columns) and rungs (every 8 texels).
fn ladder_wood(x: i64, y: i64) -> bool {
    let rail = (3..=6).contains(&x) || (25..=28).contains(&x);
    let rung = (2..=4).contains(&y.rem_euclid(8));
    rail || rung
}

/// Rails and rungs; the gaps between them are transparent (`ladder_alpha`: the ladder draws
/// cutout) and keep the wood's colour, so filtering at a gap's edge never darkens the wood.
fn ladder(x: i64, y: i64) -> Rgb {
    let grain = 0.8 + 0.3 * tiled_noise(x as f64, y as f64 * 0.25, 16, 6, 161);
    let rail = (3..=6).contains(&x) || (25..=28).contains(&x);
    if rail {
        return scale(RAIL, grain);
    }
    scale(RAIL, grain * 0.9)
}

fn ladder_alpha(x: i64, y: i64) -> f64 {
    if ladder_wood(x, y) { 255.0 } else { 0.0 }
}

/// Blades of the `tallGrass` tile: (base x, base half-width, height, lean at the tip), in texels.
/// Bases stay two texels inside the tile's sides and tips under its top row, so nothing wraps
/// round the tile (a cross quad shows exactly one copy).
const BLADES: [(f64, f64, f64, f64); 18] = [
    (5.5, 0.75, 22.0, 1.5),
    (7.0, 0.85, 29.0, 2.0),
    (8.5, 0.65, 17.0, 3.5),
    (10.0, 0.8, 25.0, -3.0),
    (11.5, 0.6, 12.0, -1.5),
    (12.5, 0.7, 20.0, 2.5),
    (14.0, 0.9, 30.0, 0.5),
    (15.5, 0.6, 15.0, -2.5),
    (16.5, 0.75, 26.5, -4.0),
    (18.0, 0.7, 23.0, 4.0),
    (19.5, 0.6, 13.0, 1.0),
    (20.5, 0.85, 27.5, -1.5),
    (22.0, 0.65, 16.0, -3.5),
    (23.0, 0.8, 26.0, 2.5),
    (24.5, 0.6, 18.0, 0.5),
    (25.5, 0.7, 21.0, -1.0),
    (26.5, 0.75, 24.0, -2.0),
    (13.0, 0.55, 10.0, 1.5),
];

/// How fast a blade's alpha falls off across its edge, per texel: the alpha is a distance field
/// (0.5 on the edge), so the cutout shader's bilinear alpha up close traces each blade's true
/// outline — thin, pointed blades from a 32-texel tile.
const BLADE_FALLOFF: f64 = 0.5;

/// The blade nearest to covering texel (x, y) of the `tallGrass` tile (y = 0 at its foot): its
/// index, how far up it the texel is (0–1) and the texel's alpha (0–1, a distance field: above
/// 0.5 inside the blade). Each blade tapers from its base to a point and curves as it leans.
fn blade_at(x: i64, y: i64) -> Option<(usize, f64, f64)> {
    let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
    let mut best: Option<(usize, f64, f64)> = None;
    for (i, &(bx, half, height, lean)) in BLADES.iter().enumerate() {
        let t = py / height;
        if t >= 1.0 {
            continue;
        }
        let centre = bx + lean * t * t;
        let width = half * (1.0 - t).powf(0.8);
        let a = (0.5 + (width - (px - centre).abs()) * BLADE_FALLOFF).clamp(0.0, 1.0);
        // The most covering blade wins (later ones on ties: they draw over).
        if a > 0.0 && best.is_none_or(|(_, _, b)| a >= b) {
            best = Some((i, t, a));
        }
    }
    best
}

/// Blades of grass in the grass block's top colours, darker at the foot and lighter at the tips,
/// each blade its own shade; the gaps are transparent (`tall_grass_alpha`) and take the blades'
/// mean colour so filtering at an edge adds no fringe.
fn tall_grass(x: i64, y: i64) -> Rgb {
    let Some((i, t, _)) = blade_at(x, y) else {
        return mix(GRASS_LOW, GRASS, 0.6);
    };
    let shade = hash(i as i64, 0, 3, 171);
    let base = mix(GRASS_LOW, GRASS, 0.35 + 0.5 * shade);
    let tip = mix(GRASS, GRASS_HIGH, 0.3 + 0.6 * shade);
    let grain = 0.97 + 0.06 * hash(x, y, 1, 172);
    scale(
        mix(base, tip, t.powf(0.8)),
        grain * (0.78 + 0.22 * t.sqrt()),
    )
}

fn tall_grass_alpha(x: i64, y: i64) -> f64 {
    blade_at(x, y).map_or(0.0, |(_, _, a)| 255.0 * a)
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
        // The same skirt as the grass side, so the two line up where they meet.
        "grassSide" | "dirtSkirt" => grass_side,
        "dirt" => dirt,
        "stone" => stone,
        "sand" => sand,
        "water" => water,
        "ladder" => ladder,
        "grid" => grid,
        "logSide" => log_side,
        "logTop" => log_top,
        "leaves" => leaves,
        "tallGrass" => tall_grass,
        MISSING_TILE => missing,
        _ => |_, _| [255.0, 255.0, 255.0],
    }
}

/// The alpha (0–255) of tile `name` at texel (x, y): opaque but where a cutout tile is
/// see-through.
fn alpha(name: &str, x: i64, y: i64) -> f64 {
    match name {
        "ladder" => ladder_alpha(x, y),
        "leaves" => leaves_alpha(x, y),
        "tallGrass" => tall_grass_alpha(x, y),
        _ => 255.0,
    }
}

/// Whether tile `name` has transparent texels (it is drawn cutout or translucent) in the set in use.
pub fn tile_has_alpha(name: &str) -> bool {
    tile_has_alpha_in(texture_set(), name)
}

/// Whether tile `name` has transparent texels in `set`.
pub fn tile_has_alpha_in(set: TextureSet, name: &str) -> bool {
    (0..TILE as i64).any(|y| (0..TILE as i64).any(|x| texel(set, name, x, y).1 < 255.0))
}

fn byte(v: f64) -> u8 {
    crate::js_round(v.clamp(0.0, 255.0)) as u8
}

/// The atlas in the set in use: RGBA8, `ATLAS_SIZE`², row 0 at v = 0 (no vertical flip on upload).
pub fn build_atlas() -> Vec<u8> {
    build_atlas_in(texture_set())
}

/// The atlas painted with `set`.
pub fn build_atlas_in(set: TextureSet) -> Vec<u8> {
    let mut data = vec![0u8; ATLAS_SIZE * ATLAS_SIZE * 4];
    for (index, name) in TILE_ORDER.iter().enumerate() {
        let ox = (index % ATLAS_CELLS) * CELL;
        let oy = (index / ATLAS_CELLS) * CELL;
        for py in 0..CELL {
            for px in 0..CELL {
                let tx = (px as i64 - GUTTER as i64).rem_euclid(TILE as i64);
                let ty = (py as i64 - GUTTER as i64).rem_euclid(TILE as i64);
                let ([r, g, b], a) = texel(set, name, tx, ty);
                let o = ((oy + py) * ATLAS_SIZE + ox + px) * 4;
                data[o] = byte(r);
                data[o + 1] = byte(g);
                data[o + 2] = byte(b);
                data[o + 3] = byte(a);
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

fn compute_average(set: TextureSet, name: &str) -> u32 {
    let (mut r, mut g, mut b, mut n) = (0.0, 0.0, 0.0, 0.0);
    // Weighted by coverage: a cutout tile's transparent texels don't count.
    for y in 0..TILE as i64 {
        for x in 0..TILE as i64 {
            let ([pr, pg, pb], alpha) = texel(set, name, x, y);
            let a = f64::from(byte(alpha)) / 255.0;
            r += srgb_to_linear(byte(pr)) * a;
            g += srgb_to_linear(byte(pg)) * a;
            b += srgb_to_linear(byte(pb)) * a;
            n += a;
        }
    }
    let n = if n > 0.0 { n } else { 1.0 };
    (u32::from(linear_to_srgb_byte(r / n)) << 16)
        | (u32::from(linear_to_srgb_byte(g / n)) << 8)
        | u32::from(linear_to_srgb_byte(b / n))
}

/// A tile's average colour (`0xRRGGBB`, sRGB): the mean of its covered texels in linear light, as a
/// mipmapped sRGB texture averages from afar — the flat colour of LOD faces. Computed from the
/// painters once per tile, in the set in use.
pub fn average_tile_color(name: &str) -> u32 {
    average_tile_color_in(texture_set(), name)
}

/// A tile's average colour in `set`.
pub fn average_tile_color_in(set: TextureSet, name: &str) -> u32 {
    static AVERAGES: [OnceLock<[u32; TILE_ORDER.len()]>; 2] = [OnceLock::new(), OnceLock::new()];
    let table = AVERAGES[set.index()].get_or_init(|| TILE_ORDER.map(|n| compute_average(set, n)));
    match TILE_ORDER.iter().position(|t| *t == name) {
        Some(i) => table[i],
        None => compute_average(set, name),
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
    fn the_ladder_tile_is_see_through_between_its_rails_and_rungs() {
        // Rails and rungs are opaque; the gaps are transparent, not painted dark.
        assert_eq!(alpha("ladder", 4, 0), 255.0, "a rail");
        assert_eq!(alpha("ladder", 15, 3), 255.0, "a rung");
        assert_eq!(alpha("ladder", 15, 6), 0.0, "a gap");
        assert!(tile_has_alpha("ladder"));
        assert!(!tile_has_alpha("stone") && !tile_has_alpha("grass"));
        let atlas = build_atlas();
        let r = tile_rect("ladder");
        let (x0, y0) = (
            (r.u0 * ATLAS_SIZE as f64).round() as usize,
            (r.v0 * ATLAS_SIZE as f64).round() as usize,
        );
        let texel = |x: usize, y: usize| &atlas[((y0 + y) * ATLAS_SIZE + x0 + x) * 4..][..4];
        assert_eq!(texel(15, 6)[3], 0, "transparent in the atlas");
        assert_eq!(texel(4, 6)[3], 255);
        // The gap keeps the wood's colour (no dark fringe when filtered).
        assert!(texel(15, 6)[0] > 0x60, "{:?}", texel(15, 6));
        // The flat (LOD) colour is the wood's, not darkened by the gaps.
        let avg = average_tile_color("ladder");
        assert!(avg >> 16 > 0x80, "{avg:06x}");
    }

    #[test]
    fn the_tall_grass_tile_is_blades_with_transparent_gaps() {
        let covered = (0..TILE as i64)
            .flat_map(|y| (0..TILE as i64).map(move |x| (x, y)))
            .filter(|&(x, y)| alpha("tallGrass", x, y) >= 128.0)
            .count();
        let share = covered as f64 / (TILE * TILE) as f64;
        assert!((0.12..0.5).contains(&share), "{share}");
        // Dense at the foot, open at the top; nothing on the top row or the side columns (no
        // blade wraps round the tile).
        let row = |y: i64| {
            (0..TILE as i64)
                .filter(|&x| alpha("tallGrass", x, y) >= 128.0)
                .count()
        };
        assert!(row(1) > row(20) && row(20) > 0);
        assert_eq!(row(TILE as i64 - 1), 0);
        for y in 0..TILE as i64 {
            assert_eq!(alpha("tallGrass", 0, y), 0.0);
            assert_eq!(alpha("tallGrass", TILE as i64 - 1, y), 0.0);
        }
        // Its colour is the grass block's: the covered texels average near the grass top's.
        let avg = average_tile_color("tallGrass");
        let top = average_tile_color("grass");
        for shift in [16, 8, 0] {
            let (a, b) = ((avg >> shift) & 0xff, (top >> shift) & 0xff);
            assert!(a.abs_diff(b) < 0x30, "{avg:06x} vs {top:06x}");
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
    static ATLAS: [OnceLock<Vec<u8>>; 2] = [OnceLock::new(), OnceLock::new()];
    let set = texture_set();
    ATLAS[set.index()].get_or_init(|| build_atlas_in(set))
}

#[cfg(test)]
mod painterly_tests {
    use super::*;

    const P: TextureSet = TextureSet::Painterly;

    fn painterly_alpha(name: &str, x: i64, y: i64) -> f64 {
        texel(P, name, x, y).1
    }

    #[test]
    fn the_query_picks_the_set_and_the_default_is_classic() {
        assert_eq!(TextureSet::default(), TextureSet::Classic);
        assert_eq!(TextureSet::from_query(""), TextureSet::Classic);
        assert_eq!(TextureSet::from_query("?textures=painterly"), P);
        assert_eq!(TextureSet::from_query("?seed=3&textures=painterly&x"), P);
        assert_eq!(
            TextureSet::from_query("?textures=nope"),
            TextureSet::Classic
        );
        assert_eq!(
            TextureSet::from_query("?textures=classic"),
            TextureSet::Classic
        );
        for s in TextureSet::ALL {
            assert_eq!(TextureSet::parse(s.name()), Some(s));
        }
    }

    #[test]
    fn the_baked_tiles_are_atlas_tiles_of_the_atlas_size() {
        assert_eq!(PAINTERLY_TILE, TILE);
        assert_eq!(
            painterly::PAINTERLY_TEXELS.len(),
            PAINTERLY_TILES.len() * TILE * TILE * 4
        );
        for name in PAINTERLY_TILES {
            assert!(has_tile(name), "{name} is not an atlas tile");
        }
        // Every registry tile but the utility ones is painted.
        for s in &dwell_blocks::STATES {
            if let Some(t) = s.textures {
                for name in [t.top, t.side, t.bottom] {
                    assert!(name == "grid" || PAINTERLY_TILES.contains(&name), "{name}");
                }
            }
        }
    }

    #[test]
    fn the_painterly_atlas_differs_from_the_classic_only_in_painted_tiles() {
        let classic = build_atlas_in(TextureSet::Classic);
        let painterly = build_atlas_in(P);
        assert_eq!(classic.len(), painterly.len());
        for name in TILE_ORDER {
            let r = tile_rect(name);
            let (x0, y0) = (
                (r.u0 * ATLAS_SIZE as f64).round() as usize,
                (r.v0 * ATLAS_SIZE as f64).round() as usize,
            );
            let same = (0..TILE).all(|y| {
                let o = ((y0 + y) * ATLAS_SIZE + x0) * 4;
                classic[o..o + TILE * 4] == painterly[o..o + TILE * 4]
            });
            assert_eq!(same, !PAINTERLY_TILES.contains(&name), "{name}");
        }
    }

    #[test]
    fn painterly_cutouts_keep_the_shapes_the_meshes_assume() {
        // Opaque blocks stay opaque; the cutouts have gaps.
        for name in [
            "grass",
            "grassSide",
            "dirt",
            "stone",
            "sand",
            "logSide",
            "logTop",
        ] {
            assert!(!tile_has_alpha_in(P, name), "{name}");
        }
        for name in ["leaves", "tallGrass", "ladder"] {
            assert!(tile_has_alpha_in(P, name), "{name}");
        }
        // The ladder: rails, rungs and gaps where the classic tile has them.
        assert_eq!(painterly_alpha("ladder", 4, 0), 255.0, "a rail");
        assert_eq!(painterly_alpha("ladder", 15, 3), 255.0, "a rung");
        assert_eq!(painterly_alpha("ladder", 15, 6), 0.0, "a gap");
        // Tall grass: blades, nothing on the top row or the side columns (it doesn't wrap).
        let covered = |y: i64| {
            (0..TILE as i64)
                .filter(|&x| painterly_alpha("tallGrass", x, y) >= 128.0)
                .count()
        };
        let share = (0..TILE as i64).map(covered).sum::<usize>() as f64 / (TILE * TILE) as f64;
        assert!((0.12..0.5).contains(&share), "{share}");
        assert!(covered(1) > covered(20) && covered(20) > 0);
        assert_eq!(covered(TILE as i64 - 1), 0);
        for y in 0..TILE as i64 {
            assert_eq!(painterly_alpha("tallGrass", 0, y), 0.0);
            assert_eq!(painterly_alpha("tallGrass", TILE as i64 - 1, y), 0.0);
        }
        // Leaves: mostly covered, with gaps.
        let leaf = (0..TILE as i64)
            .flat_map(|y| (0..TILE as i64).map(move |x| (x, y)))
            .filter(|&(x, y)| painterly_alpha("leaves", x, y) >= 128.0)
            .count() as f64
            / (TILE * TILE) as f64;
        assert!((0.6..0.95).contains(&leaf), "{leaf}");
    }

    #[test]
    fn painterly_averages_stay_near_the_classic_hues() {
        // The LOD colours: the same material reads as the same colour from afar in either set.
        for name in [
            "grass",
            "dirt",
            "stone",
            "sand",
            "water",
            "logSide",
            "leaves",
            "tallGrass",
        ] {
            let (a, b) = (
                average_tile_color_in(P, name),
                average_tile_color_in(TextureSet::Classic, name),
            );
            for shift in [16, 8, 0] {
                let (x, y) = ((a >> shift) & 0xff, (b >> shift) & 0xff);
                assert!(x.abs_diff(y) < 0x40, "{name}: {a:06x} vs {b:06x}");
            }
        }
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
