//! Blueprints (WORK_QUEUE item 55, docs/BLUEPRINTS.md): a structure's cells placed in the world as
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
