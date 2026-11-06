//! What the water grid reads of the world, as its own copy (ADR 0062, docs/WATER_SURFACE_GRID.md
//! §2d): a byte a cell, solid, open, or liquid and its fill in eighths, per loaded chunk; a uniform
//! chunk (all stone, all air) as one value. The grid reads only this, so it can run off the
//! server's thread; the server sends it the world's chunks as they load and its cells as they are
//! edited ([`Mirror`] finds what changed).

use dwell_blocks::{StateId, info};
use dwell_world::hash::{HashMap, HashSet};
use dwell_world::{
    CHUNK_SIZE, CHUNK_VOLUME, Chunk, ChunkCoord, M, VoxelWorld, chunk_of, local_index,
};

/// A cell water can't enter.
pub const SOLID: u8 = 0;
/// A cell water may fill (air, or what it washes away).
pub const OPEN: u8 = 1;
/// A liquid cell is `LIQUID + eighths - 1` (`eighths` 1..=8; 8 a full cell).
pub const LIQUID: u8 = 2;
/// A full liquid cell.
pub const FULL: u8 = LIQUID + 7;

/// What a cell is to water.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Solid,
    Open,
    Liquid,
}

/// A state's byte.
pub fn byte_of(m: StateId) -> u8 {
    if m == M.air {
        return OPEN;
    }
    let s = info(m);
    if s.liquid {
        let fill = if s.fill <= 0.0 { 1.0 } else { s.fill.min(1.0) };
        LIQUID + ((fill * 8.0).round().clamp(1.0, 8.0) as u8) - 1
    } else if s.replaceable && !s.solid {
        OPEN
    } else {
        SOLID
    }
}

/// A byte's kind.
pub fn kind_of(b: u8) -> Kind {
    match b {
        SOLID => Kind::Solid,
        OPEN => Kind::Open,
        _ => Kind::Liquid,
    }
}

/// A liquid byte's share of its cell (a full one, or its eighths); 0 for others.
pub fn fill_of(b: u8) -> f32 {
    if b < LIQUID {
        0.0
    } else {
        f32::from(b - LIQUID + 1) / 8.0
    }
}

/// One chunk's cells.
#[derive(Clone, PartialEq, Eq)]
pub enum ChunkCells {
    /// Every cell the same.
    Uniform(u8),
    /// A byte a cell, in `dwell_world::local_index`'s order.
    Mixed(Box<[u8; CHUNK_VOLUME]>),
}

impl std::fmt::Debug for ChunkCells {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChunkCells::Uniform(b) => write!(f, "Uniform({b})"),
            ChunkCells::Mixed(_) => write!(f, "Mixed"),
        }
    }
}

impl ChunkCells {
    /// A chunk's cells.
    pub fn of(chunk: &Chunk) -> Self {
        let v = chunk.voxels();
        let first = v[0];
        if v.iter().all(|&m| m == first) {
            return ChunkCells::Uniform(byte_of(first));
        }
        let mut out = Box::new([0u8; CHUNK_VOLUME]);
        // most chunks hold few states: each looked up once per run
        let (mut last, mut b) = (first, byte_of(first));
        for (o, &m) in out.iter_mut().zip(v.iter()) {
            if m != last {
                last = m;
                b = byte_of(m);
            }
            *o = b;
        }
        let one = out[0];
        if out.iter().all(|&x| x == one) {
            return ChunkCells::Uniform(one);
        }
        ChunkCells::Mixed(out)
    }

    pub fn get(&self, i: usize) -> u8 {
        match self {
            ChunkCells::Uniform(b) => *b,
            ChunkCells::Mixed(v) => v[i],
        }
    }

    pub fn set(&mut self, i: usize, b: u8) {
        match self {
            ChunkCells::Uniform(u) if *u == b => {}
            ChunkCells::Uniform(u) => {
                let mut v = Box::new([*u; CHUNK_VOLUME]);
                v[i] = b;
                *self = ChunkCells::Mixed(v);
            }
            ChunkCells::Mixed(v) => v[i] = b,
        }
    }

    /// The local indices of its liquid cells.
    pub fn liquids(&self) -> Vec<usize> {
        match self {
            ChunkCells::Uniform(b) if *b >= LIQUID => (0..CHUNK_VOLUME).collect(),
            ChunkCells::Uniform(_) => Vec::new(),
            ChunkCells::Mixed(v) => (0..CHUNK_VOLUME).filter(|&i| v[i] >= LIQUID).collect(),
        }
    }

    /// Bytes held (a diagnostic).
    pub fn bytes(&self) -> usize {
        match self {
            ChunkCells::Uniform(_) => 1,
            ChunkCells::Mixed(_) => CHUNK_VOLUME,
        }
    }

    /// Encoded for a message: 0 then the byte (uniform), or 1 then runs (a byte, then the run's
    /// length − 1 as a u16, little-endian) in local-index order.
    pub fn encode(&self, out: &mut Vec<u8>) {
        match self {
            ChunkCells::Uniform(b) => {
                out.push(0);
                out.push(*b);
            }
            ChunkCells::Mixed(v) => {
                out.push(1);
                let mut i = 0;
                while i < CHUNK_VOLUME {
                    let b = v[i];
                    let mut n = 1;
                    while i + n < CHUNK_VOLUME && v[i + n] == b && n < 0x1_0000 {
                        n += 1;
                    }
                    out.push(b);
                    out.extend_from_slice(&((n - 1) as u16).to_le_bytes());
                    i += n;
                }
            }
        }
    }

    /// Decoded from `b` at `*at` (None if malformed).
    pub fn decode(b: &[u8], at: &mut usize) -> Option<Self> {
        let tag = *b.get(*at)?;
        *at += 1;
        match tag {
            0 => {
                let v = *b.get(*at)?;
                *at += 1;
                Some(ChunkCells::Uniform(v))
            }
            1 => {
                let mut v = Box::new([0u8; CHUNK_VOLUME]);
                let mut i = 0;
                while i < CHUNK_VOLUME {
                    let s = b.get(*at..*at + 3)?;
                    *at += 3;
                    let n = usize::from(u16::from_le_bytes([s[1], s[2]])) + 1;
                    v.get_mut(i..i + n)?.fill(s[0]);
                    i += n;
                }
                Some(ChunkCells::Mixed(v))
            }
            _ => None,
        }
    }
}

/// The grid's copy of the loaded world.
#[derive(Default, Clone, Debug)]
pub struct Cells {
    chunks: HashMap<ChunkCoord, ChunkCells>,
}

fn split(c: [i32; 3]) -> (ChunkCoord, usize) {
    let cc = chunk_of(c[0], c[1], c[2]);
    (
        cc,
        local_index(
            c[0] - cc.x * CHUNK_SIZE,
            c[1] - cc.y * CHUNK_SIZE,
            c[2] - cc.z * CHUNK_SIZE,
        ),
    )
}

impl Cells {
    /// A cell's byte, or none where its chunk isn't loaded.
    pub fn get(&self, c: [i32; 3]) -> Option<u8> {
        let (cc, i) = split(c);
        self.chunks.get(&cc).map(|k| k.get(i))
    }

    /// A cell's kind, or none where its chunk isn't loaded.
    pub fn kind(&self, c: [i32; 3]) -> Option<Kind> {
        self.get(c).map(kind_of)
    }

    /// Sets a loaded cell's byte (a cell of a chunk not loaded is left: its chunk brings it).
    pub fn set(&mut self, c: [i32; 3], b: u8) {
        let (cc, i) = split(c);
        if let Some(k) = self.chunks.get_mut(&cc) {
            k.set(i, b);
        }
    }

    pub fn insert(&mut self, at: ChunkCoord, cells: ChunkCells) {
        self.chunks.insert(at, cells);
    }

    pub fn remove(&mut self, at: ChunkCoord) {
        self.chunks.remove(&at);
    }

    pub fn contains(&self, at: ChunkCoord) -> bool {
        self.chunks.contains_key(&at)
    }

    pub fn chunk(&self, at: ChunkCoord) -> Option<&ChunkCells> {
        self.chunks.get(&at)
    }

    /// Chunks held.
    pub fn len(&self) -> usize {
        self.chunks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    /// Bytes held (a diagnostic: the copy's memory).
    pub fn bytes(&self) -> usize {
        self.chunks.values().map(ChunkCells::bytes).sum()
    }
}

/// What changed in the world for the grid's copy.
#[derive(Debug, PartialEq)]
pub enum Change {
    /// A chunk loaded (`fresh`: new to the grid, its water read) or changed past what was told
    /// (its cells sent again, its water not read again).
    Load {
        at: ChunkCoord,
        cells: ChunkCells,
        fresh: bool,
    },
    Unload(ChunkCoord),
}

/// What the grid's copy was told of each loaded chunk: the chunk object (`Chunk::stamp`) and its
/// revision. Changes the server tells cell by cell (edits, the water's own writes) are accounted
/// with [`Mirror::told`]; anything else that changes a chunk (a chunk stored again, a write
/// outside a modification) sends it whole.
#[derive(Default, Debug)]
pub struct Mirror {
    seen: HashMap<ChunkCoord, (u64, u32)>,
}

impl Mirror {
    /// The world's chunks against what the copy was told: loads, reloads and unloads.
    pub fn sync(&mut self, world: &VoxelWorld) -> Vec<Change> {
        let mut out = Vec::new();
        let mut present: HashSet<ChunkCoord> = HashSet::default();
        for (&c, chunk) in world.chunks() {
            present.insert(c);
            let now = (chunk.stamp(), chunk.revision());
            match self.seen.insert(c, now) {
                Some(was) if was == now => {}
                was => out.push(Change::Load {
                    at: c,
                    cells: ChunkCells::of(chunk),
                    fresh: was.is_none(),
                }),
            }
        }
        if present.len() != self.seen.len() {
            let gone: Vec<ChunkCoord> = self
                .seen
                .keys()
                .filter(|c| !present.contains(c))
                .copied()
                .collect();
            for c in gone {
                self.seen.remove(&c);
                out.push(Change::Unload(c));
            }
        }
        // a deterministic order (the world's map isn't)
        out.sort_by_key(|ch| match ch {
            Change::Load { at, .. } | Change::Unload(at) => (at.x, at.y, at.z),
        });
        out
    }

    /// A chunk's revision moved from `before` to `after` by changes told to the copy cell by cell:
    /// kept as told, if the copy was up to date before them.
    pub fn told(&mut self, at: ChunkCoord, stamp: u64, before: u32, after: u32) {
        if let Some(s) = self.seen.get_mut(&at)
            && *s == (stamp, before)
        {
            s.1 = after;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_blocks::require;
    use dwell_world::empty_generator;

    #[test]
    fn a_states_byte_is_its_kind_and_fill() {
        // face-a-only: cells are world cells, the same on both faces.
        let stone = require("dwell:stone");
        assert_eq!(byte_of(M.air), OPEN);
        assert_eq!(byte_of(stone), SOLID);
        assert_eq!(byte_of(M.water), FULL);
        assert_eq!(fill_of(FULL), 1.0);
        let half = dwell_blocks::liquid_at_fill(M.water, 0.5);
        assert_eq!(fill_of(byte_of(half)), 0.5);
        assert_eq!(kind_of(byte_of(half)), Kind::Liquid);
    }

    #[test]
    fn chunk_cells_round_trip_compactly() {
        // face-a-only: an encoding, no up or down in it.
        let stone = require("dwell:stone");
        let mut chunk = Chunk::default();
        for i in 0..CHUNK_VOLUME / 2 {
            chunk.set_at(i, stone);
        }
        chunk.set_at(CHUNK_VOLUME - 1, M.water);
        let cells = ChunkCells::of(&chunk);
        assert!(matches!(cells, ChunkCells::Mixed(_)));
        let mut b = Vec::new();
        cells.encode(&mut b);
        assert!(
            b.len() < 64,
            "a half-stone chunk is a few runs: {} bytes",
            b.len()
        );
        let mut at = 0;
        assert_eq!(ChunkCells::decode(&b, &mut at), Some(cells.clone()));
        assert_eq!(at, b.len());
        assert_eq!(cells.liquids(), vec![CHUNK_VOLUME - 1]);
        // all one state: one value
        let air = ChunkCells::of(&Chunk::default());
        assert_eq!(air, ChunkCells::Uniform(OPEN));
        let mut b = Vec::new();
        air.encode(&mut b);
        assert_eq!(b, vec![0, OPEN]);
        // a malformed one fails
        let mut at = 0;
        assert_eq!(ChunkCells::decode(&[1, 0, 0xff], &mut at), None);
    }

    #[test]
    fn the_mirror_tells_loads_changes_and_unloads_once() {
        // face-a-only: chunks as stored, no up or down in it.
        let mut world = VoxelWorld::new(empty_generator(), None);
        let mut m = Mirror::default();
        world.set_voxel(1, 1, 1, M.water);
        let c = chunk_of(1, 1, 1);
        let ch = m.sync(&world);
        assert!(matches!(&ch[..], [Change::Load { at, fresh: true, .. }] if *at == c));
        assert!(m.sync(&world).is_empty(), "nothing new: nothing told");
        // a change told cell by cell is accounted
        let (stamp, rev) = {
            let k = world.find(c).expect("loaded");
            (k.stamp(), k.revision())
        };
        world.write_voxel(2, 1, 1, M.water);
        world.get_or_create(c).bump_revision();
        m.told(c, stamp, rev, rev + 1);
        assert!(m.sync(&world).is_empty(), "told by its cells");
        // one that wasn't sends the chunk again, not as new
        world.set_voxel(3, 1, 1, M.water);
        let ch = m.sync(&world);
        assert!(
            matches!(&ch[..], [Change::Load { fresh: false, cells, .. }] if cells.get(local_index(3, 1, 1)) == FULL)
        );
        world.remove(c);
        assert_eq!(m.sync(&world), vec![Change::Unload(c)]);
    }
}
