//! Blueprints (WORK_QUEUE item 56, docs/BLUEPRINTS.md): a structure's cells placed in the world as
//! an entity shown as ghost blocks, built by players and builder NPCs. Everyone in the world sees
//! them; the server keeps them in the world's optional `blueprints` table and sends them to every
//! client with the `blueprints.world` capability (`Blueprints`).
//!
//! The record: `u32 id`, `str name` (the structure's, for the panel), `u8 face` (0 A, 1 B),
//! `i32 x, y, z` (the face-local cell the anchor stands on), `u8 turn` (quarter turns clockwise
//! seen from above, 0–3), `u8 sx, sy, sz` (each 1..=[`MAX_SIDE`]), `u8 ax, ay, az` (the anchor,
//! inside the box), `u8 n` palette names (at most [`MAX_PALETTE`]) each a `str` block state name,
//! then the cells (x fastest, then z, then y) as runs: `u16 run` (≥ 1), `u8 cell` (0 keeps what
//! is there, k the palette's k-th name), the runs summing to the box's volume. Names a build
//! doesn't know are kept as they are; their cells are left out where it resolves them.

use crate::bytes::{ByteReader, ByteWriter};

/// The most cells along a side (a one-file structure's, STRUCTURES.md §3).
pub const MAX_SIDE: u8 = 32;
/// The most block states a blueprint's palette names.
pub const MAX_PALETTE: usize = 64;
/// The longest structure or state name (bytes).
pub const NAME_BYTES: usize = 96;

/// One placed blueprint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blueprint {
    /// The server's id (0 in a client's `Place`).
    pub id: u32,
    /// The structure it was made from (`dwell:bp_hut`), for the panel.
    pub name: String,
    /// 0 face A, 1 face B.
    pub face: u8,
    /// The face-local cell its anchor stands on.
    pub at: [i32; 3],
    /// Quarter turns clockwise seen from above, 0–3.
    pub turn: u8,
    pub size: [u8; 3],
    pub anchor: [u8; 3],
    /// Block state names; a cell's value k > 0 is `palette[k - 1]`.
    pub palette: Vec<String>,
    /// x fastest, then z, then y; 0 keeps what is there.
    pub cells: Vec<u8>,
}

impl Blueprint {
    pub fn volume(&self) -> usize {
        self.size.iter().map(|&n| usize::from(n)).product()
    }

    pub fn encode_into(&self, w: &mut ByteWriter) {
        w.u32(self.id);
        w.str(&self.name);
        w.u8(self.face);
        for v in self.at {
            w.i32(v);
        }
        w.u8(self.turn);
        for v in self.size {
            w.u8(v);
        }
        for v in self.anchor {
            w.u8(v);
        }
        w.u8(self.palette.len() as u8);
        for n in &self.palette {
            w.str(n);
        }
        let mut i = 0;
        while i < self.cells.len() {
            let c = self.cells[i];
            let mut run = 1;
            while i + run < self.cells.len()
                && self.cells[i + run] == c
                && run < usize::from(u16::MAX)
            {
                run += 1;
            }
            w.u16(run as u16);
            w.u8(c);
            i += run;
        }
    }

    /// Reads a record, failing `r` on a bad one: a turn past 3, a face past B, a side of 0 or past
    /// [`MAX_SIDE`], an anchor outside the box, too many or empty names, a cell past the palette,
    /// runs that don't sum to the volume.
    pub fn decode_from(r: &mut ByteReader) -> Self {
        let id = r.u32();
        let name = r.str(NAME_BYTES);
        let face = r.u8();
        r.check(face <= 1);
        let at = [r.i32(), r.i32(), r.i32()];
        let turn = r.u8();
        r.check(turn <= 3);
        let size = [r.u8(), r.u8(), r.u8()];
        r.check(size.iter().all(|&n| (1..=MAX_SIDE).contains(&n)));
        let anchor = [r.u8(), r.u8(), r.u8()];
        r.check((0..3).all(|i| anchor[i] < size[i]));
        let n = usize::from(r.u8());
        r.check(n <= MAX_PALETTE);
        let mut palette = Vec::new();
        for _ in 0..n {
            if !r.ok() {
                break;
            }
            let s = r.str(NAME_BYTES);
            r.check(!s.is_empty());
            palette.push(s);
        }
        let volume: usize = size.iter().map(|&n| usize::from(n)).product();
        let mut cells = Vec::with_capacity(if r.ok() { volume } else { 0 });
        while r.ok() && cells.len() < volume {
            let run = usize::from(r.u16());
            let c = r.u8();
            r.check(run >= 1 && cells.len() + run <= volume && usize::from(c) <= n);
            if r.ok() {
                cells.extend(std::iter::repeat_n(c, run));
            }
        }
        Self {
            id,
            name,
            face,
            at,
            turn,
            size,
            anchor,
            palette,
            cells,
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.encode_into(&mut ByteWriter::new(&mut out));
        out
    }

    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        let mut r = ByteReader::new(b);
        let m = Self::decode_from(&mut r);
        (r.ok() && r.at_end()).then_some(m)
    }
}

/// The `Blueprints` message (reliable, world channel, only with `blueprints.world` on both sides).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlueprintsMessage {
    /// S→C once after the client's `Capabilities`: every blueprint in the world (op 0).
    All(Vec<Blueprint>),
    /// S→C: a blueprint placed (op 1).
    Added(Blueprint),
    /// S→C: a blueprint gone, completed or removed (op 2).
    Removed(u32),
    /// C→S: place this blueprint (its id is ignored) (op 3).
    Place(Blueprint),
    /// C→S: remove blueprint `id` (op 4).
    Remove(u32),
}

/// The most blueprints one `All` carries.
pub const MAX_ALL: usize = 1024;

impl BlueprintsMessage {
    pub fn encode_into(&self, w: &mut ByteWriter) {
        match self {
            Self::All(list) => {
                w.u8(0);
                w.u16(list.len() as u16);
                for b in list {
                    b.encode_into(w);
                }
            }
            Self::Added(b) => {
                w.u8(1);
                b.encode_into(w);
            }
            Self::Removed(id) => {
                w.u8(2);
                w.u32(*id);
            }
            Self::Place(b) => {
                w.u8(3);
                b.encode_into(w);
            }
            Self::Remove(id) => {
                w.u8(4);
                w.u32(*id);
            }
        }
    }

    pub fn decode_from(r: &mut ByteReader) -> Self {
        match r.u8() {
            0 => {
                let n = usize::from(r.u16());
                r.check(n <= MAX_ALL);
                let mut list = Vec::new();
                for _ in 0..n {
                    if !r.ok() {
                        break;
                    }
                    list.push(Blueprint::decode_from(r));
                }
                Self::All(list)
            }
            1 => Self::Added(Blueprint::decode_from(r)),
            2 => Self::Removed(r.u32()),
            3 => Self::Place(Blueprint::decode_from(r)),
            4 => Self::Remove(r.u32()),
            _ => {
                r.check(false);
                Self::Remove(0)
            }
        }
    }
}

/// The saved list's format version: 2 adds the towns founded (1 had none); 3 the villagers' work
/// (B5: the storehouses and the builders' carried loads).
pub const LIST_VERSION: u8 = 3;

/// The longest block state name a saved load or stock names.
pub const MAX_STATE_NAME: usize = 128;
/// The most storehouses, loads, or kinds in one stock or load a saved record holds.
pub const MAX_WORK_ENTRIES: usize = 4096;

/// A storehouse as saved (B5, docs/BLUEPRINTS.md §9): its id, face, box (world cells, both
/// corners in), the blueprint still building it (0: built, open), and its stock: None unlimited,
/// else the counted items by block state name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedStorehouse {
    pub id: u32,
    pub face: u8,
    pub min: [i32; 3],
    pub max: [i32; 3],
    pub blueprint: u32,
    pub counted: Option<Vec<(String, u32)>>,
}

/// A builder's carried load as saved: the blueprint it works, its items by block state name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedLoad {
    pub blueprint: u32,
    pub items: Vec<(String, u32)>,
}

/// The villagers' work as saved (format 3; empty in older ones).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SavedWork {
    pub next_storehouse: u32,
    pub storehouses: Vec<SavedStorehouse>,
    pub loads: Vec<SavedLoad>,
}

fn write_items(w: &mut ByteWriter, items: &[(String, u32)]) {
    let n = items.len().min(MAX_WORK_ENTRIES);
    w.u16(n as u16);
    for (name, count) in &items[..n] {
        w.str(crate::bytes::clamp_str(name, MAX_STATE_NAME));
        w.u32(*count);
    }
}

fn read_items(r: &mut ByteReader) -> Vec<(String, u32)> {
    let n = usize::from(r.u16());
    r.check(n <= MAX_WORK_ENTRIES);
    let mut out = Vec::new();
    for _ in 0..n {
        if !r.ok() {
            break;
        }
        out.push((r.str(MAX_STATE_NAME), r.u32()));
    }
    out
}

/// The world's blueprints as saved (the `blueprints` table): `u8 version` (3), `u32 next id`,
/// `u16 count`, each a record; then `u32 count` towns founded, each `u8 face`, `i32 i, k` (its
/// grid cell); then the work (version 3): `u32 next storehouse id`, `u16 count` storehouses (each
/// `u32 id`, `u8 face`, `i32 × 3` min, `i32 × 3` max, `u32 blueprint`, `u8 stock` 0 unlimited or 1
/// counted followed by its items), `u16 count` loads (each `u32 blueprint` and its items); items
/// are `u16 count`, each `str state name`, `u32 count`. Version 1 ends after the records, 2 after
/// the towns.
pub fn list_to_bytes(
    next_id: u32,
    list: &[Blueprint],
    towns: &[(u8, i32, i32)],
    work: &SavedWork,
) -> Vec<u8> {
    let mut out = Vec::new();
    let mut w = ByteWriter::new(&mut out);
    w.u8(LIST_VERSION);
    w.u32(next_id);
    w.u16(list.len() as u16);
    for b in list {
        b.encode_into(&mut w);
    }
    w.u32(towns.len() as u32);
    for &(f, i, k) in towns {
        w.u8(f);
        w.i32(i);
        w.i32(k);
    }
    w.u32(work.next_storehouse);
    let n = work.storehouses.len().min(MAX_WORK_ENTRIES);
    w.u16(n as u16);
    for s in &work.storehouses[..n] {
        w.u32(s.id);
        w.u8(s.face);
        for v in s.min.iter().chain(&s.max) {
            w.i32(*v);
        }
        w.u32(s.blueprint);
        match &s.counted {
            None => w.u8(0),
            Some(items) => {
                w.u8(1);
                write_items(&mut w, items);
            }
        }
    }
    let n = work.loads.len().min(MAX_WORK_ENTRIES);
    w.u16(n as u16);
    for l in &work.loads[..n] {
        w.u32(l.blueprint);
        write_items(&mut w, &l.items);
    }
    out
}

/// What a saved list holds: the next id, the blueprints, the towns founded, the work.
pub type SavedList = (u32, Vec<Blueprint>, Vec<(u8, i32, i32)>, SavedWork);

/// A saved list (version 1, 2 or 3), or None for a damaged one or a later version.
pub fn list_from_bytes(bytes: &[u8]) -> Option<SavedList> {
    let mut r = ByteReader::new(bytes);
    let version = r.u8();
    r.check((1..=3).contains(&version));
    let next = r.u32();
    let n = usize::from(r.u16());
    let mut list = Vec::new();
    for _ in 0..n {
        if !r.ok() {
            break;
        }
        list.push(Blueprint::decode_from(&mut r));
    }
    let mut towns = Vec::new();
    if version >= 2 {
        let t = r.u32() as usize;
        r.check(t <= 1 << 20);
        for _ in 0..t {
            if !r.ok() {
                break;
            }
            towns.push((r.u8(), r.i32(), r.i32()));
        }
    }
    let mut work = SavedWork::default();
    if version >= 3 {
        work.next_storehouse = r.u32();
        let n = usize::from(r.u16());
        r.check(n <= MAX_WORK_ENTRIES);
        for _ in 0..n {
            if !r.ok() {
                break;
            }
            let (id, face) = (r.u32(), r.u8());
            let min = [r.i32(), r.i32(), r.i32()];
            let max = [r.i32(), r.i32(), r.i32()];
            let blueprint = r.u32();
            let mode = r.u8();
            r.check(mode <= 1);
            let counted = (mode == 1).then(|| read_items(&mut r));
            work.storehouses.push(SavedStorehouse {
                id,
                face,
                min,
                max,
                blueprint,
                counted,
            });
        }
        let n = usize::from(r.u16());
        r.check(n <= MAX_WORK_ENTRIES);
        for _ in 0..n {
            if !r.ok() {
                break;
            }
            let blueprint = r.u32();
            work.loads.push(SavedLoad {
                blueprint,
                items: read_items(&mut r),
            });
        }
    }
    (r.ok() && r.at_end()).then_some((next, list, towns, work))
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn sample() -> Blueprint {
        let mut cells = vec![0u8; 3 * 2 * 3];
        cells[..9].fill(1);
        cells[13] = 2;
        Blueprint {
            id: 7,
            name: "dwell:bp_well".into(),
            face: 1,
            at: [-40, 66, 900],
            turn: 3,
            size: [3, 2, 3],
            anchor: [1, 0, 1],
            palette: vec!["dwell:stone".into(), "mymod:brick[wet=true]".into()],
            cells,
        }
    }

    #[test]
    fn a_blueprint_round_trips_its_unknown_names_kept() {
        let b = sample();
        assert_eq!(Blueprint::from_bytes(&b.to_bytes()), Some(b.clone()));
        for m in [
            BlueprintsMessage::All(vec![b.clone(), Blueprint { id: 8, ..b.clone() }]),
            BlueprintsMessage::Added(b.clone()),
            BlueprintsMessage::Removed(9),
            BlueprintsMessage::Place(b),
            BlueprintsMessage::Remove(3),
        ] {
            let mut out = Vec::new();
            m.encode_into(&mut ByteWriter::new(&mut out));
            let mut r = ByteReader::new(&out);
            assert_eq!(BlueprintsMessage::decode_from(&mut r), m);
            assert!(r.ok() && r.at_end());
        }
    }

    #[test]
    fn the_saved_list_round_trips() {
        let list = vec![sample(), Blueprint { id: 9, ..sample() }];
        let towns = vec![(0, 3, -4), (1, -1, 2)];
        let work = SavedWork {
            next_storehouse: 3,
            storehouses: vec![
                SavedStorehouse {
                    id: 1,
                    face: 1,
                    min: [-3, 70, 4],
                    max: [1, 74, 7],
                    blueprint: 0,
                    counted: None,
                },
                SavedStorehouse {
                    id: 2,
                    face: 0,
                    min: [0, 0, 0],
                    max: [4, 4, 3],
                    blueprint: 7,
                    counted: Some(vec![("dwell:log[axis=y]".into(), 40)]),
                },
            ],
            loads: vec![SavedLoad {
                blueprint: 9,
                items: vec![("dwell:stone".into(), 6), ("dwell:log".into(), 4)],
            }],
        };
        assert_eq!(
            list_from_bytes(&list_to_bytes(10, &list, &towns, &work)),
            Some((10, list.clone(), towns.clone(), work))
        );
        // Version 2 (B4's): no work after the towns.
        let empty = SavedWork::default();
        let mut v2 = list_to_bytes(10, &list, &towns, &empty);
        v2.truncate(v2.len() - 8);
        v2[0] = 2;
        assert_eq!(
            list_from_bytes(&v2),
            Some((10, list.clone(), towns, empty.clone()))
        );
        // Version 1 (B1–B3's): no towns after the records.
        let mut v1 = list_to_bytes(10, &list, &[], &empty);
        v1.truncate(v1.len() - 12);
        v1[0] = 1;
        assert_eq!(list_from_bytes(&v1), Some((10, list, vec![], empty)));
        assert_eq!(list_from_bytes(&[4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]), None);
    }

    #[test]
    fn the_cells_go_as_runs() {
        let b = Blueprint {
            size: [32, 32, 32],
            cells: vec![1; 32 * 32 * 32],
            ..sample()
        };
        let bytes = b.to_bytes();
        assert!(bytes.len() < 100, "{} bytes", bytes.len());
        assert_eq!(Blueprint::from_bytes(&bytes), Some(b));
    }

    #[test]
    fn bad_records_are_refused() {
        let bad = |edit: &dyn Fn(&mut Blueprint)| {
            let mut b = sample();
            edit(&mut b);
            Blueprint::from_bytes(&b.to_bytes())
        };
        assert!(bad(&|b| b.turn = 4).is_none());
        assert!(bad(&|b| b.face = 2).is_none());
        assert!(bad(&|b| b.anchor = [3, 0, 0]).is_none());
        assert!(
            bad(&|b| b.cells[0] = 3).is_none(),
            "a cell past the palette"
        );
        assert!(bad(&|b| b.palette.push(String::new())).is_none());
        // Runs short of the volume, or past it.
        let mut short = sample().to_bytes();
        short.truncate(short.len() - 3);
        assert!(Blueprint::from_bytes(&short).is_none());
        let mut size0 = sample();
        size0.size = [0, 2, 3];
        size0.cells.clear();
        assert!(Blueprint::from_bytes(&size0.to_bytes()).is_none());
    }
}
