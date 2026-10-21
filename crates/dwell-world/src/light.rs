//! Flood light (docs/DAY_NIGHT.md §11; data: shared/look/light.json and the blocks' `lightFilter`
//! and `emits`): two channels per cell, each 0–15, packed in a byte (sky light high, block light
//! low), kept by the client for the chunks it holds and never saved or sent.
//!
//! - **Sky light** enters from the face's sky at full strength and falls straight down
//!   (face-local, [`up_of_y`]) without loss through cells that filter nothing; everywhere else it
//!   spreads by flood fill, losing `falloff` per cell (across, or vertical) or the cell's filter
//!   if that is more. A cell whose face-local "above" is in a chunk the map doesn't hold takes the
//!   sky at full strength: the sky enters at the top of what is held.
//! - **Block light** spreads the same way from cells that emit it.
//!
//! Updates are the standard removal and addition passes (Minecraft's, Starlight's): what an
//! edit, a load or an unload could have lit is cleared, then refilled from what is still lit
//! around it and from the sources. The result is the fixed point of the rules over the chunks
//! held, so it doesn't depend on the order chunks came in ([`LightMap::relit_from_scratch`] is
//! the reference the tests compare it with).

use std::collections::VecDeque;

use crate::hash::HashMap;

use crate::{CHUNK_SIZE, CHUNK_VOLUME, Chunk, ChunkCoord, up_of_y};
use dwell_blocks::{LIGHT_MAX, StateId};

#[path = "light.gen.rs"]
mod generated;
pub use generated::LIGHT;

/// The flood light's tunables (shared/look/light.json).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightTunables {
    /// Levels lost per cell spread into across (x, z) and vertically.
    pub falloff_across: u8,
    pub falloff_vertical: u8,
    /// The sky light's brightness curve ([`Self::brightness`]).
    pub sky_curve: f64,
    /// The direct sun and moon: none at sky level `direct_from`, all at `direct_full`.
    pub direct_from: u8,
    pub direct_full: u8,
    /// The block light's brightness curve, colour (sRGB, 0–1) and strength.
    pub block_curve: f64,
    pub block_color: [f64; 3],
    pub block_strength: f64,
    /// How fast a character's light follows the cell it is in (s, the ease's time constant).
    pub character_ease_s: f64,
}

impl LightTunables {
    /// The brightness (0–1) of a light `level` (0–15, fractional between cells) on `curve`:
    /// x / (curve − (curve − 1)·x), x = level / 15.
    pub fn brightness(level: f64, curve: f64) -> f64 {
        let x = (level / f64::from(LIGHT_MAX)).clamp(0.0, 1.0);
        x / (curve - (curve - 1.0) * x)
    }

    /// How much of the hemisphere (ambient) light a sky level lets in.
    pub fn sky(&self, level: f64) -> f64 {
        Self::brightness(level, self.sky_curve)
    }

    /// How much of the direct sun and moon a sky level lets in (the shadow stand-in).
    pub fn direct(&self, level: f64) -> f64 {
        let t = ((level - f64::from(self.direct_from))
            / f64::from(self.direct_full - self.direct_from))
        .clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }

    /// How bright a block level glows.
    pub fn block(&self, level: f64) -> f64 {
        Self::brightness(level, self.block_curve)
    }
}

/// A character's light eased over time toward what it stands in (`characters.easeS`), so walking
/// into a tunnel fades a hand down rather than popping.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EasedLight {
    value: Option<[f32; 2]>,
}

impl EasedLight {
    /// Moves toward `target` over `dt` seconds (exponentially, time constant `ease_s`; the first
    /// call takes it at once) and returns the value.
    pub fn update(&mut self, target: [f32; 2], dt: f64, ease_s: f64) -> [f32; 2] {
        let v = match self.value {
            None => target,
            Some(v) => {
                let k = if ease_s <= 0.0 {
                    1.0
                } else {
                    (1.0 - (-dt.max(0.0) / ease_s).exp()) as f32
                };
                [v[0] + (target[0] - v[0]) * k, v[1] + (target[1] - v[1]) * k]
            }
        };
        self.value = Some(v);
        v
    }
}

/// One of the two light channels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Sky,
    Block,
}

impl Channel {
    const fn shift(self) -> u32 {
        match self {
            Channel::Sky => 4,
            Channel::Block => 0,
        }
    }
}

/// A packed cell: sky light in the high nibble, block light in the low.
pub const fn pack(sky: u8, block: u8) -> u8 {
    (sky << 4) | block
}
/// The sky light of a packed cell.
pub const fn sky_of(packed: u8) -> u8 {
    packed >> 4
}
/// The block light of a packed cell.
pub const fn block_of(packed: u8) -> u8 {
    packed & 15
}
/// What a cell outside the chunks held reads as for meshing: open sky, no block light.
pub const OPEN: u8 = pack(LIGHT_MAX, 0);

const N: i32 = CHUNK_SIZE;
const NONE: u32 = u32::MAX;
/// +X, −X, +Y, −Y, +Z, −Z.
const DIRS: [[i32; 3]; 6] = [
    [1, 0, 0],
    [-1, 0, 0],
    [0, 1, 0],
    [0, -1, 0],
    [0, 0, 1],
    [0, 0, -1],
];

const fn local(x: i32, y: i32, z: i32) -> u16 {
    (x | (y << 5) | (z << 10)) as u16
}
const fn coords(i: u16) -> [i32; 3] {
    let i = i as i32;
    [i & 31, (i >> 5) & 31, (i >> 10) & 31]
}

/// A held chunk's light.
struct Slot {
    coord: ChunkCoord,
    /// None: every cell is `uniform`.
    cells: Option<Box<[u8; CHUNK_VOLUME]>>,
    uniform: u8,
    /// The slots of the six face neighbours (`DIRS` order), `NONE` when not held.
    near: [u32; 6],
    /// Which of the 27 chunks around (and including) it show a cell changed in this update (bit
    /// `(dx + 1) + 3(dy + 1) + 9(dz + 1)`): a vertex reads the cells around it, so a change on a
    /// border changes the neighbour's mesh too.
    changed: u32,
}

impl Slot {
    fn get(&self, i: u16) -> u8 {
        self.cells
            .as_ref()
            .map_or(self.uniform, |c| c[usize::from(i)])
    }
}

/// A cell of a held chunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct P {
    slot: u32,
    i: u16,
}

/// The flood light of the chunks the client holds.
pub struct LightMap {
    slots: Vec<Slot>,
    index: HashMap<ChunkCoord, u32>,
    free: Vec<u32>,
    /// Per state: (filter, emits).
    table: Vec<(u8, u8)>,
    /// Slots changed in the current update.
    touched: Vec<u32>,
    /// Chunks whose meshes may show a light change, since the last [`Self::take_changed`].
    changed: Vec<ChunkCoord>,
    tunables: LightTunables,
}

impl Default for LightMap {
    fn default() -> Self {
        Self::new()
    }
}

/// The voxels the map reads (the world's chunks; chunks it holds but the world doesn't are air).
pub type Voxels = HashMap<ChunkCoord, Box<Chunk>>;

impl LightMap {
    pub fn new() -> Self {
        Self::with_tunables(LIGHT)
    }

    pub fn with_tunables(tunables: LightTunables) -> Self {
        Self {
            slots: Vec::new(),
            index: HashMap::default(),
            free: Vec::new(),
            table: Vec::new(),
            touched: Vec::new(),
            changed: Vec::new(),
            tunables,
        }
    }

    pub fn tunables(&self) -> &LightTunables {
        &self.tunables
    }

    /// Whether the map holds the chunk.
    pub fn holds(&self, c: ChunkCoord) -> bool {
        self.index.contains_key(&c)
    }

    /// How many chunks it holds, and how many of them need a full array (the rest are uniform).
    pub fn counts(&self) -> (usize, usize) {
        let full = self
            .index
            .values()
            .filter(|&&s| self.slots[s as usize].cells.is_some())
            .count();
        (self.index.len(), full)
    }

    /// Bytes of light arrays held.
    pub fn bytes(&self) -> usize {
        self.counts().1 * CHUNK_VOLUME
    }

    /// The packed light of world cell `c` (None: its chunk isn't held).
    pub fn get(&self, c: [i32; 3]) -> Option<u8> {
        let coord = ChunkCoord::new(c[0].div_euclid(N), c[1].div_euclid(N), c[2].div_euclid(N));
        let &s = self.index.get(&coord)?;
        let l = c.map(|v| v.rem_euclid(N));
        Some(self.slots[s as usize].get(local(l[0], l[1], l[2])))
    }

    /// The light a character at world point `pos` (metres) takes: sky and block level as shares
    /// of 15, blended trilinearly between the cell centres around it (smooth as it moves), cells
    /// that hold no light left out (the solid next to it); open sky where nothing is held.
    pub fn sample(&self, pos: [f64; 3]) -> [f32; 2] {
        let u = [pos[0] - 0.5, pos[1] / crate::CELL_H - 0.5, pos[2] - 0.5];
        let base = u.map(|v| v.floor() as i32);
        let f: [f64; 3] = std::array::from_fn(|a| u[a] - f64::from(base[a]));
        let (mut w, mut sky, mut block) = (0.0, 0.0, 0.0);
        for k in 0..8 {
            let d = [k & 1, (k >> 1) & 1, (k >> 2) & 1];
            let c: [i32; 3] = std::array::from_fn(|a| base[a] + d[a]);
            let v = self.get(c).unwrap_or(OPEN);
            if v == 0 {
                continue;
            }
            let wk: f64 = (0..3)
                .map(|a| if d[a] == 1 { f[a] } else { 1.0 - f[a] })
                .product();
            w += wk;
            sky += wk * f64::from(sky_of(v));
            block += wk * f64::from(block_of(v));
        }
        if w <= 1e-9 {
            return [0.0, 0.0];
        }
        let max = f64::from(LIGHT_MAX);
        [(sky / w / max) as f32, (block / w / max) as f32]
    }

    /// The chunk's light with a one-cell apron (the mesher's padded layout: x fastest, then z,
    /// then y, each −1..=32), cells of chunks not held [`OPEN`].
    pub fn padded(&self, coord: ChunkCoord) -> Vec<u8> {
        let pad = N + 2;
        let mut out = vec![OPEN; (pad * pad * pad) as usize];
        for dy in -1..=1 {
            for dz in -1..=1 {
                for dx in -1..=1 {
                    let c = ChunkCoord::new(coord.x + dx, coord.y + dy, coord.z + dz);
                    let Some(&s) = self.index.get(&c) else {
                        continue;
                    };
                    let slot = &self.slots[s as usize];
                    let range = |d: i32| match d {
                        -1 => N - 1..N,
                        0 => 0..N,
                        _ => 0..1,
                    };
                    for ly in range(dy) {
                        let py = ly + dy * N + 1;
                        for lz in range(dz) {
                            let pz = lz + dz * N + 1;
                            for lx in range(dx) {
                                let px = lx + dx * N + 1;
                                out[(px + pad * (pz + pad * py)) as usize] =
                                    slot.get(local(lx, ly, lz));
                            }
                        }
                    }
                }
            }
        }
        out
    }

    /// The chunks whose meshes may show a light change since the last call (each once).
    pub fn take_changed(&mut self) -> Vec<ChunkCoord> {
        let mut out = std::mem::take(&mut self.changed);
        out.sort_by_key(|c| (c.x, c.y, c.z));
        out.dedup();
        out
    }

    // ---- Updates ---------------------------------------------------------------------------

    /// The chunk is now held (stored in `voxels`, or all air): it is lit, and what it changes
    /// around it relit.
    pub fn load(&mut self, voxels: &Voxels, coord: ChunkCoord) {
        let replaced = self.holds(coord);
        let s = if replaced {
            self.index[&coord]
        } else {
            self.alloc(coord)
        };
        // Cells below it whose sky came from its place (open while it wasn't held) are cleared
        // and relit; every cell of it, and the neighbours' cells next to it, are lit.
        // A chunk whose voxels were replaced is cleared first: any cell may have changed.
        let mut removal: Vec<P> = Vec::new();
        let mut add: Vec<P> = Vec::with_capacity(CHUNK_VOLUME + 6 * 1024);
        let kind = if replaced {
            Kind::Mixed
        } else {
            self.kind_of(voxels, s)
        };
        match kind {
            // Open sky through and through: every cell at full sky light, then spread out of
            // its sides and block light in from the neighbours'.
            Kind::Sky => {
                self.slots[s as usize].uniform = pack(LIGHT_MAX, 0);
                for d in 0..6 {
                    add.extend(border(s, d));
                }
            }
            // Nothing gets in or out: dark; only the cells below that saw the sky through its
            // place change.
            Kind::Shut => {}
            Kind::Mixed if replaced => {
                add.extend((0..CHUNK_VOLUME as u32).map(|i| P {
                    slot: s,
                    i: i as u16,
                }));
                removal.extend_from_slice(&add);
            }
            // Its cells start dark: what lights them is the sky entering its top when nothing is
            // held over it, its emitters and the neighbours' light (their sides, below).
            Kind::Mixed => {
                let coord = self.slots[s as usize].coord;
                let up = if up_of_y(coord.y * N) > 0 { 2 } else { 3 };
                if self.slots[s as usize].near[up] == NONE {
                    add.extend(border(s, up));
                }
                if let Some(chunk) = voxels.get(&coord) {
                    for (i, &id) in chunk.voxels().iter().enumerate() {
                        if id != 0 && table_rules(&mut self.table, id).1 > 0 {
                            add.push(P {
                                slot: s,
                                i: i as u16,
                            });
                        }
                    }
                }
            }
        }
        for d in 0..6 {
            let n = self.slots[s as usize].near[d];
            if n == NONE {
                continue;
            }
            for p in border(n, opposite(d)) {
                let below = d / 2 == 1 && self.above_slot(p) == Some(s);
                // An open-sky chunk keeps the full sky the cells under it had.
                if below && kind != Kind::Sky {
                    removal.push(p);
                }
                if below || kind != Kind::Shut {
                    add.push(p);
                }
            }
        }
        self.run_all(voxels, &removal, &add);
    }

    /// What a newly held chunk is, for the fast paths of [`Self::load`].
    fn kind_of(&mut self, voxels: &Voxels, s: u32) -> Kind {
        let slot = &self.slots[s as usize];
        let coord = slot.coord;
        let up = up_of_y(coord.y * N);
        let above = slot.near[if up > 0 { 2 } else { 3 }];
        let open_above = above == NONE || {
            let a = &self.slots[above as usize];
            a.cells.is_none() && a.uniform == pack(LIGHT_MAX, 0)
        };
        let Some(chunk) = voxels.get(&coord) else {
            return if open_above { Kind::Sky } else { Kind::Mixed };
        };
        let v = chunk.voxels();
        if v.iter().all(|&id| id == 0) {
            return if open_above { Kind::Sky } else { Kind::Mixed };
        }
        let first = v[0];
        let shut = |t: &mut Vec<(u8, u8)>, id: StateId| {
            let (f, e) = table_rules(t, id);
            f >= LIGHT_MAX && e == 0
        };
        if shut(&mut self.table, first)
            && (v.iter().all(|&id| id == first) || v.iter().all(|&id| shut(&mut self.table, id)))
        {
            return Kind::Shut;
        }
        Kind::Mixed
    }

    /// The chunk is no longer held: what it lit around it is cleared and relit (cells below it
    /// take the open sky again).
    pub fn unload(&mut self, voxels: &Voxels, coord: ChunkCoord) {
        let Some(&s) = self.index.get(&coord) else {
            return;
        };
        let near = self.slots[s as usize].near;
        self.release(s);
        let mut cells = Vec::new();
        for (d, &n) in near.iter().enumerate() {
            if n != NONE {
                cells.extend(border(n, opposite(d)));
            }
        }
        // Over-clearing is safe: what is still lit some other way is refilled.
        self.run_all(voxels, &cells, &cells);
    }

    /// Cells whose blocks changed (world cells; ones in chunks not held are skipped).
    pub fn edited(&mut self, voxels: &Voxels, cells: &[[i32; 3]]) {
        let ps: Vec<P> = cells.iter().filter_map(|&c| self.p_of(c)).collect();
        let mut add = ps.clone();
        for &p in &ps {
            add.extend((0..6).filter_map(|d| self.step(p, d)));
        }
        self.run_all(voxels, &ps, &add);
    }

    /// Relights every held chunk from nothing: the reference [`Self::load`], [`Self::unload`] and
    /// [`Self::edited`] must agree with.
    pub fn relit_from_scratch(&mut self, voxels: &Voxels) {
        let mut all = Vec::new();
        for (&_, &s) in &self.index {
            let slot = &mut self.slots[s as usize];
            slot.cells = None;
            slot.uniform = 0;
            all.extend((0..CHUNK_VOLUME as u32).map(|i| P {
                slot: s,
                i: i as u16,
            }));
        }
        self.run_all(voxels, &[], &all);
    }

    fn alloc(&mut self, coord: ChunkCoord) -> u32 {
        let slot = Slot {
            coord,
            cells: None,
            uniform: 0,
            near: [NONE; 6],
            changed: 0,
        };
        let s = match self.free.pop() {
            Some(s) => {
                self.slots[s as usize] = slot;
                s
            }
            None => {
                self.slots.push(slot);
                (self.slots.len() - 1) as u32
            }
        };
        self.index.insert(coord, s);
        for (d, dir) in DIRS.iter().enumerate() {
            let c = ChunkCoord::new(coord.x + dir[0], coord.y + dir[1], coord.z + dir[2]);
            if let Some(&n) = self.index.get(&c) {
                self.slots[s as usize].near[d] = n;
                self.slots[n as usize].near[opposite(d)] = s;
            }
        }
        s
    }

    fn release(&mut self, s: u32) {
        let slot = &mut self.slots[s as usize];
        let (coord, near) = (slot.coord, slot.near);
        slot.cells = None;
        self.index.remove(&coord);
        for (d, &n) in near.iter().enumerate() {
            if n != NONE {
                self.slots[n as usize].near[opposite(d)] = NONE;
            }
        }
        self.free.push(s);
    }

    fn p_of(&self, c: [i32; 3]) -> Option<P> {
        let coord = ChunkCoord::new(c[0].div_euclid(N), c[1].div_euclid(N), c[2].div_euclid(N));
        let &slot = self.index.get(&coord)?;
        let l = c.map(|v| v.rem_euclid(N));
        Some(P {
            slot,
            i: local(l[0], l[1], l[2]),
        })
    }

    /// The cell next to `p` along `DIRS[d]`, if held.
    #[inline]
    fn step(&self, p: P, d: usize) -> Option<P> {
        let mut l = coords(p.i);
        let a = d / 2;
        l[a] += DIRS[d][a];
        if (0..N).contains(&l[a]) {
            return Some(P {
                slot: p.slot,
                i: local(l[0], l[1], l[2]),
            });
        }
        let n = self.slots[p.slot as usize].near[d];
        (n != NONE).then(|| {
            l[a] = l[a].rem_euclid(N);
            P {
                slot: n,
                i: local(l[0], l[1], l[2]),
            }
        })
    }

    /// The face-local "up" of `p` (+1 on face A, −1 on face B).
    #[inline]
    fn up(&self, p: P) -> i32 {
        up_of_y(self.slots[p.slot as usize].coord.y * N + coords(p.i)[1])
    }

    /// The slot holding the cell above `p` (face-local), None when not held.
    fn above_slot(&self, p: P) -> Option<u32> {
        let d = if self.up(p) > 0 { 2 } else { 3 };
        self.step(p, d).map(|q| q.slot)
    }

    #[inline]
    fn level(&self, p: P, ch: Channel) -> u8 {
        (self.slots[p.slot as usize].get(p.i) >> ch.shift()) & 15
    }

    #[inline]
    fn set_level(&mut self, p: P, ch: Channel, v: u8) {
        let slot = &mut self.slots[p.slot as usize];
        let old = slot.get(p.i);
        let new = (old & !(15 << ch.shift())) | (v << ch.shift());
        if new == old {
            return;
        }
        let uniform = slot.uniform;
        let cells = slot
            .cells
            .get_or_insert_with(|| Box::new([uniform; CHUNK_VOLUME]));
        cells[usize::from(p.i)] = new;
        let first = slot.changed == 0;
        let l = coords(p.i);
        let mut mask = 1 << 13;
        let border = l.iter().any(|&v| v == 0 || v == N - 1);
        let sides = |a: usize| -> [Option<i32>; 3] {
            [
                (l[a] == 0).then_some(-1),
                Some(0),
                (l[a] == N - 1).then_some(1),
            ]
        };
        for dz in sides(2).into_iter().flatten().filter(|_| border) {
            for dy in sides(1).into_iter().flatten() {
                for dx in sides(0).into_iter().flatten() {
                    mask |= 1 << ((dx + 1) + 3 * (dy + 1) + 9 * (dz + 1));
                }
            }
        }
        slot.changed |= mask;
        if first {
            self.touched.push(p.slot);
        }
    }

    // ---- The passes --------------------------------------------------------------------------

    fn run_all(&mut self, voxels: &Voxels, removal: &[P], add: &[P]) {
        let mut cache = VoxelCache::new(voxels);
        for ch in [Channel::Sky, Channel::Block] {
            self.run(&mut cache, ch, removal, add);
        }
        self.finish();
    }

    /// The (filter, emits) of `p`'s block.
    #[inline]
    fn rules(&mut self, cache: &mut VoxelCache<'_>, p: P) -> (u8, u8) {
        let id = cache.voxel(self.slots[p.slot as usize].coord, p.slot, p.i);
        table_rules(&mut self.table, id)
    }

    /// The light `v` at `from` gives the neighbour `to` (`DIRS[d]` from it) with filter `f`.
    #[inline]
    fn passed(&self, ch: Channel, from_v: u8, to: P, d: usize, f: u8) -> u8 {
        let vertical = d / 2 == 1;
        let t = &self.tunables;
        // Sky light falling straight down at full strength through a clear cell keeps it.
        if ch == Channel::Sky
            && vertical
            && from_v == LIGHT_MAX
            && f == 0
            && DIRS[d][1] == -self.up(to)
        {
            return LIGHT_MAX;
        }
        let fall = if vertical {
            t.falloff_vertical
        } else {
            t.falloff_across
        };
        from_v.saturating_sub(fall.max(f))
    }

    /// The light `p` has of itself: its block's emission, or the open sky above it when its
    /// face-local above is in a chunk not held.
    fn source(&mut self, cache: &mut VoxelCache<'_>, ch: Channel, p: P) -> u8 {
        let (f, e) = self.rules(cache, p);
        match ch {
            Channel::Block => e,
            Channel::Sky => {
                if f >= LIGHT_MAX || self.above_slot(p).is_some() {
                    return 0;
                }
                let d = if self.up(p) > 0 { 3 } else { 2 };
                self.passed(ch, LIGHT_MAX, p, d, f)
            }
        }
    }

    fn run(&mut self, cache: &mut VoxelCache<'_>, ch: Channel, removal: &[P], add: &[P]) {
        let mut readd: Vec<P> = add.to_vec();
        let mut q: VecDeque<(P, u8)> = VecDeque::new();
        for &p in removal {
            let v = self.level(p, ch);
            if v > 0 {
                self.set_level(p, ch, 0);
                q.push_back((p, v));
            }
        }
        while let Some((p, ov)) = q.pop_front() {
            readd.push(p);
            for d in 0..6 {
                let Some(n) = self.step(p, d) else {
                    continue;
                };
                let nv = self.level(n, ch);
                if nv == 0 {
                    continue;
                }
                let (f, _) = self.rules(cache, n);
                if nv <= self.passed(ch, ov, n, d, f) {
                    // It may have come from p: cleared, and its own light looked for again.
                    self.set_level(n, ch, 0);
                    q.push_back((n, nv));
                } else {
                    // Lit more than p could light it: it lights the cleared cells back.
                    readd.push(n);
                }
            }
        }
        // Brightest first (a bucket per level): every cell spreads once, at its final level.
        let mut buckets: [Vec<P>; LIGHT_MAX as usize + 1] = Default::default();
        for p in readd {
            let s = self.source(cache, ch, p);
            let v = self.level(p, ch);
            if s > v {
                self.set_level(p, ch, s);
                buckets[usize::from(s)].push(p);
            } else if v > 0 {
                buckets[usize::from(v)].push(p);
            }
        }
        for level in (1..=LIGHT_MAX).rev() {
            while let Some(p) = buckets[usize::from(level)].pop() {
                if self.level(p, ch) != level {
                    continue; // raised since, and spread from there
                }
                for d in 0..6 {
                    let Some(n) = self.step(p, d) else {
                        continue;
                    };
                    let (f, _) = self.rules(cache, n);
                    if f >= LIGHT_MAX {
                        continue;
                    }
                    let nv = self.passed(ch, level, n, d, f);
                    if nv > self.level(n, ch) {
                        self.set_level(n, ch, nv);
                        buckets[usize::from(nv)].push(n);
                    }
                }
            }
        }
    }

    /// Records the changed chunks and folds uniform arrays back into one value.
    fn finish(&mut self) {
        for s in std::mem::take(&mut self.touched) {
            let slot = &mut self.slots[s as usize];
            let (c, mask) = (slot.coord, slot.changed);
            slot.changed = 0;
            if let Some(cells) = &slot.cells
                && cells.iter().all(|&v| v == cells[0])
            {
                slot.uniform = cells[0];
                slot.cells = None;
            }
            for b in 0..27 {
                if mask & (1 << b) != 0 {
                    let (dx, dy, dz) = (b % 3 - 1, (b / 3) % 3 - 1, b / 9 - 1);
                    self.changed
                        .push(ChunkCoord::new(c.x + dx, c.y + dy, c.z + dz));
                }
            }
        }
    }
}

/// A newly held chunk, for [`LightMap::load`]'s fast paths.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// All air under the open sky (or a chunk all at full sky light).
    Sky,
    /// Every cell lets no light in and gives none off.
    Shut,
    Mixed,
}

/// The (filter, emits) of state `id`, looked up in the registry once.
#[inline]
fn table_rules(table: &mut Vec<(u8, u8)>, id: StateId) -> (u8, u8) {
    let k = usize::from(id);
    if let Some(&r) = table.get(k) {
        return r;
    }
    let old = table.len();
    table.resize(k + 1, (0, 0));
    for (j, r) in table.iter_mut().enumerate().skip(old) {
        *r = dwell_blocks::try_info(j as StateId).map_or((0, 0), |s| {
            (s.light_filter.min(LIGHT_MAX), s.emits.min(LIGHT_MAX))
        });
    }
    table[k]
}

const fn opposite(d: usize) -> usize {
    d ^ 1
}

/// The cells of slot `s` on its side `DIRS[d]`.
fn border(s: u32, d: usize) -> impl Iterator<Item = P> {
    let a = d / 2;
    let at = if DIRS[d][a] > 0 { N - 1 } else { 0 };
    (0..N * N).map(move |k| {
        let (j, m) = (k % N, k / N);
        let mut l = [0; 3];
        l[a] = at;
        l[(a + 1) % 3] = j;
        l[(a + 2) % 3] = m;
        P {
            slot: s,
            i: local(l[0], l[1], l[2]),
        }
    })
}

/// The voxels of the chunks a pass reads, looked up once per chunk per pass.
struct VoxelCache<'a> {
    voxels: &'a Voxels,
    by_slot: Vec<Option<Option<&'a [StateId; CHUNK_VOLUME]>>>,
}

impl<'a> VoxelCache<'a> {
    fn new(voxels: &'a Voxels) -> Self {
        Self {
            voxels,
            by_slot: Vec::new(),
        }
    }

    #[inline]
    fn voxel(&mut self, coord: ChunkCoord, slot: u32, i: u16) -> StateId {
        let s = slot as usize;
        if self.by_slot.len() <= s {
            self.by_slot.resize(s + 1, None);
        }
        let v = *self.by_slot[s].get_or_insert_with(|| self.voxels.get(&coord).map(|c| c.voxels()));
        v.map_or(0, |v| v[usize::from(i)])
    }
}

#[cfg(test)]
#[path = "light_tests.rs"]
mod tests;
