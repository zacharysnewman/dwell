//! Ground items (docs/adr/0071-ground-items.md): sticks under the trees, rocks at the cliffs' feet,
//! small things lying on the ground that a player picks up. They are not saved terrain: a chunk's
//! are scattered from its blocks as it loads, deterministically from the world seed and the cell
//! ([`item_at`], [`scatter_chunk`]), by `shared/ground/ground.json`'s rules (`ground.gen.rs`). The
//! picked-up ones are kept gone ([`GroundGone`], saved with the world, like tree tombstones in
//! docs/TREE_GROWTH.md §5a). The server, the client's crosshair and the client's mesh jobs all
//! read the same function, so they agree on what lies where.
//!
//! Face-local throughout: a cell's ground is the cell under it on its face, its canopy above, and
//! the roll hashes the face-local cell, so face B's items are face A's mirror image.

use crate::hash::HashSet;
use crate::{CHUNK_SIZE, ChunkCoord, FaceFrame, m_of_row};
use dwell_blocks::{StateId, VoxelShape, find_block, try_info};

/// A kind's mesh: its item's look's shape (`shared/inventory` `look`, one look per item).
pub use dwell_protocol::inventory::LookShape as GroundShape;

/// A ground item's kind (`ground.json` `kinds`); its look (shape, size, texture, colour) is copied
/// from its item's `look` by `shared/ground/gen.mjs`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroundKind {
    pub name: &'static str,
    /// The item picking it up gives (an inventory item, the same name as a drop's).
    pub item: &'static str,
    pub shape: GroundShape,
    /// Metres, before `scale`: x (its length), y (up), z.
    pub size: [f64; 3],
    /// The block whose side tile it is drawn with.
    pub texture: &'static str,
    /// sRGB tint on the texture.
    pub color: [u8; 3],
    /// Each one's scale is in `[min, max]`.
    pub scale: [f64; 2],
}

/// A rule's canopy condition: one of `blocks` within `within_rows` rows above the cell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Under {
    pub blocks: &'static [&'static str],
    pub within_rows: i32,
}

/// A rule's cliff condition: a column `1..=reach` cells away along a compass direction is solid
/// for `rows` rows up from the cell's own row (a vertical face at least that tall).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NearWall {
    pub rows: i32,
    pub reach: i32,
}

/// A scatter rule (`ground.json` `rules`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroundRule {
    /// Index into [`KINDS`].
    pub kind: usize,
    /// The full blocks it lies on (the cell under it).
    pub on: &'static [&'static str],
    pub under: Option<Under>,
    pub near_wall: Option<NearWall>,
    /// The share of the cells passing its conditions that get one.
    pub density: f64,
}

// Included (not a `#[path]` module) so rustfmt leaves the generated file as written.
mod ground_generated {
    include!("ground.gen.rs");
}
pub use ground_generated::{KINDS, RULES};

/// One ground item: in `cell` (world), resting on the cell under it (face-local).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub cell: [i32; 3],
    /// Index into the kinds it was scattered from.
    pub kind: usize,
    /// Turn about up (radians), face-local.
    pub yaw: f64,
    pub scale: f64,
    /// Its centre's offset from the cell's centre, x and z (m).
    pub offset: [f64; 2],
}

impl Placed {
    /// Where it rests (world m): the middle of its footprint, on the ground's top.
    pub fn point(&self) -> [f64; 3] {
        let f = FaceFrame::at_row(self.cell[1]);
        let l = f.cell(self.cell);
        f.point([
            f64::from(l[0]) + 0.5 + self.offset[0],
            m_of_row(l[1]),
            f64::from(l[2]) + 0.5 + self.offset[1],
        ])
    }
}

/// A rule set with its block names resolved to state ranges (once per scatter, as the registry a
/// client syncs may grow).
pub struct Rules<'a> {
    kinds: &'a [GroundKind],
    rules: Vec<(&'a GroundRule, StateRanges, StateRanges)>,
}

/// State id ranges (a block's states each).
type StateRanges = Vec<std::ops::Range<usize>>;

fn ranges(names: &[&str]) -> StateRanges {
    names
        .iter()
        .filter_map(|n| find_block(n))
        .map(|b| b.states())
        .collect()
}

fn among(m: StateId, r: &[std::ops::Range<usize>]) -> bool {
    r.iter().any(|r| r.contains(&usize::from(m)))
}

fn solid(m: StateId) -> bool {
    try_info(m).is_some_and(|i| i.solid)
}

/// Whether a cell can hold a ground item: open (air, or a plant one walks through), no liquid.
fn open(m: StateId) -> bool {
    try_info(m).is_some_and(|i| !i.solid && !i.liquid)
}

/// Whether a cell is ground an item lies flat on: a solid full block.
fn flat_ground(m: StateId) -> bool {
    try_info(m).is_some_and(|i| i.solid && i.shape == VoxelShape::Full)
}

const COMPASS: [[i32; 2]; 4] = [[1, 0], [-1, 0], [0, 1], [0, -1]];

impl<'a> Rules<'a> {
    /// The shipped data's rules.
    pub fn shipped() -> Rules<'static> {
        Rules::new(&KINDS, &RULES)
    }

    pub fn new(kinds: &'a [GroundKind], rules: &'a [GroundRule]) -> Self {
        Self {
            kinds,
            rules: rules
                .iter()
                .map(|r| {
                    (
                        r,
                        ranges(r.on),
                        r.under.map_or(Vec::new(), |u| ranges(u.blocks)),
                    )
                })
                .collect(),
        }
    }

    pub fn kind(&self, i: usize) -> Option<&'a GroundKind> {
        self.kinds.get(i)
    }

    /// The ground item in world cell `cell`, if any: `get` reads a world cell (None: not known,
    /// and a rule that needs it does not hold).
    pub fn item_at(
        &self,
        get: &mut impl FnMut([i32; 3]) -> Option<StateId>,
        seed: u64,
        cell: [i32; 3],
    ) -> Option<Placed> {
        let f = FaceFrame::at_row(cell[1]);
        let [x, y, z] = f.cell(cell);
        let mut at = |dx: i32, dy: i32, dz: i32| get(f.cell([x + dx, y + dy, z + dz]));
        if !open(at(0, 0, 0)?) {
            return None;
        }
        let below = at(0, -1, 0)?;
        if !flat_ground(below) {
            return None;
        }
        for (i, (rule, on, under)) in self.rules.iter().enumerate() {
            if !among(below, on) {
                continue;
            }
            if let Some(u) = rule.under
                && !(1..=u.within_rows).any(|k| at(0, k, 0).is_some_and(|m| among(m, under)))
            {
                continue;
            }
            if let Some(w) = rule.near_wall
                && !COMPASS.iter().any(|[dx, dz]| {
                    (1..=w.reach)
                        .any(|d| (0..w.rows).all(|k| at(dx * d, k, dz * d).is_some_and(solid)))
                })
            {
                continue;
            }
            // The roll and the look hash the face-local cell: face B mirrors face A.
            let h = |salt: u64| unit(mix(seed, [x, y, z], salt));
            if h(i as u64) >= rule.density {
                continue;
            }
            let kind = self.kinds.get(rule.kind)?;
            let [lo, hi] = kind.scale;
            let scale = lo + (hi - lo) * h(101);
            // Kept inside its cell (its longest half, scaled, from the centre).
            let room = (0.5 - kind.size[0].max(kind.size[2]) * scale / 2.0).max(0.0);
            return Some(Placed {
                cell,
                kind: rule.kind,
                yaw: h(102) * std::f64::consts::TAU,
                scale,
                offset: [room * (2.0 * h(103) - 1.0), room * (2.0 * h(104) - 1.0)],
            });
        }
        None
    }

    /// Every ground item in chunk `c`, in cell order (x fastest, then z, then y).
    pub fn scatter_chunk(
        &self,
        get: &mut impl FnMut([i32; 3]) -> Option<StateId>,
        seed: u64,
        c: ChunkCoord,
    ) -> Vec<Placed> {
        let o = [c.x * CHUNK_SIZE, c.y * CHUNK_SIZE, c.z * CHUNK_SIZE];
        let mut out = Vec::new();
        for ly in 0..CHUNK_SIZE {
            for lz in 0..CHUNK_SIZE {
                for lx in 0..CHUNK_SIZE {
                    let cell = [o[0] + lx, o[1] + ly, o[2] + lz];
                    if let Some(p) = self.item_at(get, seed, cell) {
                        out.push(p);
                    }
                }
            }
        }
        out
    }

    /// The ground item a ray from `eye` along unit `dir` meets within `reach` m, before a solid
    /// block: the crosshair's. Gone ones (`gone`) are passed over.
    pub fn target(
        &self,
        get: &mut impl FnMut([i32; 3]) -> Option<StateId>,
        seed: u64,
        eye: [f64; 3],
        dir: [f64; 3],
        reach: f64,
        gone: &HashSet<[i32; 3]>,
    ) -> Option<Placed> {
        let step = 0.05;
        let mut last = None;
        let mut t = 0.0;
        while t <= reach {
            let p = std::array::from_fn::<f64, 3, _>(|k| eye[k] + dir[k] * t);
            let cell = crate::cell_of_pos(p);
            if last != Some(cell) {
                last = Some(cell);
                if get(cell).is_some_and(solid) {
                    return None;
                }
                if !gone.contains(&cell)
                    && let Some(it) = self.item_at(get, seed, cell)
                    && self.hit(&it, eye, dir, reach)
                {
                    return Some(it);
                }
            }
            t += step;
        }
        None
    }

    /// Whether the break action aimed along `dir` from `eye` hits the item `it` (the crosshair's,
    /// from [`target`](Self::target)) rather than `block`, the cell the block targeting found (ADR
    /// 0071, amended 2026-11-09): the item when it is nearer along the ray than where the ray
    /// enters the block's cell, so a hit on a stick does not break the ground under it; with no
    /// block, the item.
    pub fn hit_before(
        &self,
        it: &Placed,
        eye: [f64; 3],
        dir: [f64; 3],
        block: Option<[i32; 3]>,
    ) -> bool {
        let Some((c, _)) = self.sphere(it) else {
            return false;
        };
        let along = (0..3).map(|i| (c[i] - eye[i]) * dir[i]).sum::<f64>();
        let Some(block) = block else {
            return true;
        };
        // The ray's entry into the block's cell (slabs).
        let lo = crate::pos_of_cell(block);
        let hi = [lo[0] + 1.0, lo[1] + crate::CELL_H, lo[2] + 1.0];
        let (mut t0, mut t1) = (f64::NEG_INFINITY, f64::INFINITY);
        for i in 0..3 {
            if dir[i].abs() < 1e-12 {
                if eye[i] < lo[i] || eye[i] > hi[i] {
                    return true; // the ray misses the cell
                }
                continue;
            }
            let (a, b) = ((lo[i] - eye[i]) / dir[i], (hi[i] - eye[i]) / dir[i]);
            t0 = t0.max(a.min(b));
            t1 = t1.min(a.max(b));
        }
        t0 > t1 || along < t0.max(0.0)
    }

    /// The item's bounding sphere (world centre, radius m).
    fn sphere(&self, it: &Placed) -> Option<([f64; 3], f64)> {
        let k = self.kind(it.kind)?;
        let f = FaceFrame::at_row(it.cell[1]);
        let base = it.point();
        let up = f.up();
        let h = k.size[1] * it.scale / 2.0;
        let c: [f64; 3] = std::array::from_fn(|i| base[i] + up[i] * h);
        let r = k.size[0].max(k.size[2]) * it.scale / 2.0 + 0.08;
        Some((c, r))
    }

    /// Whether the ray meets the item's bounding sphere (a little generous: they are small).
    fn hit(&self, it: &Placed, eye: [f64; 3], dir: [f64; 3], reach: f64) -> bool {
        let Some((c, r)) = self.sphere(it) else {
            return false;
        };
        let to: [f64; 3] = std::array::from_fn(|i| c[i] - eye[i]);
        let along = (0..3)
            .map(|i| to[i] * dir[i])
            .sum::<f64>()
            .clamp(0.0, reach);
        let d2 = (0..3)
            .map(|i| (to[i] - dir[i] * along).powi(2))
            .sum::<f64>();
        d2 <= r * r
    }
}

/// splitmix64's finaliser.
fn mix(seed: u64, [x, y, z]: [i32; 3], salt: u64) -> u64 {
    let mut h = seed ^ 0x6772_6f75_6e64_0000; // "ground"
    for v in [x as u32 as u64, y as u32 as u64, z as u32 as u64, salt] {
        h = h.wrapping_add(v).wrapping_add(0x9e37_79b9_7f4a_7c15);
        h = (h ^ (h >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        h = (h ^ (h >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        h ^= h >> 31;
    }
    h
}

fn unit(h: u64) -> f64 {
    (h >> 11) as f64 / (1u64 << 53) as f64
}

/// The ground items picked up (or knocked loose), by world cell: the scatter's tombstones, saved
/// with the world (one record; ADR 0071).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GroundGone {
    pub cells: HashSet<[i32; 3]>,
}

const GONE_VERSION: u8 = 1;

impl GroundGone {
    /// A version byte, a count, then each cell (x, y, z little-endian i32), sorted.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut cells: Vec<[i32; 3]> = self.cells.iter().copied().collect();
        cells.sort_unstable();
        let mut out = Vec::with_capacity(5 + 12 * cells.len());
        out.push(GONE_VERSION);
        out.extend((cells.len() as u32).to_le_bytes());
        for c in cells {
            for v in c {
                out.extend(v.to_le_bytes());
            }
        }
        out
    }

    /// None: not a record this build reads (a newer one's: kept, not written over).
    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        if *b.first()? != GONE_VERSION {
            return None;
        }
        let n = u32::from_le_bytes(b.get(1..5)?.try_into().ok()?) as usize;
        let body = b.get(5..)?;
        if body.len() != n * 12 {
            return None;
        }
        let cells = body
            .chunks_exact(12)
            .map(|c| {
                std::array::from_fn(|k| {
                    i32::from_le_bytes(c[4 * k..4 * k + 4].try_into().expect("4 bytes"))
                })
            })
            .collect();
        Some(Self { cells })
    }

    /// The gone cells in chunk `c`, chunk-local (a mesh job's).
    pub fn in_chunk(&self, c: [i32; 3]) -> Vec<[i32; 3]> {
        let o = c.map(|v| v * CHUNK_SIZE);
        let mut out: Vec<[i32; 3]> = self
            .cells
            .iter()
            .filter(|w| (0..3).all(|k| w[k].div_euclid(CHUNK_SIZE) == c[k]))
            .map(|w| [w[0] - o[0], w[1] - o[1], w[2] - o[2]])
            .collect();
        out.sort_unstable();
        out
    }
}

#[cfg(test)]
mod tests;
