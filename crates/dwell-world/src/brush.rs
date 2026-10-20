//! Brushes (docs/BRUSHES.md; VoxelSniper-style painting and sculpting): a **brush** is the set of
//! cells a stroke covers (balls, discs, cubes, lines, blobs, stencils, trees, …), a **performer** is
//! what happens to them (set, replace, overlay, erase, blend, and the sculpt performers smooth,
//! erode, flatten, raise, lower, …). Both are a small fixed vocabulary here; the named presets
//! players pick are data (`shared/brushes/brushes.json` → `brushes.gen.rs`), as is which world
//! types allow brushes and how far they reach.
//!
//! Everything is a pure function of the brush, the aim and a grid of the world's states, so the
//! preview, the stroke and the tests agree (every random choice is seeded by the preset's seed and
//! the aim). A stroke's result is a list of changed cells, sent as `RegionEdit` pastes of at most
//! 32³ whose other cells are `region_edit::KEEP` ([`paste_edits`]); the server validates them like
//! any region edit.

use dwell_blocks::StateId;
use dwell_protocol::{RegionEdit, RegionEditOp, region_edit};

use crate::M;
use crate::edit::{FACE_DIRS, flooded_for, placeable, unbreakable};

#[rustfmt::skip]
#[path = "brushes.gen.rs"]
mod table;
pub use table::{FORMAT_VERSION, MAX_SIZE, MIN_SIZE, PRESETS, WORLD_POLICIES};

/// A cell and the state a stroke puts there.
pub type Change = ([i32; 3], StateId);

/// The `KEEP` cell of a paste: the server leaves it as it is (`welcome_flags::BRUSHES`).
pub const KEEP: StateId = region_edit::KEEP as StateId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BrushKind {
    Ball,
    /// A horizontal disc one cell thick.
    Disc,
    /// A disc `height` cells tall, standing on the aimed-at cell.
    Cylinder,
    Cube,
    /// A ball stretched by `stretch` along x, y, z.
    Ellipsoid,
    /// A ball with a seeded `density` of its cells (½ unless the preset says).
    Splatter,
    /// From the line's start (the previous click) to the aimed-at cell, `size` cells thick.
    Line,
    /// A horizontal square one cell thick (VoxelSniper's voxel disc).
    Square,
    /// A disc across the aimed-at face: horizontal on a floor, upright on a wall.
    DiscFace,
    /// A square across the aimed-at face (VoxelSniper's voxel disc face).
    SquareFace,
    /// An ellipse across the aimed-at face, stretched by `stretch` along the face's two axes.
    Ellipse,
    /// A horizontal ring `width` cells wide at the disc's edge.
    Ring,
    /// A filled triangle through the last two clicks and this one, `size` cells thick.
    Triangle,
    /// A line from the previous click that wanders by up to `jitter` cells (seeded).
    JaggedLine,
    /// A lump grown from the centre within the ball, each step taking a neighbour with chance
    /// `growth` (seeded).
    Blob,
    /// The upper half of an ellipsoid (stretched by `stretch`), standing on the aimed-at cell.
    Dome,
    /// The aimed-at cell alone.
    Snipe,
    /// A structure's cells (STRUCTURES.md), one of `structures` picked and turned by its variants
    /// (seeded): VoxelSniper's stencil and stencil list.
    Stencil,
    /// A procedural tree of `wood` and `leaves`: a trunk `height` tall, `branches` limbs
    /// `branch_length` long, a crown `size` in radius, `roots` roots (seeded).
    Tree,
}

pub const BRUSH_KINDS: [BrushKind; 19] = [
    BrushKind::Ball,
    BrushKind::Disc,
    BrushKind::Cylinder,
    BrushKind::Cube,
    BrushKind::Ellipsoid,
    BrushKind::Splatter,
    BrushKind::Line,
    BrushKind::Square,
    BrushKind::DiscFace,
    BrushKind::SquareFace,
    BrushKind::Ellipse,
    BrushKind::Ring,
    BrushKind::Triangle,
    BrushKind::JaggedLine,
    BrushKind::Blob,
    BrushKind::Dome,
    BrushKind::Snipe,
    BrushKind::Stencil,
    BrushKind::Tree,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PerformerKind {
    /// The material in every cell (that the mask lets through).
    Set,
    /// The material in cells holding the replace target.
    Replace,
    /// The material on the top `depth` layers of solid ground: cells with open space above. With
    /// `on_top`, `depth` new layers on the ground instead, in the open cells above it.
    Overlay,
    /// Air.
    Erase,
    /// A majority filter: a cell becomes solid when most of its 26 neighbours are, air when most
    /// are not; `iterations` times.
    Smooth,
    /// VoxelSniper's erosion: solid cells with at least `erode_faces` open faces become air
    /// (`erode_iterations` times), then open cells with at least `fill_faces` solid faces fill
    /// (`fill_iterations` times). Erode, melt and fill/inflate are presets of it.
    Erode,
    /// Columns under the brush cut to the aimed-at height and filled up to it (`height` cells
    /// above and below).
    Flatten,
    /// Columns under the brush grown by `height` cells of their top block (less toward the edge
    /// with `falloff`).
    Raise,
    /// Columns under the brush cut down by `height` cells.
    Lower,
    /// VoxelSniper's blend: each cell takes the commonest state among its neighbours (26, or the 8
    /// in the plane for a flat brush) when one state leads; air and liquids count unless
    /// `exclude_air` / `exclude_water`; `iterations` times.
    Blend,
    /// A blend that fades out linearly toward the brush's edge: a cell is blended with a chance
    /// of 1 − its distance from the centre over the radius (seeded).
    LinearBlend,
    /// The material on the bottom `depth` layers of solid ground: cells with open space below.
    Underlay,
    /// Hollows solids out (VoxelSniper's shell brushes): solid cells further than `depth` cells
    /// from open space become air, leaving a shell `depth` thick.
    Hollow,
    /// Each column of the brush filled with the material from the aimed-at height down to the
    /// first solid block, at most `height` cells.
    FillDown,
    /// Solid cells pushed out along the aimed-at face's normal: up to `height` copies of each
    /// cell's block in the open cells in front of it.
    Extrude,
    /// The brush's own blocks (a stencil's or a tree's), where the mask lets them.
    Stamp,
}

pub const PERFORMER_KINDS: [PerformerKind; 16] = [
    PerformerKind::Set,
    PerformerKind::Replace,
    PerformerKind::Overlay,
    PerformerKind::Erase,
    PerformerKind::Smooth,
    PerformerKind::Erode,
    PerformerKind::Flatten,
    PerformerKind::Raise,
    PerformerKind::Lower,
    PerformerKind::Blend,
    PerformerKind::LinearBlend,
    PerformerKind::Underlay,
    PerformerKind::Hollow,
    PerformerKind::FillDown,
    PerformerKind::Extrude,
    PerformerKind::Stamp,
];

impl BrushKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ball => "Ball",
            Self::Disc => "Disc",
            Self::Cylinder => "Cylinder",
            Self::Cube => "Cube",
            Self::Ellipsoid => "Ellipsoid",
            Self::Splatter => "Splatter",
            Self::Line => "Line",
            Self::Square => "Square",
            Self::DiscFace => "Disc on face",
            Self::SquareFace => "Square on face",
            Self::Ellipse => "Ellipse",
            Self::Ring => "Ring",
            Self::Triangle => "Triangle",
            Self::JaggedLine => "Jagged line",
            Self::Blob => "Blob",
            Self::Dome => "Dome",
            Self::Snipe => "Snipe",
            Self::Stencil => "Stencil",
            Self::Tree => "Tree",
        }
    }

    /// Clicks it takes before the stroke's: a line's start, a triangle's first two corners.
    pub fn points(self) -> usize {
        match self {
            Self::Line | Self::JaggedLine => 1,
            Self::Triangle => 2,
            _ => 0,
        }
    }

    /// Brings its own blocks (a stencil's, a tree's), which only its preset's data names: the
    /// shape key (Z) skips it, and only it can stamp.
    pub fn own_blocks(self) -> bool {
        matches!(self, Self::Stencil | Self::Tree)
    }

    /// One cell thick and horizontal: a blend there looks at the 8 neighbours in its plane.
    pub fn flat(self) -> bool {
        matches!(self, Self::Disc | Self::Square | Self::Ring)
    }
}

impl PerformerKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Set => "Set",
            Self::Replace => "Replace",
            Self::Overlay => "Paint surface",
            Self::Erase => "Erase",
            Self::Smooth => "Smooth",
            Self::Erode => "Erode",
            Self::Flatten => "Flatten",
            Self::Raise => "Raise",
            Self::Lower => "Lower",
            Self::Blend => "Blend",
            Self::LinearBlend => "Linear blend",
            Self::Underlay => "Paint underside",
            Self::Hollow => "Hollow",
            Self::FillDown => "Fill down",
            Self::Extrude => "Extrude",
            Self::Stamp => "Stamp",
        }
    }

    /// Acts on whole columns under the brush's footprint, not on its cells.
    pub fn by_column(self) -> bool {
        matches!(
            self,
            Self::Flatten | Self::Raise | Self::Lower | Self::FillDown
        )
    }

    /// Uses the replace target (picked with a middle or Alt click).
    pub fn uses_target(self, mask: Mask) -> bool {
        self == Self::Replace || matches!(mask, Mask::Target | Mask::NotTarget)
    }
}

/// Which cells a set, erase, overlay, underlay, blend or stamp may change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mask {
    None,
    /// Only non-solid cells (air, water).
    Air,
    /// Only solid cells.
    Solid,
    /// Only cells holding the replace target (VoxelSniper's include).
    Target,
    /// Every cell but those holding the replace target (VoxelSniper's exclude).
    NotTarget,
    /// Only liquid cells (water): an erase with it drains.
    Liquid,
}

/// A preset's parameters; each brush and performer reads the ones it takes (`gen.mjs` says which).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    pub height: u8,
    pub stretch: [f32; 3],
    /// The share of the brush's cells kept (seeded); 1 keeps them all. Any brush takes it.
    pub density: f32,
    pub seed: u32,
    /// Only the cells of a 3D checkerboard (x + y + z even). Any brush takes it.
    pub checker: bool,
    pub depth: u8,
    pub mask: Mask,
    pub iterations: u8,
    pub erode_faces: u8,
    pub erode_iterations: u8,
    pub fill_faces: u8,
    pub fill_iterations: u8,
    pub falloff: bool,
    /// Overlay: new layers on the ground rather than painting its top.
    pub on_top: bool,
    /// Blend: air (and with `exclude_water`, liquids) never wins.
    pub exclude_air: bool,
    pub exclude_water: bool,
    /// Ring: its width in cells.
    pub width: u8,
    /// Jagged line: how far it wanders, in cells.
    pub jitter: u8,
    /// Blob: the chance a step takes a neighbour.
    pub growth: f32,
    /// Stencil: the structures (by name) one is picked from.
    pub structures: &'static [&'static str],
    /// Tree: its blocks by name ("" for the hotbar's block), limbs and roots.
    pub wood: &'static str,
    pub leaves: &'static str,
    pub branches: u8,
    pub branch_length: u8,
    pub roots: u8,
    pub thickness: u8,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            height: 1,
            stretch: [1.0; 3],
            density: 1.0,
            seed: 0,
            checker: false,
            depth: 1,
            mask: Mask::None,
            iterations: 1,
            erode_faces: 2,
            erode_iterations: 1,
            fill_faces: 5,
            fill_iterations: 1,
            falloff: false,
            on_top: false,
            exclude_air: false,
            exclude_water: false,
            width: 1,
            jitter: 1,
            growth: 0.5,
            structures: &[],
            wood: "",
            leaves: "",
            branches: 4,
            branch_length: 5,
            roots: 0,
            thickness: 1,
        }
    }
}

/// A named brush, performer, size and parameters (`shared/brushes/brushes.json`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Preset {
    pub name: &'static str,
    pub label: &'static str,
    pub brush: BrushKind,
    pub performer: PerformerKind,
    /// Radius in cells, `MIN_SIZE..=MAX_SIZE`.
    pub size: u8,
    /// `KeyboardEvent.code` that picks it while the brush tool is on.
    pub key: Option<&'static str>,
    pub params: Params,
}

impl Preset {
    pub fn spec(&self) -> BrushSpec {
        BrushSpec {
            brush: self.brush,
            performer: self.performer,
            size: self.size,
            params: self.params,
        }
    }
}

/// Whether a world type allows brushes, and how far a brush's region edits may be from the eye.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldPolicy {
    pub world_type: &'static str,
    pub brushes: bool,
    /// Metres from the eye to a region edit's nearest cell.
    pub reach: f32,
}

/// The preset of that name.
pub fn preset(name: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.name == name)
}

/// The brush policy of a world (by its generator, §6.3), if its type allows brushes.
pub fn brush_policy(generator_version: u32) -> Option<&'static WorldPolicy> {
    let t = crate::world_type(generator_version);
    WORLD_POLICIES
        .iter()
        .find(|p| p.world_type == t && p.brushes)
}

/// What a stroke is: the brush, the performer, the radius and the parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushSpec {
    pub brush: BrushKind,
    pub performer: PerformerKind,
    pub size: u8,
    pub params: Params,
}

/// Where a stroke goes: the cell it is centred on, the face it was aimed at (an index into
/// `FACE_DIRS`; 2, +Y, for a floor), and the earlier clicks a line or triangle takes (`from` the
/// last, `before` the one before it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Aim {
    pub center: [i32; 3],
    pub face: u8,
    pub from: Option<[i32; 3]>,
    pub before: Option<[i32; 3]>,
}

impl Aim {
    /// Centred on `center`, aimed at a floor, with no earlier clicks.
    pub fn at(center: [i32; 3]) -> Self {
        Self {
            center,
            face: 2,
            from: None,
            before: None,
        }
    }

    /// The axis (0 x, 1 y, 2 z) the aimed-at face looks along.
    pub fn axis(&self) -> usize {
        usize::from(self.face.min(5)) / 2
    }

    /// The aimed-at face's outward normal.
    pub fn normal(&self) -> [i32; 3] {
        FACE_DIRS[usize::from(self.face.min(5))]
    }
}

/// The blocks a stroke puts down: the hotbar's block, and the replace target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Materials {
    pub material: StateId,
    pub target: StateId,
}

fn mix(mut h: u64) -> u64 {
    // SplitMix64's finaliser.
    h = (h ^ (h >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    h = (h ^ (h >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    h ^ (h >> 31)
}

fn cell_hash(seed: u64, c: [i32; 3]) -> u64 {
    let mut h = mix(seed ^ 0x9e37_79b9_7f4a_7c15);
    for v in c {
        h = mix(h ^ u64::from(v as u32));
    }
    h
}

/// A seeded number in [0, 1) for `c` (an offset or a counter) in the stream `seed`.
fn unit(seed: u64, c: [i32; 3]) -> f64 {
    (cell_hash(seed, c) >> 11) as f64 / (1u64 << 53) as f64
}

/// The stroke's own seed: the preset's, mixed with the centre (and a stream for each use).
fn stroke_seed(spec: &BrushSpec, aim: Aim, stream: u64) -> u64 {
    cell_hash(
        u64::from(spec.params.seed) ^ stream.wrapping_mul(0x9e37),
        aim.center,
    )
}

/// The distance from `p` to the segment `a`–`b`, squared.
fn segment_dist2(p: [f64; 3], a: [f64; 3], b: [f64; 3]) -> f64 {
    let d: [f64; 3] = std::array::from_fn(|i| b[i] - a[i]);
    let len2: f64 = d.iter().map(|v| v * v).sum();
    let k = if len2 > 0.0 {
        ((0..3).map(|i| (p[i] - a[i]) * d[i]).sum::<f64>() / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (0..3).map(|i| (p[i] - a[i] - k * d[i]).powi(2)).sum()
}

fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The distance from `p` to the triangle `a b c` (filled), squared: the closest point on a
/// triangle (Ericson, *Real-Time Collision Detection*, 5.1.5).
fn triangle_dist2(p: [f64; 3], a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> f64 {
    let (ab, ac, ap) = (sub3(b, a), sub3(c, a), sub3(p, a));
    let at = |v: f64, w: f64| -> [f64; 3] { std::array::from_fn(|i| a[i] + ab[i] * v + ac[i] * w) };
    let d2 = |q: [f64; 3]| -> f64 { dot3(sub3(p, q), sub3(p, q)) };
    let (d1, d2v) = (dot3(ab, ap), dot3(ac, ap));
    if d1 <= 0.0 && d2v <= 0.0 {
        return d2(a);
    }
    let bp = sub3(p, b);
    let (d3, d4) = (dot3(ab, bp), dot3(ac, bp));
    if d3 >= 0.0 && d4 <= d3 {
        return d2(b);
    }
    let vc = d1 * d4 - d3 * d2v;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return d2(at(d1 / (d1 - d3), 0.0));
    }
    let cp = sub3(p, c);
    let (d5, d6) = (dot3(ab, cp), dot3(ac, cp));
    if d6 >= 0.0 && d5 <= d6 {
        return d2(c);
    }
    let vb = d5 * d2v - d1 * d6;
    if vb <= 0.0 && d2v >= 0.0 && d6 <= 0.0 {
        return d2(at(0.0, d2v / (d2v - d6)));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        let q: [f64; 3] = std::array::from_fn(|i| b[i] + (c[i] - b[i]) * w);
        return d2(q);
    }
    let denom = 1.0 / (va + vb + vc);
    if !denom.is_finite() {
        // Degenerate (the corners in a line): the distance to its longest side.
        return segment_dist2(p, a, b)
            .min(segment_dist2(p, b, c))
            .min(segment_dist2(p, a, c));
    }
    d2(at(vb * denom, vc * denom))
}

/// Every cell of the box `lo..=hi` whose centre `keep` accepts, as offsets.
fn scan(lo: [i32; 3], hi: [i32; 3], mut keep: impl FnMut([i32; 3]) -> bool) -> Vec<[i32; 3]> {
    let mut out = Vec::new();
    for y in lo[1]..=hi[1] {
        for z in lo[2]..=hi[2] {
            for x in lo[0]..=hi[0] {
                if keep([x, y, z]) {
                    out.push([x, y, z]);
                }
            }
        }
    }
    out
}

/// Cells within `t` of the polyline through `points` (offsets from the centre).
fn thick_path(points: &[[i32; 3]], t: f64) -> Vec<[i32; 3]> {
    let reach = t.ceil() as i32;
    let lo: [i32; 3] =
        std::array::from_fn(|i| points.iter().map(|p| p[i]).min().unwrap_or(0) - reach);
    let hi: [i32; 3] =
        std::array::from_fn(|i| points.iter().map(|p| p[i]).max().unwrap_or(0) + reach);
    let f = |p: [i32; 3]| p.map(f64::from);
    scan(lo, hi, |c| {
        let p = f(c);
        if points.len() == 1 {
            return dot3(sub3(p, f(points[0])), sub3(p, f(points[0]))) <= t * t;
        }
        points
            .windows(2)
            .any(|w| segment_dist2(p, f(w[0]), f(w[1])) <= t * t)
    })
}

/// The cells of the brush's own blocks (a stencil's, a tree's) with those blocks, as world cells
/// in layer order; empty for other brushes. A tree's blocks named "" are the hotbar's `material`.
pub fn brush_blocks(spec: &BrushSpec, aim: Aim, material: StateId) -> Vec<Change> {
    let mut out = match spec.brush {
        BrushKind::Stencil => stencil_blocks(spec, aim),
        BrushKind::Tree => tree_blocks(spec, aim, material),
        _ => Vec::new(),
    };
    out.sort_by_key(|(p, _)| [p[1], p[2], p[0]]);
    out.dedup_by_key(|(p, _)| *p);
    out
}

/// A stencil: one of the preset's structures, turned by its variants, its anchor on the centre.
fn stencil_blocks(spec: &BrushSpec, aim: Aim) -> Vec<Change> {
    let names = spec.params.structures;
    if names.is_empty() {
        return Vec::new();
    }
    let seed = stroke_seed(spec, aim, 1);
    let name = names[(mix(seed) % names.len() as u64) as usize];
    let Ok(s) = crate::structure::load(name) else {
        return Vec::new();
    };
    s.place(aim.center, mix(seed ^ 0x51)).cells().collect()
}

/// A state by name, else `fallback` ("" or a name this registry lacks).
fn named(name: &str, fallback: StateId) -> StateId {
    if name.is_empty() {
        return fallback;
    }
    dwell_blocks::parse_state(name).unwrap_or(fallback)
}

/// A procedural tree (VoxelSniper's generate-tree brush, in Dwell's own terms): a trunk, limbs
/// rising from its upper half, a leafy crown at the top and a smaller one at each limb's end, and
/// roots spreading down from its foot.
fn tree_blocks(spec: &BrushSpec, aim: Aim, material: StateId) -> Vec<Change> {
    let p = &spec.params;
    let (wood, leaves) = (named(p.wood, material), named(p.leaves, material));
    let seed = stroke_seed(spec, aim, 2);
    let mut k = 0;
    let mut rnd = || {
        k += 1;
        unit(seed, [k, 0, 0])
    };
    let c = aim.center;
    let height = i32::from(p.height.max(1)) + (rnd() * f64::from(p.height) * 0.25) as i32;
    let t = i32::from(p.thickness.clamp(1, 4));
    let mut wood_cells: Vec<[i32; 3]> = Vec::new();
    for y in 0..height {
        for z in 0..t {
            for x in 0..t {
                wood_cells.push([c[0] + x - (t - 1) / 2, c[1] + y, c[2] + z - (t - 1) / 2]);
            }
        }
    }
    let top = [c[0], c[1] + height - 1, c[2]];
    // Limbs: from the upper half, outward and up.
    let mut crowns: Vec<([i32; 3], f64)> = vec![(top, f64::from(spec.size) + 0.5)];
    let branches = i32::from(p.branches);
    for b in 0..branches {
        let start = c[1] + (f64::from(height) * (0.5 + 0.4 * rnd())) as i32;
        let angle = (f64::from(b) + rnd() * 0.6) / f64::from(branches) * std::f64::consts::TAU;
        let len = f64::from(p.branch_length.max(1)) * (0.7 + 0.3 * rnd());
        let end = [
            c[0] + (angle.cos() * len).round() as i32,
            start + (len * 0.5).round() as i32,
            c[2] + (angle.sin() * len).round() as i32,
        ];
        let from = [c[0], start, c[2]];
        for q in thick_path(&[[0; 3], sub_i(end, from)], 0.6) {
            wood_cells.push(add(from, q));
        }
        crowns.push((end, (f64::from(spec.size) * 0.6).max(1.0) + 0.5));
    }
    // Roots: from the foot, outward and down.
    for r in 0..i32::from(p.roots) {
        let angle = (f64::from(r) + rnd() * 0.5) / f64::from(p.roots) * std::f64::consts::TAU;
        let len = 2.0 + f64::from(t) + rnd() * 2.0;
        let end = [
            (angle.cos() * len).round() as i32,
            -(len * 0.6).round() as i32,
            (angle.sin() * len).round() as i32,
        ];
        for q in thick_path(&[[0; 3], end], 0.6) {
            wood_cells.push(add(c, q));
        }
    }
    let mut out: Vec<Change> = wood_cells.iter().map(|&w| (w, wood)).collect();
    // Leaves: ragged balls round the crowns, never over wood.
    for (centre, r) in crowns {
        let n = r.ceil() as i32;
        for o in scan([-n; 3], [n; 3], |o| {
            let d2 = f64::from(o[0] * o[0] + o[1] * o[1] + o[2] * o[2]);
            d2 <= r * r
                && (d2 <= (r - 1.0).max(0.0).powi(2) || unit(seed ^ 0x1eaf, add(centre, o)) < 0.6)
        }) {
            let cell = add(centre, o);
            if !wood_cells.contains(&cell) {
                out.push((cell, leaves));
            }
        }
    }
    out
}

fn sub_i(a: [i32; 3], b: [i32; 3]) -> [i32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// The cells a brush covers, in layer order (y, then z, then x) without repeats. Every random
/// choice (splatter, blob, jagged line, density, stencil and tree picks) is seeded by the preset's
/// seed and the aim, so the same stroke covers the same cells.
pub fn brush_cells(spec: &BrushSpec, aim: Aim) -> Vec<[i32; 3]> {
    let r = i32::from(spec.size.clamp(MIN_SIZE, MAX_SIZE));
    let c = aim.center;
    let rf = f64::from(r) + 0.5;
    let rr = rf * rf;
    let p = &spec.params;
    let rel = |q: Option<[i32; 3]>| sub_i(q.unwrap_or(c), c);
    // Offsets from the centre, in any order.
    let offsets: Vec<[i32; 3]> = match spec.brush {
        BrushKind::Ball | BrushKind::Splatter => scan([-r; 3], [r; 3], |[x, y, z]| {
            f64::from(x * x + y * y + z * z) <= rr
        }),
        BrushKind::Ellipsoid | BrushKind::Dome => {
            let axes = p.stretch.map(|s| rf * f64::from(s));
            let ext = axes.map(|a| (a - 0.5).ceil().max(0.0) as i32);
            let y0 = if spec.brush == BrushKind::Dome {
                0
            } else {
                -ext[1]
            };
            scan([-ext[0], y0, -ext[2]], ext, |o| {
                (0..3)
                    .map(|i| (f64::from(o[i]) / axes[i]).powi(2))
                    .sum::<f64>()
                    <= 1.0
            })
        }
        BrushKind::Disc | BrushKind::Cylinder => {
            let h = if spec.brush == BrushKind::Disc {
                1
            } else {
                i32::from(p.height.max(1))
            };
            scan([-r, 0, -r], [r, h - 1, r], |[x, _, z]| {
                f64::from(x * x + z * z) <= rr
            })
        }
        BrushKind::Ring => {
            let inner = (rf - f64::from(p.width.max(1))).max(0.0);
            scan([-r, 0, -r], [r, 0, r], |[x, _, z]| {
                let d = f64::from(x * x + z * z);
                d <= rr && d > inner * inner
            })
        }
        BrushKind::Cube => scan([-r; 3], [r; 3], |_| true),
        BrushKind::Square => scan([-r, 0, -r], [r, 0, r], |_| true),
        BrushKind::DiscFace | BrushKind::SquareFace | BrushKind::Ellipse => {
            let a = aim.axis();
            let (u, v) = ((a + 1) % 3, (a + 2) % 3);
            let mut lo = [-r; 3];
            let mut hi = [r; 3];
            let mut radii = [rf; 3];
            if spec.brush == BrushKind::Ellipse {
                // The face's two axes, in x, y, z order: stretch's.
                for i in [u, v] {
                    radii[i] = rf * f64::from(p.stretch[i]);
                    hi[i] = (radii[i] - 0.5).ceil().max(0.0) as i32;
                    lo[i] = -hi[i];
                }
            }
            (lo[a], hi[a]) = (0, 0);
            scan(lo, hi, |o| {
                spec.brush == BrushKind::SquareFace
                    || (f64::from(o[u]) / radii[u]).powi(2) + (f64::from(o[v]) / radii[v]).powi(2)
                        <= 1.0
            })
        }
        BrushKind::Line => {
            // `size` 1 is a line one cell thick.
            thick_path(&[rel(aim.from), [0; 3]], f64::from(r - 1) + 0.6)
        }
        BrushKind::JaggedLine => {
            let (from, to) = (rel(aim.from), [0; 3]);
            let len = (0..3)
                .map(|i| f64::from(from[i]).powi(2))
                .sum::<f64>()
                .sqrt();
            let jitter = i32::from(p.jitter);
            let step = f64::from((2 * jitter).max(2));
            let n = (len / step).ceil().max(1.0) as i32;
            let seed = stroke_seed(spec, aim, 3);
            let points: Vec<[i32; 3]> = (0..=n)
                .map(|k| {
                    let t = f64::from(k) / f64::from(n);
                    std::array::from_fn(|i| {
                        let base = f64::from(from[i]) * (1.0 - t) + f64::from(to[i]) * t;
                        let wobble = if k == 0 || k == n {
                            0
                        } else {
                            (unit(seed, [k, i as i32, 0]) * f64::from(2 * jitter + 1)) as i32
                                - jitter
                        };
                        base.round() as i32 + wobble
                    })
                })
                .collect();
            thick_path(&points, f64::from(r - 1) + 0.6)
        }
        BrushKind::Triangle => {
            let (a, b) = (rel(aim.before), rel(aim.from));
            let t = f64::from(r - 1) + 0.6;
            let reach = t.ceil() as i32;
            let lo: [i32; 3] = std::array::from_fn(|i| a[i].min(b[i]).min(0) - reach);
            let hi: [i32; 3] = std::array::from_fn(|i| a[i].max(b[i]).max(0) + reach);
            let f = |q: [i32; 3]| q.map(f64::from);
            scan(lo, hi, |o| {
                triangle_dist2(f(o), f(a), f(b), [0.0; 3]) <= t * t
            })
        }
        BrushKind::Blob => {
            let seed = stroke_seed(spec, aim, 4);
            let side = (2 * r + 1) as usize;
            let index = |o: [i32; 3]| {
                ((o[0] + r) as usize)
                    + side * (((o[2] + r) as usize) + side * ((o[1] + r) as usize))
            };
            let inside = |o: [i32; 3]| f64::from(o[0] * o[0] + o[1] * o[1] + o[2] * o[2]) <= rr;
            let mut grown = vec![false; side * side * side];
            grown[index([0; 3])] = true;
            let ball = scan([-r; 3], [r; 3], inside);
            for step in 0..r {
                let before = grown.clone();
                for &o in &ball {
                    if before[index(o)] {
                        continue;
                    }
                    let touches = FACES.iter().any(|&f| {
                        let n = add(o, f);
                        n.iter().all(|v| v.abs() <= r) && before[index(n)]
                    });
                    if touches && unit(seed, [o[0], o[1], o[2] ^ (step << 8)]) < f64::from(p.growth)
                    {
                        grown[index(o)] = true;
                    }
                }
            }
            ball.into_iter().filter(|&o| grown[index(o)]).collect()
        }
        BrushKind::Snipe => vec![[0; 3]],
        BrushKind::Stencil | BrushKind::Tree => brush_blocks(spec, aim, M.air)
            .into_iter()
            .map(|(q, _)| sub_i(q, c))
            .collect(),
    };
    // Splatter keeps half its cells unless its preset says otherwise.
    let density = if spec.brush == BrushKind::Splatter && p.density >= 1.0 {
        0.5
    } else {
        f64::from(p.density.clamp(0.0, 1.0))
    };
    let thin_seed = cell_hash(u64::from(p.seed), c);
    let threshold = (density * u32::MAX as f64) as u64;
    let mut out: Vec<[i32; 3]> = offsets
        .into_iter()
        .filter(|&o| density >= 1.0 || cell_hash(thin_seed, o) & 0xffff_ffff <= threshold)
        .map(|o| add(c, o))
        .filter(|w| !p.checker || (w[0] + w[1] + w[2]).rem_euclid(2) == 0)
        .collect();
    out.sort_by_key(|q| [q[1], q[2], q[0]]);
    out.dedup();
    out
}

/// The box of `cells`: its min corner and size; `None` for no cells.
pub fn bounds(cells: &[[i32; 3]]) -> Option<([i32; 3], [i32; 3])> {
    let first = cells.first()?;
    let (mut lo, mut hi) = (*first, *first);
    for c in cells {
        for i in 0..3 {
            lo[i] = lo[i].min(c[i]);
            hi[i] = hi[i].max(c[i]);
        }
    }
    Some((lo, std::array::from_fn(|i| hi[i] - lo[i] + 1)))
}

/// The box of the world a stroke reads: its cells' box grown by what the performer looks at
/// around them (neighbours, the columns above and below).
pub fn read_bounds(spec: &BrushSpec, cells: &[[i32; 3]]) -> Option<([i32; 3], [i32; 3])> {
    let (min, size) = bounds(cells)?;
    let p = &spec.params;
    let (side, up, down) = match spec.performer {
        PerformerKind::Set
        | PerformerKind::Replace
        | PerformerKind::Erase
        | PerformerKind::Stamp => (0, 0, 0),
        PerformerKind::Overlay if p.on_top => (0, 0, i32::from(p.depth)),
        PerformerKind::Overlay => (0, i32::from(p.depth), 0),
        PerformerKind::Underlay => (0, 0, i32::from(p.depth)),
        PerformerKind::Smooth
        | PerformerKind::Erode
        | PerformerKind::Blend
        | PerformerKind::LinearBlend => (1, 1, 1),
        PerformerKind::Hollow => {
            let d = i32::from(p.depth);
            (d, d, d)
        }
        PerformerKind::FillDown => (0, 0, i32::from(p.height)),
        PerformerKind::Extrude => {
            let h = i32::from(p.height);
            (h, h, h)
        }
        PerformerKind::Flatten => {
            let h = i32::from(p.height);
            (0, h, h)
        }
        PerformerKind::Raise | PerformerKind::Lower => {
            let reach = i32::from(spec.size) + i32::from(p.height) + 1;
            (0, reach, reach)
        }
    };
    Some((
        [min[0] - side, min[1] - down, min[2] - side],
        [size[0] + 2 * side, size[1] + up + down, size[2] + 2 * side],
    ))
}

/// A box of the world's states, read once for a stroke; cells outside it read as air.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grid {
    pub min: [i32; 3],
    pub size: [i32; 3],
    /// Layer order: x fastest, then z, then y.
    pub cells: Vec<StateId>,
}

impl Grid {
    pub fn new(min: [i32; 3], size: [i32; 3], cells: Vec<StateId>) -> Self {
        debug_assert_eq!(cells.len(), size.iter().product::<i32>() as usize);
        Self { min, size, cells }
    }

    fn index(&self, c: [i32; 3]) -> Option<usize> {
        let l: [i32; 3] = std::array::from_fn(|i| c[i] - self.min[i]);
        (0..3)
            .all(|i| l[i] >= 0 && l[i] < self.size[i])
            .then(|| (l[0] + self.size[0] * (l[2] + self.size[2] * l[1])) as usize)
    }

    pub fn get(&self, c: [i32; 3]) -> StateId {
        self.index(c).map_or(M.air, |i| self.cells[i])
    }

    fn set(&mut self, c: [i32; 3], m: StateId) {
        if let Some(i) = self.index(c) {
            self.cells[i] = m;
        }
    }
}

fn solid(m: StateId) -> bool {
    dwell_blocks::try_info(m).is_some_and(|s| s.solid)
}

const FACES: [[i32; 3]; 6] = [
    [1, 0, 0],
    [-1, 0, 0],
    [0, 1, 0],
    [0, -1, 0],
    [0, 0, 1],
    [0, 0, -1],
];

fn add(a: [i32; 3], b: [i32; 3]) -> [i32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// The most common placeable solid state among `states`, else `fallback` (a sculpt never puts down
/// an unbreakable block, or one the palette has no slot for). Ties go to the lowest id.
fn commonest(states: impl Iterator<Item = StateId>, fallback: StateId) -> StateId {
    let mut counts: Vec<(StateId, u32)> = Vec::new();
    for m in states.filter(|&m| solid(m) && placeable(m)) {
        match counts.iter_mut().find(|(s, _)| *s == m) {
            Some((_, n)) => *n += 1,
            None => counts.push((m, 1)),
        }
    }
    counts
        .into_iter()
        .max_by_key(|&(m, n)| (n, std::cmp::Reverse(m)))
        .map_or(fallback, |(m, _)| m)
}

fn passes(mask: Mask, existing: StateId, target: StateId) -> bool {
    match mask {
        Mask::None => true,
        Mask::Air => !solid(existing),
        Mask::Solid => solid(existing),
        Mask::Target => existing == target,
        Mask::NotTarget => existing != target,
        Mask::Liquid => dwell_blocks::try_info(existing).is_some_and(|s| s.liquid),
    }
}

/// Blend's pick for a cell: the commonest state among `around` that may win (no unbreakable or
/// unplaceable solid; air and liquids unless excluded), when one leads; `None` on a tie.
fn blend_pick(around: &[StateId], p: &Params) -> Option<StateId> {
    let mut counts: Vec<(StateId, u32)> = Vec::new();
    for &m in around {
        let liquid = dwell_blocks::try_info(m).is_some_and(|s| s.liquid);
        let may = if solid(m) {
            placeable(m)
        } else if liquid {
            !p.exclude_water
        } else {
            !p.exclude_air && !unbreakable(m)
        };
        if !may {
            continue;
        }
        match counts.iter_mut().find(|(s, _)| *s == m) {
            Some((_, n)) => *n += 1,
            None => counts.push((m, 1)),
        }
    }
    let best = counts.iter().map(|&(_, n)| n).max()?;
    let mut leaders = counts.iter().filter(|&&(_, n)| n == best);
    let (m, _) = *leaders.next()?;
    leaders.next().is_none().then_some(m)
}

/// What a stroke changes: (cell, new state) in layer order, only cells whose state changes and
/// never an unbreakable one. `cells` are the brush's ([`brush_cells`]); `grid` holds the world
/// over [`read_bounds`].
pub fn perform(
    spec: &BrushSpec,
    aim: Aim,
    cells: &[[i32; 3]],
    grid: &Grid,
    materials: Materials,
) -> Vec<([i32; 3], StateId)> {
    let p = &spec.params;
    let Materials { material, target } = materials;
    let mut work = grid.clone();
    match spec.performer {
        PerformerKind::Set | PerformerKind::Replace | PerformerKind::Erase => {
            let mask = if spec.performer == PerformerKind::Replace {
                Mask::Target
            } else {
                p.mask
            };
            let put = if spec.performer == PerformerKind::Erase {
                M.air
            } else {
                material
            };
            for &c in cells {
                let existing = grid.get(c);
                if passes(mask, existing, target) {
                    work.set(c, flooded_for(put, existing));
                }
            }
        }
        PerformerKind::Overlay if p.on_top => {
            // The `depth` open cells right above solid ground, through open cells.
            for &c in cells {
                let existing = grid.get(c);
                if solid(existing) || !passes(p.mask, existing, target) {
                    continue;
                }
                let on_ground = (1..=i32::from(p.depth))
                    .map(|k| grid.get([c[0], c[1] - k, c[2]]))
                    .try_fold((), |(), m| if solid(m) { Err(()) } else { Ok(()) })
                    .is_err();
                if on_ground {
                    work.set(c, flooded_for(material, existing));
                }
            }
        }
        PerformerKind::Underlay => {
            for &c in cells {
                let existing = grid.get(c);
                if !solid(existing) || !passes(p.mask, existing, target) {
                    continue;
                }
                let exposed =
                    (1..=i32::from(p.depth)).any(|k| !solid(grid.get([c[0], c[1] - k, c[2]])));
                if exposed {
                    work.set(c, material);
                }
            }
        }
        PerformerKind::Blend | PerformerKind::LinearBlend => {
            let flat = spec.brush.flat();
            let seed = stroke_seed(spec, aim, 5);
            let r = f64::from(spec.size) + 0.5;
            for _ in 0..p.iterations {
                let before = work.clone();
                for &c in cells {
                    if spec.performer == PerformerKind::LinearBlend {
                        let o = sub_i(c, aim.center);
                        let d = f64::from(o[0] * o[0] + o[1] * o[1] + o[2] * o[2]).sqrt();
                        if unit(seed, c) >= 1.0 - d / r {
                            continue;
                        }
                    }
                    let here = before.get(c);
                    if !passes(p.mask, here, target) {
                        continue;
                    }
                    let mut around = Vec::with_capacity(26);
                    for dy in if flat { 0..=0 } else { -1..=1 } {
                        for dz in -1..=1 {
                            for dx in -1..=1 {
                                if (dx, dy, dz) != (0, 0, 0) {
                                    around.push(before.get(add(c, [dx, dy, dz])));
                                }
                            }
                        }
                    }
                    if let Some(m) = blend_pick(&around, p) {
                        work.set(c, flooded_for(m, here));
                    }
                }
            }
        }
        PerformerKind::Hollow => {
            // Distance (in face steps) to open space, up to `depth`: solid cells further are
            // inside the shell.
            let d = i32::from(p.depth);
            let mut near: std::collections::HashSet<[i32; 3]> = std::collections::HashSet::new();
            let mut frontier: Vec<[i32; 3]> = Vec::new();
            for y in 0..grid.size[1] {
                for z in 0..grid.size[2] {
                    for x in 0..grid.size[0] {
                        let c = add(grid.min, [x, y, z]);
                        if solid(grid.get(c)) && FACES.iter().any(|&f| !solid(grid.get(add(c, f))))
                        {
                            near.insert(c);
                            frontier.push(c);
                        }
                    }
                }
            }
            for _ in 1..d {
                let mut next = Vec::new();
                for c in frontier {
                    for f in FACES {
                        let n = add(c, f);
                        if solid(grid.get(n)) && near.insert(n) {
                            next.push(n);
                        }
                    }
                }
                frontier = next;
            }
            for &c in cells {
                let existing = grid.get(c);
                if solid(existing) && !near.contains(&c) {
                    work.set(c, M.air);
                }
            }
        }
        PerformerKind::Extrude => {
            let n = aim.normal();
            for &c in cells {
                let m = grid.get(c);
                if !solid(m) || !placeable(m) {
                    continue;
                }
                for k in 1..=i32::from(p.height) {
                    let q = add(c, n.map(|v| v * k));
                    if solid(grid.get(q)) {
                        break;
                    }
                    work.set(q, flooded_for(m, grid.get(q)));
                }
            }
        }
        PerformerKind::Stamp => {
            for (c, m) in brush_blocks(spec, aim, material) {
                let existing = grid.get(c);
                if passes(p.mask, existing, target) {
                    work.set(c, flooded_for(m, existing));
                }
            }
        }
        PerformerKind::Overlay => {
            for &c in cells {
                let existing = grid.get(c);
                if !solid(existing) || !passes(p.mask, existing, target) {
                    continue;
                }
                // Within `depth` cells of open space above, through solid cells.
                let exposed =
                    (1..=i32::from(p.depth)).any(|k| !solid(grid.get([c[0], c[1] + k, c[2]])));
                if exposed {
                    work.set(c, material);
                }
            }
        }
        PerformerKind::Smooth => {
            for _ in 0..p.iterations {
                let before = work.clone();
                for &c in cells {
                    let mut around = Vec::with_capacity(26);
                    for dy in -1..=1 {
                        for dz in -1..=1 {
                            for dx in -1..=1 {
                                if (dx, dy, dz) != (0, 0, 0) {
                                    around.push(before.get(add(c, [dx, dy, dz])));
                                }
                            }
                        }
                    }
                    let n = around.iter().filter(|&&m| solid(m)).count();
                    let here = before.get(c);
                    if n >= 14 && !solid(here) {
                        work.set(c, commonest(around.into_iter(), material));
                    } else if n <= 12 && solid(here) {
                        work.set(c, M.air);
                    }
                }
            }
        }
        PerformerKind::Erode => {
            for _ in 0..p.erode_iterations {
                let before = work.clone();
                for &c in cells {
                    let open = FACES
                        .iter()
                        .filter(|&&f| !solid(before.get(add(c, f))))
                        .count();
                    if solid(before.get(c)) && open >= usize::from(p.erode_faces) {
                        work.set(c, M.air);
                    }
                }
            }
            for _ in 0..p.fill_iterations {
                let before = work.clone();
                for &c in cells {
                    let faces: Vec<StateId> =
                        FACES.iter().map(|&f| before.get(add(c, f))).collect();
                    let n = faces.iter().filter(|&&m| solid(m)).count();
                    if !solid(before.get(c)) && n >= usize::from(p.fill_faces) {
                        work.set(c, commonest(faces.into_iter(), material));
                    }
                }
            }
        }
        PerformerKind::FillDown => {
            // Each column, from the brush's lowest cell in it down to the first solid block.
            let mut columns: Vec<([i32; 2], i32)> = Vec::new();
            for c in cells {
                match columns.iter_mut().find(|(k, _)| *k == [c[0], c[2]]) {
                    Some((_, y)) => *y = (*y).min(c[1]),
                    None => columns.push(([c[0], c[2]], c[1])),
                }
            }
            for ([x, z], top) in columns {
                for y in (top - i32::from(p.height)..=top).rev() {
                    let existing = grid.get([x, y, z]);
                    if solid(existing) {
                        break;
                    }
                    work.set([x, y, z], flooded_for(material, existing));
                }
            }
        }
        PerformerKind::Flatten | PerformerKind::Raise | PerformerKind::Lower => {
            let mut columns: Vec<[i32; 2]> = cells.iter().map(|c| [c[0], c[2]]).collect();
            columns.sort_unstable();
            columns.dedup();
            let h = i32::from(p.height);
            let cy = aim.center[1];
            let r = f64::from(spec.size) + 0.5;
            for [x, z] in columns {
                if spec.performer == PerformerKind::Flatten {
                    for y in cy + 1..=cy + h {
                        if solid(grid.get([x, y, z])) {
                            work.set([x, y, z], M.air);
                        }
                    }
                    for y in cy - h..=cy {
                        if !solid(grid.get([x, y, z])) {
                            work.set([x, y, z], material);
                        }
                    }
                    continue;
                }
                let amount = if p.falloff {
                    let d2 = f64::from((x - aim.center[0]).pow(2) + (z - aim.center[2]).pow(2));
                    (f64::from(h) * (1.0 - d2 / (r * r))).round().max(0.0) as i32
                } else {
                    h
                };
                if amount == 0 {
                    continue;
                }
                // The column's top: the highest solid cell within the read box.
                let span = i32::from(spec.size) + h;
                let Some(top) = (cy - span..=cy + span)
                    .rev()
                    .find(|&y| solid(grid.get([x, y, z])))
                else {
                    continue;
                };
                if spec.performer == PerformerKind::Raise {
                    let top_state = grid.get([x, top, z]);
                    let put = if placeable(top_state) {
                        top_state
                    } else {
                        material
                    };
                    for y in top + 1..=top + amount {
                        if !solid(grid.get([x, y, z])) {
                            work.set([x, y, z], put);
                        }
                    }
                } else {
                    for y in top - amount + 1..=top {
                        work.set([x, y, z], M.air);
                    }
                }
            }
        }
    }
    // The cells that changed, in layer order, leaving unbreakable cells and unknown states alone.
    let mut out = Vec::new();
    for y in 0..grid.size[1] {
        for z in 0..grid.size[2] {
            for x in 0..grid.size[0] {
                let i = (x + grid.size[0] * (z + grid.size[2] * y)) as usize;
                let (old, new) = (grid.cells[i], work.cells[i]);
                if old == new
                    || unbreakable(old)
                    || usize::from(new) >= dwell_blocks::state_count()
                    || unbreakable(new)
                {
                    continue;
                }
                out.push(([grid.min[0] + x, grid.min[1] + y, grid.min[2] + z], new));
            }
        }
    }
    out
}

/// Cells to set as region-edit pastes: the cells' box split into tiles of at most
/// `region_edit::MAX_SIDE` a side, each trimmed to the cells in it, every other cell `KEEP`.
/// Tiles go in layer order; a tile with no cells is not sent.
pub fn paste_edits(cells: &[([i32; 3], StateId)]) -> Vec<RegionEdit> {
    let side = region_edit::MAX_SIDE as i32;
    let Some((min, _)) = bounds(&cells.iter().map(|c| c.0).collect::<Vec<_>>()) else {
        return Vec::new();
    };
    // Tile → its cells.
    let mut tiles: Vec<([i32; 3], Vec<Change>)> = Vec::new();
    for &(c, m) in cells {
        let t: [i32; 3] = std::array::from_fn(|i| (c[i] - min[i]).div_euclid(side));
        match tiles.iter_mut().find(|(k, _)| *k == t) {
            Some((_, v)) => v.push((c, m)),
            None => tiles.push((t, vec![(c, m)])),
        }
    }
    tiles.sort_by_key(|(t, _)| [t[1], t[2], t[0]]);
    tiles
        .into_iter()
        .map(|(_, list)| {
            let (lo, size) = bounds(&list.iter().map(|c| c.0).collect::<Vec<_>>()).expect("cells");
            let size8 = size.map(|s| s as u8);
            let mut out = vec![KEEP; size.iter().product::<i32>() as usize];
            for (c, m) in list {
                let l: [usize; 3] = std::array::from_fn(|i| (c[i] - lo[i]) as usize);
                out[RegionEdit::cell_index(size8, l[0], l[1], l[2])] = m;
            }
            RegionEdit {
                op: RegionEditOp::Paste,
                min: lo,
                size: size8,
                material: 0,
                from: 0,
                cells: out,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::{RegionCheck, check_region_edit};
    use crate::{VoxelWorld, empty_generator};
    use dwell_blocks::require;

    fn spec(brush: BrushKind, performer: PerformerKind, size: u8) -> BrushSpec {
        BrushSpec {
            brush,
            performer,
            size,
            params: Params::default(),
        }
    }

    fn at(c: [i32; 3]) -> Aim {
        Aim::at(c)
    }

    fn mats(material: StateId) -> Materials {
        Materials {
            material,
            target: 0,
        }
    }

    /// Mirrors every cell through the centre along `axis`.
    fn symmetric(cells: &[[i32; 3]], c: [i32; 3], axis: usize) -> bool {
        cells.iter().all(|p| {
            let mut q = *p;
            q[axis] = 2 * c[axis] - p[axis];
            cells.contains(&q)
        })
    }

    #[test]
    fn brush_cell_counts_and_symmetry() {
        let c = [10, 5, -3];
        let set = PerformerKind::Set;
        // A ball of radius r holds the cells within r + ½ of the centre.
        let ball = |r| brush_cells(&spec(BrushKind::Ball, set, r), at(c));
        assert_eq!(ball(1).len(), 19);
        assert_eq!(ball(2).len(), 81);
        let b4 = ball(4);
        assert!((0..3).all(|a| symmetric(&b4, c, a)));
        // Its volume approaches 4/3 π (r + ½)³.
        let ideal = 4.0 / 3.0 * std::f64::consts::PI * 4.5f64.powi(3);
        assert!((b4.len() as f64 / ideal - 1.0).abs() < 0.05, "{}", b4.len());
        // No repeats, in layer order.
        let mut sorted = b4.clone();
        sorted.sort_by_key(|p| [p[1], p[2], p[0]]);
        sorted.dedup();
        assert_eq!(sorted, b4);

        let disc = brush_cells(&spec(BrushKind::Disc, set, 3), at(c));
        assert_eq!(disc.len(), 37);
        assert!(disc.iter().all(|p| p[1] == c[1]));
        assert!(symmetric(&disc, c, 0) && symmetric(&disc, c, 2));
        let mut cyl = spec(BrushKind::Cylinder, set, 3);
        cyl.params.height = 4;
        let cyl = brush_cells(&cyl, at(c));
        assert_eq!(cyl.len(), 37 * 4);
        assert_eq!(bounds(&cyl).unwrap().0[1], c[1]); // stands on the aimed-at cell
        assert_eq!(
            brush_cells(&spec(BrushKind::Cube, set, 2), at(c)).len(),
            125
        );
        // The largest brush is 33 cells a side.
        let big = brush_cells(&spec(BrushKind::Cube, set, MAX_SIZE), at(c));
        assert_eq!(bounds(&big).unwrap().1, [33; 3]);

        let mut ell = spec(BrushKind::Ellipsoid, set, 4);
        ell.params.stretch = [1.0, 0.5, 2.0];
        let e = brush_cells(&ell, at(c));
        let (_, size) = bounds(&e).unwrap();
        assert_eq!(size, [9, 5, 19]);
        assert!((0..3).all(|a| symmetric(&e, c, a)));
        // Unstretched, it is the ball.
        assert_eq!(brush_cells(&spec(BrushKind::Ellipsoid, set, 4), at(c)), b4);
    }

    #[test]
    fn splatter_is_seeded_and_about_its_density() {
        let mut s = spec(BrushKind::Splatter, PerformerKind::Set, 6);
        s.params.density = 0.4;
        s.params.seed = 7;
        let a = brush_cells(&s, at([0, 0, 0]));
        assert_eq!(
            a,
            brush_cells(&s, at([0, 0, 0])),
            "the same stroke picks the same cells"
        );
        let ball = brush_cells(&spec(BrushKind::Ball, PerformerKind::Set, 6), at([0, 0, 0]));
        assert!(a.iter().all(|p| ball.contains(p)));
        let share = a.len() as f64 / ball.len() as f64;
        assert!((share - 0.4).abs() < 0.05, "{share}");
        // Another seed or another place picks other cells.
        s.params.seed = 8;
        assert_ne!(a, brush_cells(&s, at([0, 0, 0])));
        s.params.seed = 7;
        let moved: Vec<[i32; 3]> = brush_cells(&s, at([1, 0, 0]))
            .iter()
            .map(|p| [p[0] - 1, p[1], p[2]])
            .collect();
        assert_ne!(a, moved);
    }

    #[test]
    fn a_line_runs_between_its_ends() {
        let s = spec(BrushKind::Line, PerformerKind::Set, 1);
        let line = brush_cells(
            &s,
            Aim {
                from: Some([0, 0, 0]),
                ..Aim::at([10, 0, 0])
            },
        );
        assert_eq!(line.len(), 11);
        assert!(line.iter().all(|p| p[1] == 0 && p[2] == 0));
        // A diagonal is connected: each cell has a neighbour (26-connectivity) further on.
        let diag = brush_cells(
            &s,
            Aim {
                from: Some([0, 0, 0]),
                ..Aim::at([7, 3, -5])
            },
        );
        assert!(diag.contains(&[0, 0, 0]) && diag.contains(&[7, 3, -5]));
        for p in &diag {
            assert!(
                diag.iter()
                    .any(|q| q != p && (0..3).all(|i| (q[i] - p[i]).abs() <= 1)),
                "{p:?} is alone"
            );
        }
        let thick = brush_cells(
            &spec(BrushKind::Line, PerformerKind::Set, 2),
            Aim {
                from: Some([0, 0, 0]),
                ..Aim::at([10, 0, 0])
            },
        );
        assert!(thick.len() > 5 * 11);
    }

    fn grid_of(w: &mut VoxelWorld, min: [i32; 3], size: [i32; 3]) -> Grid {
        Grid::new(
            min,
            size,
            crate::region::read_box(w, min, size).expect("loaded"),
        )
    }

    /// Runs a stroke on the world through the server's validation of its region edits.
    fn stroke(
        w: &mut VoxelWorld,
        s: &BrushSpec,
        aim: Aim,
        m: Materials,
    ) -> Vec<([i32; 3], StateId)> {
        let cells = brush_cells(s, aim);
        let (min, size) = read_bounds(s, &cells).unwrap();
        let grid = grid_of(w, min, size);
        let changes = perform(s, aim, &cells, &grid, m);
        for e in paste_edits(&changes) {
            let eye = [f64::from(aim.center[0]), 100.0, f64::from(aim.center[2])];
            let out = check_region_edit(w, &e, eye, &[], 256.0);
            assert_eq!(out.check, RegionCheck::Ok);
            for (c, m) in out.changes {
                w.set_voxel(c[0], c[1], c[2], m);
            }
        }
        changes
    }

    fn world() -> VoxelWorld {
        VoxelWorld::new(empty_generator(), None)
    }

    fn count(w: &mut VoxelWorld, min: [i32; 3], size: [i32; 3], m: StateId) -> usize {
        crate::region::read_box(w, min, size)
            .unwrap()
            .iter()
            .filter(|&&s| s == m)
            .count()
    }

    /// A slab of `m` 1 cell thick at y = 0 over x, z in −r..=r.
    fn floor(w: &mut VoxelWorld, r: i32, m: StateId) {
        for z in -r..=r {
            for x in -r..=r {
                w.set_voxel(x, 0, z, m);
            }
        }
    }

    #[test]
    fn set_replace_erase_and_masks() {
        let (stone, sand, dirt) = (
            require("dwell:stone"),
            require("dwell:sand"),
            require("dwell:dirt"),
        );
        let mut w = world();
        let ball = spec(BrushKind::Ball, PerformerKind::Set, 2);
        let changed = stroke(&mut w, &ball, at([0, 5, 0]), mats(stone));
        assert_eq!(changed.len(), 81);
        assert_eq!(count(&mut w, [-2, 3, -2], [5, 5, 5], stone), 81);
        // Again: nothing changes.
        assert!(stroke(&mut w, &ball, at([0, 5, 0]), mats(stone)).is_empty());
        // Replace: only the target.
        w.set_voxel(0, 5, 0, sand);
        let mut rep = spec(BrushKind::Ball, PerformerKind::Replace, 2);
        let m = Materials {
            material: dirt,
            target: sand,
        };
        assert_eq!(
            stroke(&mut w, &rep, at([0, 5, 0]), m),
            vec![([0, 5, 0], dirt)]
        );
        // A set with the air mask fills around a solid and leaves it.
        rep.performer = PerformerKind::Set;
        rep.params.mask = Mask::Air;
        let around = stroke(&mut w, &rep, at([10, 5, 0]), mats(sand));
        assert_eq!(around.len(), 81);
        w.set_voxel(20, 5, 0, dirt);
        let around = stroke(&mut w, &rep, at([20, 5, 0]), mats(sand));
        assert_eq!(around.len(), 80);
        assert_eq!(w.get_voxel(20, 5, 0), dirt);
        // Erase.
        let erase = spec(BrushKind::Ball, PerformerKind::Erase, 2);
        assert_eq!(stroke(&mut w, &erase, at([0, 5, 0]), mats(stone)).len(), 81);
        assert_eq!(count(&mut w, [-2, 3, -2], [5, 5, 5], M.air), 125);
    }

    #[test]
    fn never_touches_an_unbreakable_block() {
        let stone = require("dwell:stone");
        let floor_block = require("dwell:sandbox_floor");
        assert!(unbreakable(floor_block));
        let mut w = world();
        floor(&mut w, 6, floor_block);
        for performer in PERFORMER_KINDS {
            let s = spec(BrushKind::Ball, performer, 3);
            let changed = stroke(&mut w, &s, at([0, 0, 0]), mats(stone));
            assert!(
                changed.iter().all(|(c, _)| c[1] != 0),
                "{performer:?} changed the floor"
            );
        }
        assert_eq!(count(&mut w, [-6, 0, -6], [13, 1, 13], floor_block), 169);
    }

    #[test]
    fn overlay_paints_the_top_layers_only() {
        let (stone, grass) = (require("dwell:stone"), require("dwell:grass"));
        let mut w = world();
        // A 5 × 3 × 5 block of stone, y 0..=2.
        for y in 0..3 {
            for z in -2..=2 {
                for x in -2..=2 {
                    w.set_voxel(x, y, z, stone);
                }
            }
        }
        let mut s = spec(BrushKind::Ball, PerformerKind::Overlay, 4);
        stroke(&mut w, &s, at([0, 2, 0]), mats(grass));
        assert_eq!(count(&mut w, [-2, 2, -2], [5, 1, 5], grass), 25);
        assert_eq!(count(&mut w, [-2, 0, -2], [5, 2, 5], stone), 50);
        s.params.depth = 2;
        stroke(&mut w, &s, at([0, 2, 0]), mats(require("dwell:sand")));
        assert_eq!(
            count(&mut w, [-2, 1, -2], [5, 2, 5], require("dwell:sand")),
            50
        );
        assert_eq!(count(&mut w, [-2, 0, -2], [5, 1, 5], stone), 25);
    }

    #[test]
    fn smooth_rounds_a_cube_and_fills_a_pit() {
        let stone = require("dwell:stone");
        let mut w = world();
        // A 7³ cube: smoothing takes its edges and corners and keeps its core.
        for y in 0..7 {
            for z in 0..7 {
                for x in 0..7 {
                    w.set_voxel(x, y, z, stone);
                }
            }
        }
        let s = spec(BrushKind::Ball, PerformerKind::Smooth, 6);
        let changed = stroke(&mut w, &s, at([3, 3, 3]), mats(stone));
        assert!(!changed.is_empty());
        assert!(
            changed.iter().all(|&(_, m)| m == M.air),
            "nothing grows on a cube"
        );
        for corner in [[0, 0, 0], [6, 6, 6], [0, 6, 0]] {
            assert_eq!(
                w.get_voxel(corner[0], corner[1], corner[2]),
                M.air,
                "{corner:?}"
            );
        }
        assert_eq!(w.get_voxel(3, 3, 3), stone);
        assert_eq!(w.get_voxel(3, 0, 3), stone, "a face's middle stays");
        // A one-cell pit in a flat slab fills with the slab's block.
        let mut w = world();
        for y in -3..=0 {
            for z in -4..=4 {
                for x in -4..=4 {
                    w.set_voxel(x, y, z, stone);
                }
            }
        }
        w.set_voxel(0, 0, 0, M.air);
        let changed = stroke(
            &mut w,
            &spec(BrushKind::Ball, PerformerKind::Smooth, 2),
            at([0, 0, 0]),
            mats(require("dwell:sand")),
        );
        assert_eq!(changed, vec![([0, 0, 0], stone)]);
    }

    #[test]
    fn erode_strips_edges_and_fill_closes_holes() {
        let stone = require("dwell:stone");
        let mut w = world();
        // A 5³ cube: erosion (2 open faces) takes its 12 edges' cells including the corners.
        for y in 0..5 {
            for z in 0..5 {
                for x in 0..5 {
                    w.set_voxel(x, y, z, stone);
                }
            }
        }
        let mut s = spec(BrushKind::Ball, PerformerKind::Erode, 6);
        s.params.fill_iterations = 0;
        let changed = stroke(&mut w, &s, at([2, 2, 2]), mats(stone));
        assert_eq!(changed.len(), 12 * 3 + 8);
        assert_eq!(count(&mut w, [0, 0, 0], [5, 5, 5], stone), 125 - 44);
        // Fill (inflate): a 3-cell notch in a wall fills where 2 faces are solid.
        let mut w = world();
        for y in 0..5 {
            for x in 0..5 {
                w.set_voxel(x, y, 0, stone);
            }
        }
        w.set_voxel(2, 2, 0, M.air);
        let mut fill = spec(BrushKind::Ball, PerformerKind::Erode, 2);
        fill.params = Params {
            erode_iterations: 0,
            fill_faces: 4,
            fill_iterations: 1,
            ..Params::default()
        };
        assert_eq!(
            stroke(&mut w, &fill, at([2, 2, 0]), mats(require("dwell:sand"))),
            vec![([2, 2, 0], stone)]
        );
        // Inflate (fill with 2 faces) thickens the wall.
        fill.params.fill_faces = 1;
        let grown = stroke(&mut w, &fill, at([2, 2, 0]), mats(require("dwell:sand")));
        assert!(grown.iter().all(|(c, m)| c[2] != 0 && *m == stone));
        assert!(!grown.is_empty());
    }

    #[test]
    fn flatten_raise_and_lower_work_on_columns() {
        let (stone, grass) = (require("dwell:stone"), require("dwell:grass"));
        let mut w = world();
        floor(&mut w, 8, stone);
        // A bump at the centre and a hole beside it.
        w.set_voxel(0, 1, 0, stone);
        w.set_voxel(0, 2, 0, stone);
        w.set_voxel(2, 0, 0, M.air);
        let mut f = spec(BrushKind::Cylinder, PerformerKind::Flatten, 3);
        f.params.height = 4;
        let changed = stroke(&mut w, &f, at([0, 0, 0]), mats(grass));
        assert_eq!(
            changed
                .iter()
                .filter(|(c, m)| *m == M.air && c[1] > 0)
                .count(),
            2
        );
        assert_eq!(w.get_voxel(2, 0, 0), grass);
        assert_eq!(w.get_voxel(0, 1, 0), M.air);
        // Raise: 3 cells on every column of a disc (no falloff), of the column's top block.
        let mut r = spec(BrushKind::Disc, PerformerKind::Raise, 2);
        r.params.height = 3;
        let raised = stroke(&mut w, &r, at([0, 0, 0]), mats(grass));
        assert_eq!(raised.len(), 21 * 3);
        assert_eq!(w.get_voxel(1, 3, 1), stone);
        assert_eq!(w.get_voxel(2, 3, 0), grass, "the filled hole's grass grows");
        // With falloff the middle rises most: a mound.
        let mut w2 = world();
        floor(&mut w2, 8, stone);
        r.size = 4;
        r.params.falloff = true;
        stroke(&mut w2, &r, at([0, 0, 0]), mats(grass));
        assert_eq!(w2.get_voxel(0, 3, 0), stone);
        assert_eq!(w2.get_voxel(4, 1, 0), stone);
        assert_eq!(w2.get_voxel(4, 2, 0), M.air);
        // Lower takes the raise back off.
        r.size = 2;
        r.params.falloff = false;
        r.performer = PerformerKind::Lower;
        stroke(&mut w, &r, at([0, 3, 0]), mats(grass));
        assert_eq!(count(&mut w, [-2, 1, -2], [5, 3, 5], M.air), 75);
    }

    #[test]
    fn large_strokes_split_into_region_edits_of_at_most_32_a_side() {
        let stone = require("dwell:stone");
        let big = spec(BrushKind::Ball, PerformerKind::Set, MAX_SIZE);
        let cells = brush_cells(&big, at([0, 0, 0]));
        let changes: Vec<([i32; 3], StateId)> = cells.iter().map(|&c| (c, stone)).collect();
        let edits = paste_edits(&changes);
        assert!(edits.len() >= 2 && edits.len() <= 8, "{}", edits.len());
        let mut covered = 0;
        for e in &edits {
            assert!(
                e.size
                    .iter()
                    .all(|&s| s >= 1 && usize::from(s) <= region_edit::MAX_SIDE)
            );
            assert_eq!(e.cells.len(), e.volume());
            covered += e.cells.iter().filter(|&&m| m != KEEP).count();
        }
        assert_eq!(covered, cells.len());
        // Applied through the server's validation, they make the ball.
        let mut w = world();
        for e in &edits {
            let out = check_region_edit(&mut w, e, [0.0, 0.0, 0.0], &[], 64.0);
            assert_eq!(out.check, RegionCheck::Ok);
            for (c, m) in out.changes {
                w.set_voxel(c[0], c[1], c[2], m);
            }
        }
        assert_eq!(
            count(&mut w, [-16, -16, -16], [33, 33, 33], stone),
            cells.len()
        );
        // A small stroke is one edit trimmed to its cells.
        let one = paste_edits(&[([5, 6, 7], stone), ([6, 6, 7], stone)]);
        assert_eq!(
            (one.len(), one[0].min, one[0].size),
            (1, [5, 6, 7], [2, 1, 1])
        );
        assert!(paste_edits(&[]).is_empty());
    }

    #[test]
    fn the_preview_is_the_result() {
        // What the tool outlines (brush_cells) is what a set changes, for every brush.
        let stone = require("dwell:stone");
        for brush in BRUSH_KINDS {
            let mut s = spec(brush, PerformerKind::Set, 3);
            s.params.seed = 5;
            s.params.height = 3;
            s.params.structures = &["dwell:proto_tree"];
            let aim = Aim {
                from: Some([0, 2, 0]),
                before: Some([8, 3, 1]),
                ..Aim::at([4, 4, 4])
            };
            let mut w = world();
            let preview = brush_cells(&s, aim);
            let changed: Vec<[i32; 3]> = stroke(&mut w, &s, aim, mats(stone))
                .iter()
                .map(|c| c.0)
                .collect();
            assert_eq!(changed, preview, "{brush:?}");
        }
    }

    #[test]
    fn the_new_shapes_cover_what_they_say() {
        let c = [3, 7, -2];
        let set = PerformerKind::Set;
        let cells = |s: &BrushSpec, aim: Aim| brush_cells(s, aim);
        // A square is a cube's middle layer; a ring is the disc's edge.
        let square = cells(&spec(BrushKind::Square, set, 2), at(c));
        assert_eq!(square.len(), 25);
        assert!(square.iter().all(|p| p[1] == c[1]));
        let disc = cells(&spec(BrushKind::Disc, set, 5), at(c));
        let mut ring = spec(BrushKind::Ring, set, 5);
        ring.params.width = 2;
        let ring = cells(&ring, at(c));
        assert!(ring.iter().all(|p| disc.contains(p)));
        assert!(!ring.contains(&c) && ring.contains(&[c[0] + 5, c[1], c[2]]));
        assert!(ring.len() < disc.len() * 3 / 4);
        // On a wall (+X), a disc on the face stands upright across it.
        let wall = Aim { face: 0, ..at(c) };
        let upright = cells(&spec(BrushKind::DiscFace, set, 3), wall);
        assert_eq!(upright.len(), 37);
        assert!(upright.iter().all(|p| p[0] == c[0]));
        assert_eq!(
            cells(&spec(BrushKind::DiscFace, set, 3), at(c)),
            cells(&spec(BrushKind::Disc, set, 3), at(c))
        );
        assert_eq!(cells(&spec(BrushKind::SquareFace, set, 2), wall).len(), 25);
        let mut ellipse = spec(BrushKind::Ellipse, set, 2);
        ellipse.params.stretch = [2.0, 1.0, 1.0];
        let (_, size) = bounds(&cells(&ellipse, at(c))).unwrap();
        assert_eq!(size, [11, 1, 5]);
        // A dome is the top half of its ellipsoid; a snipe is one cell.
        let ball = cells(&spec(BrushKind::Ball, set, 3), at(c));
        let dome = cells(&spec(BrushKind::Dome, set, 3), at(c));
        assert_eq!(
            dome,
            ball.iter()
                .copied()
                .filter(|p| p[1] >= c[1])
                .collect::<Vec<_>>()
        );
        assert_eq!(cells(&spec(BrushKind::Snipe, set, 9), at(c)), vec![c]);
        // A blob grows from the centre within the ball, seeded.
        let mut blob = spec(BrushKind::Blob, set, 5);
        blob.params.growth = 0.5;
        blob.params.seed = 3;
        let b = cells(&blob, at(c));
        assert!(b.contains(&c) && b.len() > 1);
        let big = cells(&spec(BrushKind::Ball, set, 5), at(c));
        assert!(b.iter().all(|p| big.contains(p)) && b.len() < big.len());
        assert_eq!(b, cells(&blob, at(c)));
        // Density thins any brush; checker keeps alternate cells.
        let mut thin = spec(BrushKind::Cube, set, 4);
        thin.params.density = 0.3;
        let n = cells(&thin, at(c)).len() as f64 / 729.0;
        assert!((n - 0.3).abs() < 0.06, "{n}");
        let mut checker = spec(BrushKind::Square, set, 2);
        checker.params.checker = true;
        let ch = cells(&checker, at([0, 0, 0]));
        assert_eq!(ch.len(), 13);
        assert!(ch.iter().all(|p| (p[0] + p[1] + p[2]).rem_euclid(2) == 0));
    }

    #[test]
    fn triangles_and_jagged_lines_join_their_clicks() {
        let set = PerformerKind::Set;
        let aim = Aim {
            before: Some([0, 0, 0]),
            from: Some([10, 0, 0]),
            ..Aim::at([0, 0, 10])
        };
        let tri = brush_cells(&spec(BrushKind::Triangle, set, 1), aim);
        assert!(tri.iter().all(|p| p[1] == 0));
        for corner in [[0, 0, 0], [10, 0, 0], [0, 0, 10], [3, 0, 3]] {
            assert!(tri.contains(&corner), "{corner:?}");
        }
        assert!(!tri.contains(&[8, 0, 8]));
        let mut jag = spec(BrushKind::JaggedLine, set, 1);
        jag.params.jitter = 3;
        jag.params.seed = 1;
        let aim = Aim {
            from: Some([0, 0, 0]),
            ..Aim::at([30, 0, 0])
        };
        let j = brush_cells(&jag, aim);
        assert!(j.contains(&[0, 0, 0]) && j.contains(&[30, 0, 0]));
        assert!(j.iter().any(|p| p[1] != 0 || p[2] != 0), "it wanders");
        assert!(j.iter().all(|p| p[1].abs() <= 3 && p[2].abs() <= 3));
        for p in &j {
            assert!(
                j.iter()
                    .any(|q| q != p && (0..3).all(|i| (q[i] - p[i]).abs() <= 1)),
                "{p:?} is alone"
            );
        }
    }

    fn copy_box(from: &mut VoxelWorld, r: i32) -> VoxelWorld {
        let mut w = world();
        for y in -r..=r {
            for z in -r..=r {
                for x in -r..=r {
                    w.set_voxel(x, y, z, from.get_voxel(x, y, z));
                }
            }
        }
        w
    }

    #[test]
    fn blend_takes_the_commonest_neighbour() {
        let (stone, sand) = (require("dwell:stone"), require("dwell:sand"));
        let mut w = world();
        // A stone slab with a sand speck and a pit: blending returns both to stone.
        for y in -2..=0 {
            for z in -4..=4 {
                for x in -4..=4 {
                    w.set_voxel(x, y, z, stone);
                }
            }
        }
        w.set_voxel(0, -1, 0, sand);
        w.set_voxel(1, -1, 0, M.air);
        let s = spec(BrushKind::Ball, PerformerKind::Blend, 2);
        stroke(&mut w, &s, at([0, -1, 0]), mats(sand));
        assert_eq!(w.get_voxel(0, -1, 0), stone);
        assert_eq!(w.get_voxel(1, -1, 0), stone);
        // A tie changes nothing.
        let around: Vec<StateId> = [stone; 13].into_iter().chain([sand; 13]).collect();
        assert_eq!(blend_pick(&around, &Params::default()), None);
        // Air wins unless excluded.
        let mostly_air: Vec<StateId> = [M.air; 20].into_iter().chain([stone; 6]).collect();
        assert_eq!(blend_pick(&mostly_air, &Params::default()), Some(M.air));
        let solid_only = Params {
            exclude_air: true,
            ..Params::default()
        };
        assert_eq!(blend_pick(&mostly_air, &solid_only), Some(stone));
        // A linear blend changes fewer cells than a blend.
        let mut noisy = world();
        for y in -6i32..=6 {
            for z in -6..=6 {
                for x in -6..=6 {
                    let m = if (x * 7 + y * 3 + z * 5).rem_euclid(4) == 0 {
                        sand
                    } else {
                        stone
                    };
                    noisy.set_voxel(x, y, z, m);
                }
            }
        }
        let mut copy = copy_box(&mut noisy, 6);
        let full = stroke(
            &mut noisy,
            &spec(BrushKind::Ball, PerformerKind::Blend, 5),
            at([0; 3]),
            mats(sand),
        );
        let mut lin = spec(BrushKind::Ball, PerformerKind::LinearBlend, 5);
        lin.params.seed = 2;
        let linear = stroke(&mut copy, &lin, at([0; 3]), mats(sand));
        assert!(
            !linear.is_empty() && linear.len() < full.len(),
            "{} {}",
            linear.len(),
            full.len()
        );
    }

    #[test]
    fn underlay_cover_hollow_fill_down_and_extrude() {
        let (stone, grass, sand) = (
            require("dwell:stone"),
            require("dwell:grass"),
            require("dwell:sand"),
        );
        // A floating 5 × 3 × 5 slab, y 0..=2.
        let mut w = world();
        for y in 0..3 {
            for z in -2..=2 {
                for x in -2..=2 {
                    w.set_voxel(x, y, z, stone);
                }
            }
        }
        stroke(
            &mut w,
            &spec(BrushKind::Ball, PerformerKind::Underlay, 4),
            at([0, 0, 0]),
            mats(sand),
        );
        assert_eq!(count(&mut w, [-2, 0, -2], [5, 1, 5], sand), 25);
        assert_eq!(count(&mut w, [-2, 1, -2], [5, 2, 5], stone), 50);
        // Cover: a new layer on top, in the air.
        let mut cover = spec(BrushKind::Ball, PerformerKind::Overlay, 4);
        cover.params.on_top = true;
        stroke(&mut w, &cover, at([0, 3, 0]), mats(grass));
        assert_eq!(count(&mut w, [-2, 3, -2], [5, 1, 5], grass), 25);
        assert_eq!(w.get_voxel(0, 4, 0), M.air);
        // Hollow: a 7³ cube keeps its one-cell shell.
        let mut w = world();
        for y in 0..7 {
            for z in 0..7 {
                for x in 0..7 {
                    w.set_voxel(x, y, z, stone);
                }
            }
        }
        let shell = stroke(
            &mut w,
            &spec(BrushKind::Cube, PerformerKind::Hollow, 4),
            at([3, 3, 3]),
            mats(sand),
        );
        assert_eq!(shell.len(), 125);
        assert_eq!(count(&mut w, [0, 0, 0], [7, 7, 7], stone), 343 - 125);
        // Fill down: from the disc to the ground.
        let mut w = world();
        floor(&mut w, 4, stone);
        let mut fd = spec(BrushKind::Disc, PerformerKind::FillDown, 1);
        fd.params.height = 16;
        stroke(&mut w, &fd, at([0, 5, 0]), mats(sand));
        assert_eq!(count(&mut w, [0, 1, 0], [1, 5, 1], sand), 5);
        assert_eq!(w.get_voxel(0, 0, 0), stone);
        // Extrude: a wall's face pushed out by 2 along the aimed-at face's normal (+X).
        let mut w = world();
        for y in 0..3 {
            for z in 0..3 {
                w.set_voxel(0, y, z, stone);
            }
        }
        let mut ex = spec(BrushKind::SquareFace, PerformerKind::Extrude, 1);
        ex.params.height = 2;
        let pushed = stroke(
            &mut w,
            &ex,
            Aim {
                face: 0,
                ..at([0, 1, 1])
            },
            mats(sand),
        );
        assert_eq!(pushed.len(), 18);
        assert!(
            pushed
                .iter()
                .all(|(c, m)| (c[0] == 1 || c[0] == 2) && *m == stone)
        );
    }

    #[test]
    fn masks_exclude_the_target_and_drain_liquids() {
        let (stone, sand, water) = (
            require("dwell:stone"),
            require("dwell:sand"),
            require("dwell:water"),
        );
        let mut w = world();
        w.set_voxel(0, 0, 0, sand);
        let mut not = spec(BrushKind::Ball, PerformerKind::Set, 1);
        not.params.mask = Mask::NotTarget;
        let m = Materials {
            material: stone,
            target: sand,
        };
        stroke(&mut w, &not, at([0, 0, 0]), m);
        assert_eq!(w.get_voxel(0, 0, 0), sand, "the target is left");
        assert_eq!(w.get_voxel(0, 1, 0), stone);
        let mut w = world();
        w.set_voxel(1, 0, 0, water);
        w.set_voxel(-1, 0, 0, stone);
        let mut drain = spec(BrushKind::Ball, PerformerKind::Erase, 2);
        drain.params.mask = Mask::Liquid;
        assert_eq!(
            stroke(&mut w, &drain, at([0, 0, 0]), mats(stone)),
            vec![([1, 0, 0], M.air)]
        );
        assert_eq!(w.get_voxel(-1, 0, 0), stone);
    }

    #[test]
    fn stencils_stamp_a_structure_and_trees_grow_of_their_blocks() {
        let (log, leaves, stone) = (
            require("dwell:log"),
            require("dwell:leaves"),
            require("dwell:stone"),
        );
        let mut s = spec(BrushKind::Stencil, PerformerKind::Stamp, 1);
        s.params.structures = &["dwell:proto_tree"];
        let tree = crate::structure::load("dwell:proto_tree").unwrap();
        let mut w = world();
        let changed = stroke(&mut w, &s, at([0, 1, 0]), mats(require("dwell:sand")));
        // The structure's cells, its anchor (the trunk's foot) on the centre.
        assert_eq!(changed.len(), tree.place([0, 1, 0], 0).cells().count());
        assert_eq!(w.get_voxel(0, 1, 0), log);
        assert!(changed.iter().any(|&(_, m)| m == leaves));
        // A stencil list picks among its structures by the aim.
        s.params.structures = &["dwell:proto_tree", "dwell:proto_rock"];
        let picks: std::collections::HashSet<usize> = (0..12)
            .map(|i| brush_blocks(&s, at([i * 10, 0, 0]), 0).len())
            .collect();
        assert!(picks.len() >= 2, "{picks:?}");
        // The fill mode (air mask) stamps only into open cells.
        let mut w = world();
        w.set_voxel(0, 4, 0, stone);
        s.params.structures = &["dwell:proto_tree"];
        s.params.mask = Mask::Air;
        stroke(&mut w, &s, at([0, 1, 0]), mats(0));
        assert_eq!(w.get_voxel(0, 4, 0), stone);
        // A tree: a trunk of wood from the centre up, leaves round its crown, seeded.
        let mut t = spec(BrushKind::Tree, PerformerKind::Stamp, 3);
        t.params = Params {
            wood: "dwell:log",
            leaves: "dwell:leaves",
            height: 8,
            branches: 3,
            branch_length: 4,
            roots: 2,
            seed: 4,
            ..Params::default()
        };
        let blocks = brush_blocks(&t, at([0, 0, 0]), 0);
        assert_eq!(blocks, brush_blocks(&t, at([0, 0, 0]), 0));
        for y in 0..8 {
            assert!(blocks.contains(&([0, y, 0], log)), "trunk {y}");
        }
        assert!(blocks.iter().any(|&(c, m)| m == leaves && c[1] >= 8));
        assert!(blocks.iter().any(|&(c, m)| m == log && c[1] < 0), "roots");
        // A tree's blocks named "" are the hotbar's.
        t.params.wood = "";
        let sand = require("dwell:sand");
        assert!(brush_blocks(&t, at([0, 0, 0]), sand).contains(&([0, 0, 0], sand)));
    }

    #[test]
    fn presets_load_and_every_one_strokes() {
        assert_eq!(FORMAT_VERSION, 1);
        assert!(!PRESETS.is_empty());
        let mut names: Vec<&str> = PRESETS.iter().map(|p| p.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), PRESETS.len());
        for name in [
            "ball", "disc", "smooth", "erode", "splatter", "overlay", "inflate",
        ] {
            assert!(preset(name).is_some(), "{name}");
        }
        let stone = require("dwell:stone");
        for p in PRESETS {
            assert!((MIN_SIZE..=MAX_SIZE).contains(&p.size), "{}", p.name);
            let mut w = world();
            floor(&mut w, 20, stone);
            let aim = Aim {
                from: Some([-5, 0, 0]),
                ..Aim::at([0, 0, 0])
            };
            let cells = brush_cells(&p.spec(), aim);
            assert!(!cells.is_empty(), "{}", p.name);
            stroke(&mut w, &p.spec(), aim, mats(require("dwell:sand")));
        }
    }

    #[test]
    fn a_preset_added_by_data_alone_works() {
        // A fixture preset: nothing but data (the shape gen.mjs writes into brushes.gen.rs).
        let fixture = Preset {
            name: "fixture_tall_cube",
            label: "Tall cube",
            brush: BrushKind::Ellipsoid,
            performer: PerformerKind::Set,
            size: 2,
            key: None,
            params: Params {
                stretch: [1.0, 2.0, 1.0],
                ..Params::default()
            },
        };
        let mut w = world();
        let sand = require("dwell:sand");
        let changed = stroke(&mut w, &fixture.spec(), at([0, 10, 0]), mats(sand));
        let (_, size) = bounds(&changed.iter().map(|c| c.0).collect::<Vec<_>>()).unwrap();
        assert_eq!(size, [5, 11, 5]);
    }

    #[test]
    fn brushes_are_for_the_world_types_the_data_names() {
        let sandbox = brush_policy(crate::GENERATOR_SANDBOX).expect("the sandbox allows brushes");
        assert_eq!(sandbox.world_type, "sandbox");
        assert!(sandbox.reach >= 128.0);
        assert_eq!(brush_policy(crate::GENERATOR_TERRAIN), None);
        assert_eq!(brush_policy(crate::GENERATOR_FLAT), None);
    }
}
