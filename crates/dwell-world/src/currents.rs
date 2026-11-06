//! Water currents (docs/WATER_SURFACE_GRID.md §2c, Phase 53 G3a): the horizontal velocity (m/s) of
//! the water in each column, kept per water chunk of `CS` × `CS` columns of one face, read by the
//! swimmers' controller and the floating bodies on the server and in the client's prediction alike.
//! The server's water grid writes its chunks' currents; a client keeps what the server sends. A
//! column with no entry is still water.
//!
//! The faces mirror only up and down (`FaceFrame`), so a column's current is the same vector in the
//! world and face-locally.

use crate::FaceFrame;
use crate::hash::HashMap;

/// Columns along a water chunk's side (`dwell-sim` `water::CS`).
pub const CS: i32 = 16;
/// Columns in a water chunk.
pub const CA: usize = (CS * CS) as usize;

/// A water chunk: its face (true: face B) and its chunk column.
pub type CurrentKey = (bool, i32, i32);

/// A water chunk's currents, a column's (m/s along x, along z) at `x % CS + CS × (z % CS)`.
pub type ChunkCurrents = Box<[[f32; 2]; CA]>;

/// The water chunk holding world column (`x`, `z`) on face `f`, and the column's index in it.
pub fn current_key(f: FaceFrame, x: i32, z: i32) -> (CurrentKey, usize) {
    let (cx, cz) = (x.div_euclid(CS), z.div_euclid(CS));
    let i = (x.rem_euclid(CS) + CS * z.rem_euclid(CS)) as usize;
    ((f.is_b(), cx, cz), i)
}

/// The currents of the water chunks that have any.
#[derive(Default)]
pub struct Currents {
    chunks: HashMap<CurrentKey, ChunkCurrents>,
}

impl Currents {
    /// Water chunk `key`'s currents (none: still).
    pub fn set(&mut self, key: CurrentKey, currents: Option<ChunkCurrents>) {
        match currents {
            Some(c) => {
                self.chunks.insert(key, c);
            }
            None => {
                self.chunks.remove(&key);
            }
        }
    }

    /// Water chunk `key`'s currents, if it has any.
    pub fn get(&self, key: CurrentKey) -> Option<&ChunkCurrents> {
        self.chunks.get(&key)
    }

    /// The water chunks with currents.
    pub fn keys(&self) -> impl Iterator<Item = CurrentKey> + '_ {
        self.chunks.keys().copied()
    }

    /// The current (m/s along x and z) at world point `p`: its column's on the face `p` is on.
    pub fn at(&self, p: [f64; 3]) -> [f32; 2] {
        let f = FaceFrame::at_height(p[1]);
        let (key, i) = current_key(f, p[0].floor() as i32, p[2].floor() as i32);
        self.chunks.get(&key).map_or([0.0; 2], |c| c[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    crate::both_faces!(
        fn a_columns_current_is_read_where_it_was_set(f: FaceFrame) {
            let mut c = Currents::default();
            let (key, i) = current_key(f, -3, 17);
            assert_eq!(key, (f.is_b(), -1, 1));
            let mut cells = Box::new([[0.0; 2]; CA]);
            cells[i] = [0.5, -0.25];
            c.set(key, Some(cells));
            let y = f.y(f64::from(crate::FLAT_TOP_Y) + 1.0);
            assert_eq!(c.at([-2.5, y, 17.5]), [0.5, -0.25]);
            assert_eq!(
                c.at([-1.5, y, 17.5]),
                [0.0, 0.0],
                "the next column is still"
            );
            // The other face's column is another chunk.
            assert_eq!(
                c.at([-2.5, f.other().y(f64::from(crate::FLAT_TOP_Y) + 1.0), 17.5]),
                [0.0; 2]
            );
            c.set(key, None);
            assert_eq!(c.at([-2.5, y, 17.5]), [0.0, 0.0]);
        }
    );
}
