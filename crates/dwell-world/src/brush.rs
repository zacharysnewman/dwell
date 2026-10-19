//! Brushes (docs/BRUSHES.md; VoxelSniper-style painting and sculpting): a **brush** is the set of
//! cells a stroke covers (ball, disc, cylinder, cube, ellipsoid, splatter, line), a **performer** is
//! what happens to them (set, replace, overlay, erase, and the sculpt performers smooth, erode,
//! flatten, raise, lower). Both are a small fixed vocabulary here; the named presets players pick
//! are data (`shared/brushes/brushes.json` → `brushes.gen.rs`), as is which world types allow
//! brushes and how far they reach.
//!
//! Everything is a pure function of the brush, the aim and a grid of the world's states, so the
//! preview, the stroke and the tests agree (splatter is seeded). A stroke's result is a list of
//! changed cells, sent as `RegionEdit` pastes of at most 32³ whose other cells are
//! `region_edit::KEEP` ([`paste_edits`]); the server validates them like any region edit.

use dwell_blocks::StateId;
use dwell_protocol::{RegionEdit, RegionEditOp, region_edit};

use crate::M;
use crate::edit::{flooded_for, placeable, unbreakable};

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
    /// A ball with a seeded `density` of its cells.
    Splatter,
    /// From the line's start (the previous click) to the aimed-at cell, `size` cells thick.
    Line,
}

pub const BRUSH_KINDS: [BrushKind; 7] = [
    BrushKind::Ball,
    BrushKind::Disc,
    BrushKind::Cylinder,
    BrushKind::Cube,
    BrushKind::Ellipsoid,
    BrushKind::Splatter,
    BrushKind::Line,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PerformerKind {
    /// The material in every cell (that the mask lets through).
    Set,
    /// The material in cells holding the replace target.
    Replace,
    /// The material on the top `depth` layers of solid ground: cells with open space above.
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
}

pub const PERFORMER_KINDS: [PerformerKind; 9] = [
    PerformerKind::Set,
    PerformerKind::Replace,
    PerformerKind::Overlay,
    PerformerKind::Erase,
    PerformerKind::Smooth,
    PerformerKind::Erode,
    PerformerKind::Flatten,
    PerformerKind::Raise,
    PerformerKind::Lower,
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
        }
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
        }
    }

    /// Acts on whole columns under the brush's footprint, not on its cells.
    pub fn by_column(self) -> bool {
        matches!(self, Self::Flatten | Self::Raise | Self::Lower)
    }

    /// Uses the replace target (picked with a middle or Alt click).
    pub fn uses_target(self, mask: Mask) -> bool {
        self == Self::Replace || mask == Mask::Target
    }
}

/// Which cells a set, erase or overlay may change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mask {
    None,
    /// Only non-solid cells (air, water).
    Air,
    /// Only solid cells.
    Solid,
    /// Only cells holding the replace target.
    Target,
}

/// A preset's parameters; each brush and performer reads the ones it takes (`gen.mjs` says which).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    pub height: u8,
    pub stretch: [f32; 3],
    pub density: f32,
    pub seed: u32,
    pub depth: u8,
    pub mask: Mask,
    pub iterations: u8,
    pub erode_faces: u8,
    pub erode_iterations: u8,
    pub fill_faces: u8,
    pub fill_iterations: u8,
    pub falloff: bool,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            height: 1,
            stretch: [1.0; 3],
            density: 0.5,
            seed: 0,
            depth: 1,
            mask: Mask::None,
            iterations: 1,
            erode_faces: 2,
            erode_iterations: 1,
            fill_faces: 5,
            fill_iterations: 1,
            falloff: false,
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

/// Where a stroke goes: the cell it is centred on, and a line's start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Aim {
    pub center: [i32; 3],
    pub from: Option<[i32; 3]>,
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

/// The cells a brush covers, in layer order (y, then z, then x) without repeats. Splatter's cells
/// are seeded by the preset's seed and the centre, so the same stroke picks the same cells.
pub fn brush_cells(spec: &BrushSpec, aim: Aim) -> Vec<[i32; 3]> {
    let r = i32::from(spec.size.clamp(MIN_SIZE, MAX_SIZE));
    let c = aim.center;
    let rr = (f64::from(r) + 0.5).powi(2);
    let mut out = Vec::new();
    let mut push = |o: [i32; 3]| out.push([c[0] + o[0], c[1] + o[1], c[2] + o[2]]);
    match spec.brush {
        BrushKind::Ball | BrushKind::Splatter => {
            let seed = cell_hash(u64::from(spec.params.seed), c);
            let threshold =
                (f64::from(spec.params.density.clamp(0.0, 1.0)) * u32::MAX as f64) as u64;
            for y in -r..=r {
                for z in -r..=r {
                    for x in -r..=r {
                        if f64::from(x * x + y * y + z * z) > rr {
                            continue;
                        }
                        if spec.brush == BrushKind::Splatter
                            && cell_hash(seed, [x, y, z]) & 0xffff_ffff > threshold
                        {
                            continue;
                        }
                        push([x, y, z]);
                    }
                }
            }
        }
        BrushKind::Ellipsoid => {
            let axes = spec
                .params
                .stretch
                .map(|s| (f64::from(r) + 0.5) * f64::from(s));
            let ext = axes.map(|a| (a - 0.5).ceil().max(0.0) as i32);
            for y in -ext[1]..=ext[1] {
                for z in -ext[2]..=ext[2] {
                    for x in -ext[0]..=ext[0] {
                        let d = [x, y, z]
                            .iter()
                            .zip(&axes)
                            .map(|(&o, &a)| (f64::from(o) / a).powi(2))
                            .sum::<f64>();
                        if d <= 1.0 {
                            push([x, y, z]);
                        }
                    }
                }
            }
        }
        BrushKind::Disc | BrushKind::Cylinder => {
            let h = if spec.brush == BrushKind::Disc {
                1
            } else {
                i32::from(spec.params.height.max(1))
            };
            for y in 0..h {
                for z in -r..=r {
                    for x in -r..=r {
                        if f64::from(x * x + z * z) <= rr {
                            push([x, y, z]);
                        }
                    }
                }
            }
        }
        BrushKind::Cube => {
            for y in -r..=r {
                for z in -r..=r {
                    for x in -r..=r {
                        push([x, y, z]);
                    }
                }
            }
        }
        BrushKind::Line => {
            let from = aim.from.unwrap_or(c);
            let d: [f64; 3] = std::array::from_fn(|i| f64::from(from[i] - c[i]));
            let len2 = d.iter().map(|v| v * v).sum::<f64>();
            // `size` 1 is a line one cell thick.
            let t = f64::from(r - 1) + 0.6;
            let lo: [i32; 3] = std::array::from_fn(|i| (from[i] - c[i]).min(0) - r);
            let hi: [i32; 3] = std::array::from_fn(|i| (from[i] - c[i]).max(0) + r);
            for y in lo[1]..=hi[1] {
                for z in lo[2]..=hi[2] {
                    for x in lo[0]..=hi[0] {
                        let p = [f64::from(x), f64::from(y), f64::from(z)];
                        let k = if len2 > 0.0 {
                            (p.iter().zip(&d).map(|(a, b)| a * b).sum::<f64>() / len2)
                                .clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        let dist2 = (0..3).map(|i| (p[i] - k * d[i]).powi(2)).sum::<f64>();
                        if dist2 <= t * t {
                            push([x, y, z]);
                        }
                    }
                }
            }
        }
    }
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
        PerformerKind::Set | PerformerKind::Replace | PerformerKind::Erase => (0, 0, 0),
        PerformerKind::Overlay => (0, i32::from(p.depth), 0),
        PerformerKind::Smooth | PerformerKind::Erode => (1, 1, 1),
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
    }
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
        Aim {
            center: c,
            from: None,
        }
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
                center: [10, 0, 0],
                from: Some([0, 0, 0]),
            },
        );
        assert_eq!(line.len(), 11);
        assert!(line.iter().all(|p| p[1] == 0 && p[2] == 0));
        // A diagonal is connected: each cell has a neighbour (26-connectivity) further on.
        let diag = brush_cells(
            &s,
            Aim {
                center: [7, 3, -5],
                from: Some([0, 0, 0]),
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
                center: [10, 0, 0],
                from: Some([0, 0, 0]),
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
            let aim = Aim {
                center: [4, 4, 4],
                from: Some([0, 2, 0]),
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
                center: [0, 0, 0],
                from: Some([-5, 0, 0]),
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
