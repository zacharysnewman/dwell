//! The air test's sky floors computed off the tick (ARCHITECTURE.md §6.3). Streaming, the
//! prefetch and the requests ask whether a chunk is all air (`AirColumns::known`); a chunk whose
//! column's sky floor is not known yet waits, and its column is wanted here. Wanted columns are
//! computed by a thread (the dedicated server, with worldgen threads), by the browser's
//! server-jobs worker (`take_job`, `deliver`: remote), or on the tick within the worldgen budget
//! (inline: tests, the fallback), and kept in the `AirColumns` cache. One sky floor is a chunk's
//! column heights: ~1 ms natively, a few in the browser, so a tick never computes more than its
//! budget's worth.
//!
//! The wanted list is asked again each tick, nearest first: `begin` forgets what is not out yet.

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use dwell_world::Face;
use dwell_worldgen::{AirColumns, SkyColumn};

use crate::clock::Deadline;

/// The most columns out at the thread or the server-jobs worker at once (the queue's order stays
/// current).
pub const IN_FLIGHT: usize = 8;

enum Mode {
    Inline,
    Thread {
        jobs: Sender<SkyColumn>,
        done: Receiver<SkyColumn>,
    },
    Remote,
}

pub struct SkyColumns {
    air: Arc<AirColumns>,
    mode: Mode,
    queue: VecDeque<SkyColumn>,
    queued: HashSet<SkyColumn>,
    in_flight: HashSet<SkyColumn>,
    /// Sky floors computed, for tests and diagnostics.
    pub computed: u64,
}

impl SkyColumns {
    /// Inline (computed on the tick within `run_inline`'s budget), on a thread, or remote.
    pub fn new(air: Arc<AirColumns>, thread: bool, remote: bool) -> Self {
        let mode = if remote {
            Mode::Remote
        } else if thread {
            let (jobs, rx) = channel::<SkyColumn>();
            let (tx, done) = channel();
            let a = air.clone();
            std::thread::spawn(move || {
                for col in rx {
                    a.insert(col, a.compute(col));
                    if tx.send(col).is_err() {
                        break;
                    }
                }
            });
            Mode::Thread { jobs, done }
        } else {
            Mode::Inline
        };
        Self {
            air,
            mode,
            queue: VecDeque::new(),
            queued: HashSet::new(),
            in_flight: HashSet::new(),
            computed: 0,
        }
    }

    pub fn air(&self) -> &AirColumns {
        &self.air
    }

    /// A tick's start: what finished is collected, and what is not out yet forgotten (the tick
    /// asks again, nearest first).
    pub fn begin(&mut self) {
        if let Mode::Thread { done, .. } = &self.mode {
            for col in done.try_iter() {
                self.in_flight.remove(&col);
                self.computed += 1;
            }
        }
        self.queue.clear();
        self.queued.clear();
    }

    /// Column `col`'s sky floor is wanted (after those wanted before it this tick).
    pub fn want(&mut self, col: SkyColumn) {
        if self.in_flight.contains(&col) || !self.queued.insert(col) {
            return;
        }
        self.queue.push_back(col);
        if let Mode::Thread { jobs, .. } = &self.mode
            && self.in_flight.len() < IN_FLIGHT
            && let Some(c) = self.queue.pop_front()
        {
            self.in_flight.insert(c);
            let _ = jobs.send(c);
        }
    }

    /// Inline: the wanted columns computed here, nearest first, until the budget is spent (at
    /// least one a call, so they always progress).
    pub fn run_inline(&mut self, budget: Duration) {
        if !matches!(self.mode, Mode::Inline) {
            return;
        }
        let deadline = Deadline::after(budget);
        while let Some(col) = self.queue.pop_front() {
            self.queued.remove(&col);
            self.air.insert(col, self.air.compute(col));
            self.computed += 1;
            if deadline.passed() {
                break;
            }
        }
    }

    /// Remote: the next column to compute elsewhere (`encode_job`), a few out at once.
    pub fn take_job(&mut self) -> Option<SkyColumn> {
        if !matches!(self.mode, Mode::Remote) || self.in_flight.len() >= IN_FLIGHT {
            return None;
        }
        let col = self.queue.pop_front()?;
        self.queued.remove(&col);
        self.in_flight.insert(col);
        Some(col)
    }

    /// Remote: a column's sky floor computed elsewhere.
    pub fn deliver(&mut self, col: SkyColumn, floor: f32) {
        if self.in_flight.remove(&col) {
            self.air.insert(col, floor);
            self.computed += 1;
        }
    }

    /// Columns wanted or out.
    pub fn pending(&self) -> usize {
        self.queue.len() + self.in_flight.len()
    }
}

/// A sky-floor job, as bytes: "SJ1", the column's x and z (i32, little-endian) and its face (u8).
pub fn encode_job(col: SkyColumn) -> Vec<u8> {
    let mut out = b"SJ1".to_vec();
    out.extend_from_slice(&col.x.to_le_bytes());
    out.extend_from_slice(&col.z.to_le_bytes());
    out.push(col.face as u8);
    out
}

fn column(b: &[u8]) -> Option<SkyColumn> {
    let x = i32::from_le_bytes(b.get(3..7)?.try_into().ok()?);
    let z = i32::from_le_bytes(b.get(7..11)?.try_into().ok()?);
    let face = match *b.get(11)? {
        0 => Face::A,
        1 => Face::B,
        _ => return None,
    };
    Some(SkyColumn { x, z, face })
}

pub fn decode_job(b: &[u8]) -> Option<SkyColumn> {
    (b.len() == 12 && b.starts_with(b"SJ1")).then(|| column(b))?
}

/// Its answer: "SD1", the column as in the job, and the sky floor (f32, little-endian).
pub fn encode_done(col: SkyColumn, floor: f32) -> Vec<u8> {
    let mut out = encode_job(col);
    out[1] = b'D';
    out.extend_from_slice(&floor.to_le_bytes());
    out
}

pub fn decode_done(b: &[u8]) -> Option<(SkyColumn, f32)> {
    if b.len() != 16 || !b.starts_with(b"SD1") {
        return None;
    }
    Some((column(b)?, f32::from_le_bytes(b[12..16].try_into().ok()?)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sky_floor_job_round_trips() {
        // both-faces: the encoding carries the face; one column of each.
        for face in [Face::A, Face::B] {
            let col = SkyColumn {
                x: -7,
                z: 232,
                face,
            };
            assert_eq!(decode_job(&encode_job(col)), Some(col));
            assert_eq!(decode_done(&encode_done(col, 81.5)), Some((col, 81.5)));
        }
        assert!(decode_job(b"SJ1").is_none());
        assert!(
            decode_done(&encode_job(SkyColumn {
                x: 0,
                z: 0,
                face: Face::A
            }))
            .is_none()
        );
    }

    #[test]
    fn a_remote_column_is_kept_once_delivered_and_only_while_out() {
        // face-a-only: the queue's bookkeeping has no up.
        let air = AirColumns::for_generator(dwell_world::GENERATOR_TERRAIN, 0);
        let mut s = SkyColumns::new(air.clone(), false, true);
        let col = SkyColumn {
            x: 73,
            z: 232,
            face: Face::A,
        };
        let c = dwell_world::ChunkCoord::new(
            73,
            dwell_world::SEA_LEVEL / dwell_world::CHUNK_SIZE + 1,
            232,
        );
        assert_eq!(air.known(c), Err(col));
        s.begin();
        s.want(col);
        s.want(col);
        assert_eq!(s.pending(), 1);
        assert_eq!(s.take_job(), Some(col));
        assert_eq!(s.take_job(), None);
        s.begin();
        s.want(col); // out already: not again
        assert_eq!(s.take_job(), None);
        s.deliver(col, 100.0);
        assert_eq!(s.pending(), 0);
        assert_eq!(
            air.known(c),
            Ok(dwell_worldgen::BifacialTerrain::is_air_chunk_with(
                &dwell_world::WorldBounds::DEFAULT,
                c,
                100.0
            ))
        );
    }
}
