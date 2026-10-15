//! Structures (ARCHITECTURE.md §6.3, docs/STRUCTURES.md §3): hand-built voxel geometry as data,
//! stamped over a base generator (the movement playground the controller tests walk is one).
//!
//! The source of truth is `shared/structures/<name>.json`; `shared/structures/gen.mjs` validates
//! every file against the block registry and emits [`STRUCTURES`] (`structures.gen.rs`): a
//! canonical form and its hash. This module loads a structure by name ([`load`]), resolves its
//! block names once, picks its variants deterministically from a seed ([`Structure::place`]) and
//! stamps the result over a generator ([`with_structure`]). Blocks are named by state string,
//! never by id.
//!
//! The kind (`Body`) and the variant operations (`Variant`) are open lists: a new one is a name
//! the generator validates plus its engine code here, with no change to existing files.

use std::sync::Arc;

use dwell_blocks::{
    Facing, PROPERTIES, PROPERTY_VALUES, StateId, block_of, info, parse_state, state_property,
    with_property,
};

use crate::{AirChunkTest, CHUNK_SIZE, ChunkCoord, ChunkGenerator, floor_div, local_index};

/// The canonical text's format line (the files' `"format"`).
pub const FORMAT: u32 = 1;

// ---- The generated tables ------------------------------------------------------------------------

/// A horizontal axis a structure may be mirrored across (its coordinate negated).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Z,
}

/// One weighted block choice for a palette character: `options` are (canonical state, weight).
#[derive(Clone, Copy, Debug)]
pub struct Choice {
    pub key: char,
    pub options: &'static [(&'static str, u32)],
}

/// A named variant operation, in the canonical order they apply.
#[derive(Clone, Copy, Debug)]
pub enum Variant {
    /// Quarter turns about +Y (clockwise seen from above: north → east), in degrees; one is picked.
    Rotate(&'static [u16]),
    /// Axes one of which may be mirrored: one of "none" and these is picked.
    Mirror(&'static [Axis]),
    /// Per-voxel weighted block choices for palette characters.
    Choices(&'static [Choice]),
}

/// What a structure is made of, by kind.
#[derive(Clone, Copy, Debug)]
pub enum Body {
    /// `"voxels"`: hand-built cells. `cells` holds `size` palette characters, x fastest, then z,
    /// then y (bottom up); `palette` maps each to a canonical state, or `None` (keep what is there).
    Voxels {
        size: [i32; 3],
        anchor: [i32; 3],
        palette: &'static [(char, Option<&'static str>)],
        cells: &'static str,
    },
}

/// One structure as the generator emits it.
#[derive(Clone, Copy, Debug)]
pub struct StructureDef {
    pub name: &'static str,
    /// FNV-1a 64 over [`canonical_text`].
    pub hash: u64,
    pub body: Body,
    pub variants: &'static [Variant],
}

#[rustfmt::skip]
#[allow(clippy::all)]
mod tables {
    #[allow(unused_imports)]
    use super::{Axis, Body, Choice, StructureDef, Variant};
    include!("structures.gen.rs");
}
pub use tables::STRUCTURES;

/// The build's structure with this name.
pub fn find(name: &str) -> Option<&'static StructureDef> {
    STRUCTURES.iter().find(|s| s.name == name)
}

/// Loads the build's structure with this name.
pub fn load(name: &str) -> Result<Structure, String> {
    Structure::from_def(find(name).ok_or_else(|| format!("unknown structure \"{name}\""))?)
}

// ---- Canonical form (shared/structures/gen.mjs writes the same text) -----------------------------

/// The canonical text of a structure: what its hash is taken over.
pub fn canonical_text(def: &StructureDef) -> String {
    let mut out = format!("structure {FORMAT}\nname {}\n", def.name);
    match def.body {
        Body::Voxels {
            size,
            anchor,
            palette,
            cells,
        } => {
            out += &format!(
                "kind voxels\nsize {} {} {}\nanchor {} {} {}\n",
                size[0], size[1], size[2], anchor[0], anchor[1], anchor[2]
            );
            let mut keys: Vec<_> = palette.to_vec();
            keys.sort_by_key(|(c, _)| *c);
            for (c, state) in keys {
                out += &format!("palette {c} {}\n", state.unwrap_or("keep"));
            }
            let (sx, sz) = (size[0] as usize, size[2] as usize);
            for y in 0..size[1] as usize {
                out += &format!("layer {y}\n");
                for z in 0..sz {
                    let at = (y * sz + z) * sx;
                    out += cells.get(at..at + sx).unwrap_or("");
                    out.push('\n');
                }
            }
        }
    }
    for v in def.variants {
        match v {
            Variant::Rotate(r) => {
                let r: Vec<_> = r.iter().map(u16::to_string).collect();
                out += &format!("rotate {}\n", r.join(" "));
            }
            Variant::Mirror(m) => {
                let m: Vec<_> = m
                    .iter()
                    .map(|a| if *a == Axis::X { "x" } else { "z" })
                    .collect();
                out += &format!("mirror {}\n", m.join(" "));
            }
            Variant::Choices(cs) => {
                for c in *cs {
                    let o: Vec<_> = c.options.iter().map(|(n, w)| format!("{n} {w}")).collect();
                    out += &format!("choices {} {}\n", c.key, o.join(" "));
                }
            }
        }
    }
    out
}

/// FNV-1a 64 over the bytes of a canonical text.
pub fn structure_hash(text: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

// ---- Deterministic picks (integer maths, ADR 0010) -----------------------------------------------

fn splitmix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

fn mix(a: u64, b: u64) -> u64 {
    splitmix(a ^ splitmix(b))
}

fn weighted(options: &[(StateId, u32)], r: u64) -> StateId {
    let total: u64 = options.iter().map(|(_, w)| u64::from(*w)).sum();
    let mut r = r % total.max(1);
    for &(s, w) in options {
        if r < u64::from(w) {
            return s;
        }
        r -= u64::from(w);
    }
    options[0].0
}

// ---- Orientation ---------------------------------------------------------------------------------

/// How a placement is turned: mirrored first (across `mirror`), then rotated `quarter_turns`
/// clockwise about +Y through the anchor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Orientation {
    pub quarter_turns: u8,
    pub mirror: Option<Axis>,
}

impl Orientation {
    /// An offset from the anchor, turned.
    pub fn apply(self, [mut x, y, mut z]: [i32; 3]) -> [i32; 3] {
        match self.mirror {
            Some(Axis::X) => x = -x,
            Some(Axis::Z) => z = -z,
            None => {}
        }
        for _ in 0..self.quarter_turns % 4 {
            (x, z) = (-z, x);
        }
        [x, y, z]
    }

    /// A compass facing (0 north, 1 east, 2 south, 3 west), turned the same way.
    fn facing(self, mut f: usize) -> usize {
        match self.mirror {
            Some(Axis::X) if !f.is_multiple_of(2) => f = 4 - f,
            Some(Axis::Z) if f.is_multiple_of(2) => f = 2 - f,
            _ => {}
        }
        (f + usize::from(self.quarter_turns)) % 4
    }
}

const FACINGS: [&str; 4] = ["north", "east", "south", "west"];

/// The registry property holding a facing state's compass facing (its block's `facingProperty`:
/// the one whose values are all facings and which holds the state's facing).
fn facing_property(state: StateId) -> Option<&'static str> {
    let now = match info(state).facing {
        Facing::None => return None,
        Facing::North => "north",
        Facing::East => "east",
        Facing::South => "south",
        Facing::West => "west",
    };
    let block = block_of(state);
    (0..block.property_count)
        .map(|k| &PROPERTIES[block.first_property + k])
        .find(|p| {
            PROPERTY_VALUES[p.first_value..p.first_value + p.value_count]
                .iter()
                .all(|v| FACINGS.contains(v))
                && state_property(state, p.name) == Some(now)
        })
        .map(|p| p.name)
}

/// A state turned with a placement: facing blocks rotate (and mirror) their facing.
pub fn orient_state(state: StateId, o: Orientation) -> StateId {
    let Some(prop) = facing_property(state) else {
        return state;
    };
    let now = state_property(state, prop).unwrap_or("north");
    let f = FACINGS.iter().position(|v| *v == now).unwrap_or(0);
    with_property(state, prop, FACINGS[o.facing(f)]).unwrap_or(state)
}

// ---- Loaded structures ---------------------------------------------------------------------------

/// A cell's content: kept, a fixed state, or a weighted choice (index into the choice list).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cell {
    Keep,
    State(StateId),
    Choice(usize),
}

/// A structure with its block names resolved against this build's registry.
#[derive(Clone, Debug)]
pub struct Structure {
    pub name: &'static str,
    pub hash: u64,
    pub size: [i32; 3],
    pub anchor: [i32; 3],
    cells: Vec<Cell>,
    choices: Vec<Vec<(StateId, u32)>>,
    rotations: Vec<u8>,
    mirrors: Vec<Option<Axis>>,
}

impl Structure {
    /// Resolves a generated structure; fails if its data disagrees with its hash or this
    /// registry (a stale generated table), naming the structure.
    pub fn from_def(def: &StructureDef) -> Result<Self, String> {
        let fail = |why: String| Err(format!("structure {}: {why}", def.name));
        if structure_hash(&canonical_text(def)) != def.hash {
            return fail("its hash does not match its data (run shared/structures/gen.mjs)".into());
        }
        let Body::Voxels {
            size,
            anchor,
            palette,
            cells,
        } = def.body;
        let resolve = |name: &str| parse_state(name).map_err(|e| format!("block \"{name}\": {e}"));
        let mut by_char = Vec::new();
        for &(c, state) in palette {
            by_char.push((
                c,
                match state {
                    None => Cell::Keep,
                    Some(n) => match resolve(n) {
                        Ok(s) => Cell::State(s),
                        Err(e) => return fail(e),
                    },
                },
            ));
        }
        let mut choices = Vec::new();
        let mut rotations = vec![0];
        let mut mirrors = vec![None];
        for v in def.variants {
            match v {
                Variant::Rotate(r) => rotations = r.iter().map(|d| (d / 90 % 4) as u8).collect(),
                Variant::Mirror(m) => mirrors.extend(m.iter().copied().map(Some)),
                Variant::Choices(cs) => {
                    for c in *cs {
                        let mut options = Vec::new();
                        for &(n, w) in c.options {
                            match resolve(n) {
                                Ok(s) => options.push((s, w)),
                                Err(e) => return fail(e),
                            }
                        }
                        let Some(slot) = by_char.iter_mut().find(|(k, _)| *k == c.key) else {
                            return fail(format!("choices for '{}', not in the palette", c.key));
                        };
                        slot.1 = Cell::Choice(choices.len());
                        choices.push(options);
                    }
                }
            }
        }
        let volume = size.iter().product::<i32>();
        if size.iter().any(|&n| n < 1) || cells.len() != volume as usize {
            return fail(format!("{} cells for size {size:?}", cells.len()));
        }
        let mut out = Vec::with_capacity(cells.len());
        for c in cells.chars() {
            match by_char.iter().find(|(k, _)| *k == c) {
                Some((_, cell)) => out.push(*cell),
                None => return fail(format!("'{c}' is not in the palette")),
            }
        }
        Ok(Self {
            name: def.name,
            hash: def.hash,
            size,
            anchor,
            cells: out,
            choices,
            rotations,
            mirrors,
        })
    }

    /// The orientation a seed picks from the structure's rotate and mirror variants.
    pub fn orientation(&self, seed: u64) -> Orientation {
        let pick = |stream: u64, n: usize| (mix(seed, stream) % n as u64) as usize;
        Orientation {
            quarter_turns: self.rotations[pick(1, self.rotations.len())],
            mirror: self.mirrors[pick(2, self.mirrors.len())],
        }
    }

    /// The structure placed with its anchor at `at`, every variant picked from `seed`.
    pub fn place(&self, at: [i32; 3], seed: u64) -> Placed {
        self.place_oriented(at, self.orientation(seed), seed)
    }

    /// The structure placed with its anchor at `at`, turned by `o` (block choices from `seed`).
    pub fn place_oriented(&self, at: [i32; 3], o: Orientation, seed: u64) -> Placed {
        let [sx, sy, sz] = self.size;
        let offset =
            |x, y, z| o.apply([x - self.anchor[0], y - self.anchor[1], z - self.anchor[2]]);
        let corners = [offset(0, 0, 0), offset(sx - 1, sy - 1, sz - 1)];
        let min: [i32; 3] = std::array::from_fn(|i| at[i] + corners[0][i].min(corners[1][i]));
        let dims: [i32; 3] = std::array::from_fn(|i| (corners[0][i] - corners[1][i]).abs() + 1);
        let mut cells = vec![None; self.cells.len()];
        let mut i = 0;
        for y in 0..sy {
            for z in 0..sz {
                for x in 0..sx {
                    let state = match self.cells[i] {
                        Cell::Keep => None,
                        Cell::State(s) => Some(s),
                        Cell::Choice(k) => {
                            Some(weighted(&self.choices[k], mix(seed, 3 + i as u64)))
                        }
                    };
                    let d = offset(x, y, z);
                    let p: [i32; 3] = std::array::from_fn(|j| at[j] + d[j] - min[j]);
                    cells[((p[1] * dims[2] + p[2]) * dims[0] + p[0]) as usize] =
                        state.map(|s| orient_state(s, o));
                    i += 1;
                }
            }
        }
        Placed {
            min,
            max: std::array::from_fn(|j| min[j] + dims[j] - 1),
            cells,
        }
    }
}

/// A structure placed in the world: what it puts at each voxel of its box.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placed {
    /// The box it occupies, inclusive.
    pub min: [i32; 3],
    pub max: [i32; 3],
    /// x fastest, then z, then y; `None` keeps what is there.
    cells: Vec<Option<StateId>>,
}

impl Placed {
    /// The state the placement puts at a voxel (`None` outside it or where it keeps the base).
    pub fn at(&self, p: [i32; 3]) -> Option<StateId> {
        if (0..3).any(|i| p[i] < self.min[i] || p[i] > self.max[i]) {
            return None;
        }
        let d: [i32; 3] = std::array::from_fn(|i| p[i] - self.min[i]);
        let w = self.max[0] - self.min[0] + 1;
        let l = self.max[2] - self.min[2] + 1;
        self.cells[((d[1] * l + d[2]) * w + d[0]) as usize]
    }

    /// True when the placement's box misses the chunk.
    pub fn misses(&self, c: ChunkCoord) -> bool {
        let c = c.to_array();
        !(0..3).all(|i| {
            floor_div(self.min[i], CHUNK_SIZE) <= c[i] && floor_div(self.max[i], CHUNK_SIZE) >= c[i]
        })
    }
}

/// `base` with the placed structure stamped over it.
pub fn with_structure(base: ChunkGenerator, placed: Arc<Placed>) -> ChunkGenerator {
    Arc::new(move |coord, chunk| {
        base(coord, chunk);
        if placed.misses(coord) {
            return;
        }
        let voxels = chunk.voxels_mut();
        for lz in 0..CHUNK_SIZE {
            for ly in 0..CHUNK_SIZE {
                for lx in 0..CHUNK_SIZE {
                    let p = [
                        coord.x * CHUNK_SIZE + lx,
                        coord.y * CHUNK_SIZE + ly,
                        coord.z * CHUNK_SIZE + lz,
                    ];
                    if let Some(m) = placed.at(p) {
                        voxels[local_index(lx, ly, lz)] = m;
                    }
                }
            }
        }
    })
}

/// `base_air` except for chunks the placed structure touches.
pub fn with_structure_air(base_air: AirChunkTest, placed: Arc<Placed>) -> AirChunkTest {
    Arc::new(move |c| placed.misses(c) && base_air(c))
}

/// Loads every structure of the build: fails, naming the structure, if any is stale or names a
/// block this registry lacks.
pub fn check_all() -> Result<(), String> {
    STRUCTURES
        .iter()
        .try_for_each(|d| Structure::from_def(d).map(|_| ()))
}
