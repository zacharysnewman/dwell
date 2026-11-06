//! The water host (ADR 0062, docs/WATER_SURFACE_GRID.md §2d): the water grid behind one interface of
//! messages, placed **inline** (on the calling thread: tests, a fallback), on a **native thread**
//! (`std::thread` and channels), or **remote** (the browser: a second Web Worker the local-mode
//! worker starts; the host only queues and takes the messages' bytes).
//!
//! The server sends [`In`] batches (chunks loaded and unloaded as cells, cells edited, the
//! players, the water time to run, requests for the record and reports) and reads [`Out`] batches at
//! its next tick's start (the water written, each write with the byte it replaced, currents, the
//! chunks the water wants loaded, the record, reports). A batch is one binary message
//! ([`encode_in`], [`encode_out`]); the native thread and the browser worker carry the same bytes.
//!
//! Off the server's thread the water falls behind instead of slowing physics: the water time it
//! hasn't run is owed and run as fast as it can (`threadStepBudget` column-substeps a run, so it
//! still answers between runs), and past `maxOwedS` the rest is dropped (its clock slows).

use std::collections::VecDeque;
use std::time::Duration;

use dwell_world::ChunkCoord;
use dwell_world::currents::ChunkCurrents;

use super::cells::ChunkCells;
use super::{CA, CurrentsKey, Water, Write};

/// What the server sends the water.
#[derive(Debug, PartialEq)]
pub enum In {
    /// The water of a world (`Water::for_world`; a remote host's first message).
    Init {
        generator_version: u32,
        world_seed: u64,
        map: bool,
    },
    /// The saved record (`Water::save`), before any chunk.
    Restore(Vec<u8>),
    /// A chunk's cells; `fresh`: new to the water (its water is read).
    Load {
        at: ChunkCoord,
        cells: ChunkCells,
        fresh: bool,
    },
    Unload(ChunkCoord),
    /// A cell edited: its new byte (`cells::byte_of`); its column is touched.
    Edit {
        cell: [i32; 3],
        byte: u8,
    },
    /// The players' positions.
    Players(Vec<[f64; 3]>),
    /// Water time to run (s).
    Run(f64),
    /// Asks for the record (`Out::Record`).
    Record,
    /// Asks for a report (`Out::Report`).
    Report {
        at: [i32; 3],
        radius: i32,
    },
}

/// The water thread's own figures, with each run (telemetry, docs/DEV_TOOLS.md).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    /// Water time run by this run, still owed after it, and dropped in all (s).
    pub ran_s: f64,
    pub owed_s: f64,
    pub dropped_s: f64,
    /// The run's wall time (ms).
    pub busy_ms: f64,
    /// The water's clock (s).
    pub t: f64,
    pub awake: u32,
    pub chunks: u32,
    /// The copy of the world's bytes (`Cells::bytes`) and chunks.
    pub cell_bytes: u64,
    pub cell_chunks: u32,
}

/// What the water sends back.
#[derive(Debug, PartialEq)]
pub enum Out {
    /// A run: the cells written, the currents, the chunks it wants loaded, its figures.
    Ran {
        writes: Vec<Write>,
        currents: Vec<(CurrentsKey, Option<ChunkCurrents>)>,
        wanted: Vec<ChunkCoord>,
        stats: Stats,
    },
    Record(Vec<u8>),
    Report(String),
    /// The saved record restored (`Water::restore`: its chunks, or None if unreadable).
    Restored(Option<usize>),
}

/// How the water's own side runs (`WaterCore`).
#[derive(Clone, Copy, Debug)]
pub struct CoreConfig {
    /// The most column-substeps a run takes.
    pub budget: usize,
    /// The most water time (s) owed; past it the rest is dropped (0: what a run didn't do is
    /// dropped at once, as inline).
    pub max_owed: f64,
    /// A wait added to each run (tests: water slower than any tick).
    pub slow: Duration,
}

impl CoreConfig {
    /// Inline: the server's thread's step budget, nothing owed.
    pub fn inline(w: &Water) -> Self {
        Self {
            budget: w.cfg.step_budget,
            max_owed: 0.0,
            slow: Duration::ZERO,
        }
    }
    /// On its own thread: the thread's budget, `maxOwedS` owed (`shared/fluids/flow.json`).
    pub fn thread() -> Self {
        Self {
            budget: super::FLOW.thread_step_budget as usize,
            max_owed: super::FLOW.max_owed_s,
            slow: Duration::ZERO,
        }
    }
}

/// The water's side of the host: the grid and what it owes; fed [`In`], answers [`Out`].
pub struct WaterCore {
    water: Water,
    cfg: CoreConfig,
    owed: f64,
    dropped: f64,
    /// A run is due (a `Run` came since the last).
    due: bool,
    /// The last batch received.
    seq: u32,
    record: bool,
    reports: Vec<([i32; 3], i32)>,
    restored: Vec<Option<usize>>,
    players: Vec<[f64; 3]>,
}

impl WaterCore {
    pub fn new(water: Water, cfg: CoreConfig) -> Self {
        Self {
            water,
            cfg,
            owed: 0.0,
            dropped: 0.0,
            due: false,
            seq: 0,
            record: false,
            reports: Vec::new(),
            restored: Vec::new(),
            players: Vec::new(),
        }
    }

    pub fn water(&self) -> &Water {
        &self.water
    }

    /// The last batch received.
    pub fn seq(&self) -> u32 {
        self.seq
    }

    /// Takes batch `seq`: its cells into the copy at once, its time owed, its requests kept for
    /// the next [`WaterCore::work`].
    pub fn receive(&mut self, seq: u32, items: Vec<In>) {
        self.seq = seq;
        for item in items {
            match item {
                In::Init {
                    generator_version,
                    world_seed,
                    map,
                } => self.water = Water::for_world(generator_version, world_seed, map),
                In::Restore(b) => {
                    let r = self.water.restore(&b, |_| 0);
                    self.restored.push(r);
                }
                In::Load { at, cells, fresh } => self.water.load(at, cells, fresh),
                In::Unload(at) => self.water.unload(at),
                In::Edit { cell, byte } => self.water.edited(cell, byte),
                In::Players(p) => self.players = p,
                In::Run(dt) => {
                    self.owed += dt.max(0.0);
                    self.due = true;
                }
                In::Record => self.record = true,
                In::Report { at, radius } => self.reports.push((at, radius)),
            }
        }
    }

    /// Water time still owed (s): more runs to do.
    pub fn busy(&self) -> bool {
        self.owed > 1e-9
    }

    /// Runs what is owed (within the budget) and answers what was asked.
    pub fn work(&mut self) -> Vec<Out> {
        let mut out = Vec::new();
        for r in self.restored.drain(..) {
            out.push(Out::Restored(r));
        }
        if self.due || self.busy() {
            self.due = false;
            let start = now();
            if !self.cfg.slow.is_zero() {
                std::thread::sleep(self.cfg.slow);
            }
            let step = self.water.step(self.owed, self.cfg.budget);
            self.owed = (self.owed - step.ran).max(0.0);
            if self.owed > self.cfg.max_owed {
                self.dropped += self.owed - self.cfg.max_owed;
                self.owed = self.cfg.max_owed;
            }
            if self.owed < 1e-9 {
                self.owed = 0.0;
            }
            let cells = self.water.cells();
            out.push(Out::Ran {
                writes: step.writes,
                currents: step.currents,
                wanted: self.water.wanted_chunks(),
                stats: Stats {
                    ran_s: step.ran,
                    owed_s: self.owed,
                    dropped_s: self.dropped,
                    busy_ms: elapsed_ms(start),
                    t: self.water.time(),
                    awake: self.water.awake_chunks() as u32,
                    chunks: self.water.chunks.len() as u32,
                    cell_bytes: cells.bytes() as u64,
                    cell_chunks: cells.len() as u32,
                },
            });
        }
        if std::mem::take(&mut self.record) {
            out.push(Out::Record(self.water.save(|_| 0)));
        }
        for (at, radius) in self.reports.drain(..) {
            out.push(Out::Report(self.water.report(at, radius)));
        }
        out
    }

    /// One encoded batch in, the encoded answer out (the native thread's and the browser
    /// worker's step).
    pub fn handle(&mut self, bytes: &[u8]) -> Option<()> {
        let (seq, items) = decode_in(bytes)?;
        self.receive(seq, items);
        Some(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn now() -> std::time::Instant {
    std::time::Instant::now()
}
#[cfg(not(target_arch = "wasm32"))]
fn elapsed_ms(t: std::time::Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}
#[cfg(target_arch = "wasm32")]
fn now() -> f64 {
    js_now()
}
#[cfg(target_arch = "wasm32")]
fn elapsed_ms(t: f64) -> f64 {
    js_now() - t
}
/// `performance.now()` where there is one (a worker), else 0.
#[cfg(target_arch = "wasm32")]
fn js_now() -> f64 {
    WASM_NOW.with(|f| f.get().map_or(0.0, |f| f()))
}
#[cfg(target_arch = "wasm32")]
thread_local! {
    static WASM_NOW: std::cell::Cell<Option<fn() -> f64>> = const { std::cell::Cell::new(None) };
}
/// The clock the water's runs are timed by in WASM (`performance.now`; dwell-sim has no JS).
#[cfg(target_arch = "wasm32")]
pub fn set_clock(f: fn() -> f64) {
    WASM_NOW.with(|c| c.set(Some(f)));
}

/// Where the water runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Placement {
    /// On the server's thread (tests, a fallback).
    #[default]
    Inline,
    /// Its own native thread (falls back to inline where there are no threads, WASM).
    Thread,
    /// Elsewhere, by messages the embedder carries (the browser's water worker).
    Remote,
}

/// The host's figures (telemetry, docs/DEV_TOOLS.md).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HostStats {
    /// Batches sent and not yet answered.
    pub queue: u32,
    /// Runs answered.
    pub runs: u64,
    /// The last run's figures.
    pub last: Stats,
    /// The longest run's wall time (ms).
    pub max_busy_ms: f64,
}

enum Kind {
    Inline(Box<WaterCore>),
    #[cfg(not(target_arch = "wasm32"))]
    Thread {
        tx: Option<std::sync::mpsc::Sender<Vec<u8>>>,
        rx: std::sync::mpsc::Receiver<Vec<u8>>,
        join: Option<std::thread::JoinHandle<()>>,
    },
    Remote {
        outgoing: VecDeque<Vec<u8>>,
    },
}

/// The water behind its interface (module docs).
pub struct WaterHost {
    kind: Kind,
    seq: u32,
    acked: u32,
    outs: VecDeque<Out>,
    stats: HostStats,
}

impl std::fmt::Debug for WaterHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WaterHost")
            .field("placement", &self.placement())
            .field("seq", &self.seq)
            .field("acked", &self.acked)
            .finish()
    }
}

impl WaterHost {
    /// `water` placed as `placement` (a remote host drops it: its worker makes its own from
    /// `In::Init`), with the placement's own budget and owing.
    pub fn new(placement: Placement, water: Water) -> Self {
        let cfg = match placement {
            Placement::Inline => CoreConfig::inline(&water),
            _ => CoreConfig::thread(),
        };
        Self::with_config(placement, water, cfg)
    }

    pub fn with_config(placement: Placement, water: Water, cfg: CoreConfig) -> Self {
        let kind = match placement {
            Placement::Remote => Kind::Remote {
                outgoing: VecDeque::new(),
            },
            #[cfg(not(target_arch = "wasm32"))]
            Placement::Thread => spawn(WaterCore::new(water, cfg)),
            _ => Kind::Inline(Box::new(WaterCore::new(water, cfg))),
        };
        Self {
            kind,
            seq: 0,
            acked: 0,
            outs: VecDeque::new(),
            stats: HostStats::default(),
        }
    }

    pub fn placement(&self) -> Placement {
        match &self.kind {
            Kind::Inline(_) => Placement::Inline,
            #[cfg(not(target_arch = "wasm32"))]
            Kind::Thread { .. } => Placement::Thread,
            Kind::Remote { .. } => Placement::Remote,
        }
    }

    /// The water itself, where it runs inline (tests, tools).
    pub fn water(&self) -> Option<&Water> {
        match &self.kind {
            Kind::Inline(c) => Some(c.water()),
            _ => None,
        }
    }

    /// Sends one batch (nothing if empty).
    pub fn send(&mut self, items: Vec<In>) {
        if items.is_empty() {
            return;
        }
        self.seq += 1;
        let seq = self.seq;
        match &mut self.kind {
            Kind::Inline(core) => {
                core.receive(seq, items);
                let outs = core.work();
                self.take(seq, outs);
            }
            #[cfg(not(target_arch = "wasm32"))]
            Kind::Thread { tx, .. } => {
                let bytes = encode_in(seq, &items);
                if tx.as_ref().is_some_and(|tx| tx.send(bytes).is_err()) {
                    eprintln!("dwell: the water thread stopped; the water stands still");
                    *tx = None;
                }
            }
            Kind::Remote { outgoing } => outgoing.push_back(encode_in(seq, &items)),
        }
    }

    fn take(&mut self, seq: u32, outs: Vec<Out>) {
        self.acked = self.acked.max(seq);
        for o in &outs {
            if let Out::Ran { stats, .. } = o {
                self.stats.runs += 1;
                self.stats.last = *stats;
                self.stats.max_busy_ms = self.stats.max_busy_ms.max(stats.busy_ms);
            }
        }
        self.outs.extend(outs);
    }

    /// An answer's bytes (a remote host's worker). False if they can't be read.
    pub fn deliver(&mut self, bytes: &[u8]) -> bool {
        match decode_out(bytes) {
            Some((seq, outs)) => {
                self.take(seq, outs);
                true
            }
            None => false,
        }
    }

    /// The next batch for a remote host's worker.
    pub fn take_outgoing(&mut self) -> Option<Vec<u8>> {
        match &mut self.kind {
            Kind::Remote { outgoing } => outgoing.pop_front(),
            _ => None,
        }
    }

    fn pump(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Kind::Thread { rx, .. } = &self.kind {
            let got: Vec<Vec<u8>> = rx.try_iter().collect();
            for b in got {
                self.deliver(&b);
            }
        }
    }

    /// The writes come back and not yet polled (tests).
    pub fn pending_writes(&mut self) -> Vec<Write> {
        self.pump();
        self.outs
            .iter()
            .flat_map(|o| match o {
                Out::Ran { writes, .. } => writes.clone(),
                _ => Vec::new(),
            })
            .collect()
    }

    /// What came back since the last poll.
    pub fn poll(&mut self) -> Vec<Out> {
        self.pump();
        self.outs.drain(..).collect()
    }

    /// Waits (at most `timeout`) until every batch sent has been answered; true if it was. A
    /// remote host can't wait (the browser's thread would block its worker's answers).
    pub fn wait(&mut self, timeout: Duration) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let end = std::time::Instant::now() + timeout;
            while self.acked < self.seq {
                let Kind::Thread { rx, .. } = &self.kind else {
                    break;
                };
                let left = end.saturating_duration_since(std::time::Instant::now());
                match rx.recv_timeout(left) {
                    Ok(b) => {
                        self.deliver(&b);
                    }
                    Err(_) => break,
                }
            }
        }
        let _ = timeout;
        self.pump();
        self.acked >= self.seq
    }

    pub fn stats(&self) -> HostStats {
        HostStats {
            queue: self.seq - self.acked,
            ..self.stats
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn(mut core: WaterCore) -> Kind {
    use std::sync::mpsc::{TryRecvError, channel};
    let (tx, rx_in) = channel::<Vec<u8>>();
    let (tx_out, rx) = channel::<Vec<u8>>();
    let join = std::thread::Builder::new()
        .name("dwell-water".into())
        .spawn(move || {
            loop {
                // idle: wait for a batch; owing water time: take what came and run on
                let first = if core.busy() {
                    match rx_in.try_recv() {
                        Ok(b) => Some(b),
                        Err(TryRecvError::Empty) => None,
                        Err(TryRecvError::Disconnected) => return,
                    }
                } else {
                    match rx_in.recv() {
                        Ok(b) => Some(b),
                        Err(_) => return,
                    }
                };
                for b in first.into_iter().chain(rx_in.try_iter()) {
                    if core.handle(&b).is_none() {
                        eprintln!("dwell: the water thread got a batch it can't read");
                    }
                }
                let outs = core.work();
                if tx_out.send(encode_out(core.seq(), &outs)).is_err() {
                    return;
                }
            }
        })
        .expect("the water thread starts");
    Kind::Thread {
        tx: Some(tx),
        rx,
        join: Some(join),
    }
}

impl Drop for WaterHost {
    fn drop(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Kind::Thread { tx, join, .. } = &mut self.kind {
            tx.take();
            if let Some(j) = join.take() {
                let _ = j.join();
            }
        }
    }
}

// --- The messages' bytes: a batch is a tag ("WI1" in, "WO1" out), its sequence number (u32), its
// item count (u32), then each item as a tag byte and its fields, little-endian.

const IN_TAG: &[u8; 3] = b"WI1";
const OUT_TAG: &[u8; 3] = b"WO1";

struct W(Vec<u8>);
impl W {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn i32(&mut self, v: i32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f64(&mut self, v: f64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn cell(&mut self, c: [i32; 3]) {
        c.iter().for_each(|&v| self.i32(v));
    }
    fn bytes(&mut self, b: &[u8]) {
        self.u32(b.len() as u32);
        self.0.extend_from_slice(b);
    }
}

struct R<'a> {
    b: &'a [u8],
    at: usize,
}
impl R<'_> {
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let s = self.b.get(self.at..self.at + N)?;
        self.at += N;
        s.try_into().ok()
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take::<1>()?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take()?))
    }
    fn i32(&mut self) -> Option<i32> {
        Some(i32::from_le_bytes(self.take()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take()?))
    }
    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.take()?))
    }
    fn f64(&mut self) -> Option<f64> {
        Some(f64::from_le_bytes(self.take()?))
    }
    fn cell(&mut self) -> Option<[i32; 3]> {
        Some([self.i32()?, self.i32()?, self.i32()?])
    }
    fn coord(&mut self) -> Option<ChunkCoord> {
        self.cell().map(ChunkCoord::from_array)
    }
    fn bytes(&mut self) -> Option<Vec<u8>> {
        let n = self.u32()? as usize;
        let s = self.b.get(self.at..self.at + n)?;
        self.at += n;
        Some(s.to_vec())
    }
    /// A count, no more than the bytes left could hold at `min` bytes each.
    fn count(&mut self, min: usize) -> Option<usize> {
        let n = self.u32()? as usize;
        (n.saturating_mul(min) <= self.b.len() - self.at).then_some(n)
    }
}

/// Batch `seq` of `items`, as bytes.
pub fn encode_in(seq: u32, items: &[In]) -> Vec<u8> {
    let mut w = W(IN_TAG.to_vec());
    w.u32(seq);
    w.u32(items.len() as u32);
    for item in items {
        match item {
            In::Init {
                generator_version,
                world_seed,
                map,
            } => {
                w.u8(0);
                w.u32(*generator_version);
                w.u64(*world_seed);
                w.u8(u8::from(*map));
            }
            In::Restore(b) => {
                w.u8(1);
                w.bytes(b);
            }
            In::Load { at, cells, fresh } => {
                w.u8(2);
                w.cell(at.to_array());
                w.u8(u8::from(*fresh));
                cells.encode(&mut w.0);
            }
            In::Unload(at) => {
                w.u8(3);
                w.cell(at.to_array());
            }
            In::Edit { cell, byte } => {
                w.u8(4);
                w.cell(*cell);
                w.u8(*byte);
            }
            In::Players(p) => {
                w.u8(5);
                w.u32(p.len() as u32);
                for v in p {
                    v.iter().for_each(|&x| w.f64(x));
                }
            }
            In::Run(dt) => {
                w.u8(6);
                w.f64(*dt);
            }
            In::Record => w.u8(7),
            In::Report { at, radius } => {
                w.u8(8);
                w.cell(*at);
                w.i32(*radius);
            }
        }
    }
    w.0
}

/// A batch's sequence number and items (None if malformed).
pub fn decode_in(b: &[u8]) -> Option<(u32, Vec<In>)> {
    let mut r = R { b, at: 0 };
    if &r.take::<3>()? != IN_TAG {
        return None;
    }
    let seq = r.u32()?;
    let n = r.count(1)?;
    let mut items = Vec::with_capacity(n);
    for _ in 0..n {
        items.push(match r.u8()? {
            0 => In::Init {
                generator_version: r.u32()?,
                world_seed: r.u64()?,
                map: r.u8()? != 0,
            },
            1 => In::Restore(r.bytes()?),
            2 => {
                let at = r.coord()?;
                let fresh = r.u8()? != 0;
                let mut i = r.at;
                let cells = ChunkCells::decode(r.b, &mut i)?;
                r.at = i;
                In::Load { at, cells, fresh }
            }
            3 => In::Unload(r.coord()?),
            4 => In::Edit {
                cell: r.cell()?,
                byte: r.u8()?,
            },
            5 => {
                let n = r.count(24)?;
                let mut p = Vec::with_capacity(n);
                for _ in 0..n {
                    p.push([r.f64()?, r.f64()?, r.f64()?]);
                }
                In::Players(p)
            }
            6 => In::Run(r.f64()?),
            7 => In::Record,
            8 => In::Report {
                at: r.cell()?,
                radius: r.i32()?,
            },
            _ => return None,
        });
    }
    (r.at == b.len()).then_some((seq, items))
}

/// The answer to batch `seq` (the last received), as bytes.
pub fn encode_out(seq: u32, outs: &[Out]) -> Vec<u8> {
    let mut w = W(OUT_TAG.to_vec());
    w.u32(seq);
    w.u32(outs.len() as u32);
    for o in outs {
        match o {
            Out::Ran {
                writes,
                currents,
                wanted,
                stats,
            } => {
                w.u8(0);
                w.u32(writes.len() as u32);
                for x in writes {
                    w.cell(x.cell);
                    w.u16(x.state);
                    w.u8(x.was);
                }
                w.u32(currents.len() as u32);
                for ((b, cx, cz), c) in currents {
                    w.u8(u8::from(*b));
                    w.i32(*cx);
                    w.i32(*cz);
                    match c {
                        Some(c) => {
                            w.u8(1);
                            for [u, v] in c.iter() {
                                w.f32(*u);
                                w.f32(*v);
                            }
                        }
                        None => w.u8(0),
                    }
                }
                w.u32(wanted.len() as u32);
                for c in wanted {
                    w.cell(c.to_array());
                }
                for v in [
                    stats.ran_s,
                    stats.owed_s,
                    stats.dropped_s,
                    stats.busy_ms,
                    stats.t,
                ] {
                    w.f64(v);
                }
                w.u32(stats.awake);
                w.u32(stats.chunks);
                w.u64(stats.cell_bytes);
                w.u32(stats.cell_chunks);
            }
            Out::Record(b) => {
                w.u8(1);
                w.bytes(b);
            }
            Out::Report(s) => {
                w.u8(2);
                w.bytes(s.as_bytes());
            }
            Out::Restored(r) => {
                w.u8(3);
                w.u64(r.map_or(u64::MAX, |n| n as u64));
            }
        }
    }
    w.0
}

/// An answer's sequence number and items (None if malformed).
pub fn decode_out(b: &[u8]) -> Option<(u32, Vec<Out>)> {
    let mut r = R { b, at: 0 };
    if &r.take::<3>()? != OUT_TAG {
        return None;
    }
    let seq = r.u32()?;
    let n = r.count(1)?;
    let mut outs = Vec::with_capacity(n);
    for _ in 0..n {
        outs.push(match r.u8()? {
            0 => {
                let n = r.count(15)?;
                let mut writes = Vec::with_capacity(n);
                for _ in 0..n {
                    writes.push(Write {
                        cell: r.cell()?,
                        state: r.u16()?,
                        was: r.u8()?,
                    });
                }
                let n = r.count(10)?;
                let mut currents = Vec::with_capacity(n);
                for _ in 0..n {
                    let key = (r.u8()? != 0, r.i32()?, r.i32()?);
                    let c = if r.u8()? == 1 {
                        let mut c: ChunkCurrents = Box::new([[0.0; 2]; CA]);
                        for x in c.iter_mut() {
                            *x = [r.f32()?, r.f32()?];
                        }
                        Some(c)
                    } else {
                        None
                    };
                    currents.push((key, c));
                }
                let n = r.count(12)?;
                let mut wanted = Vec::with_capacity(n);
                for _ in 0..n {
                    wanted.push(r.coord()?);
                }
                let stats = Stats {
                    ran_s: r.f64()?,
                    owed_s: r.f64()?,
                    dropped_s: r.f64()?,
                    busy_ms: r.f64()?,
                    t: r.f64()?,
                    awake: r.u32()?,
                    chunks: r.u32()?,
                    cell_bytes: r.u64()?,
                    cell_chunks: r.u32()?,
                };
                Out::Ran {
                    writes,
                    currents,
                    wanted,
                    stats,
                }
            }
            1 => Out::Record(r.bytes()?),
            2 => Out::Report(String::from_utf8(r.bytes()?).ok()?),
            3 => {
                let v = r.u64()?;
                Out::Restored((v != u64::MAX).then_some(v as usize))
            }
            _ => return None,
        });
    }
    (r.at == b.len()).then_some((seq, outs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::water::cells::{FULL, OPEN};

    #[test]
    fn batches_round_trip() {
        // face-a-only: an encoding; cells are world cells, the same on both faces.
        let mut mixed = dwell_world::Chunk::default();
        mixed.set_at(5, dwell_world::M.water);
        let items = vec![
            In::Init {
                generator_version: 8,
                world_seed: u64::MAX - 3,
                map: true,
            },
            In::Restore(vec![1, 2, 3]),
            In::Load {
                at: ChunkCoord::new(-1, 2, 3),
                cells: ChunkCells::Uniform(OPEN),
                fresh: true,
            },
            In::Load {
                at: ChunkCoord::new(4, -5, 6),
                cells: ChunkCells::of(&mixed),
                fresh: false,
            },
            In::Unload(ChunkCoord::new(7, 8, -9)),
            In::Edit {
                cell: [1, -2, 3],
                byte: FULL,
            },
            In::Players(vec![[1.5, 2.5, -3.5]]),
            In::Run(7.2),
            In::Record,
            In::Report {
                at: [4, 5, 6],
                radius: 48,
            },
        ];
        let b = encode_in(42, &items);
        assert_eq!(decode_in(&b), Some((42, items)));
        let mut c: ChunkCurrents = Box::new([[0.0; 2]; CA]);
        c[3] = [0.25, -1.5];
        let outs = vec![
            Out::Ran {
                writes: vec![Write {
                    cell: [1, 2, 3],
                    state: dwell_world::M.water,
                    was: OPEN,
                }],
                currents: vec![((true, -1, 2), Some(c)), ((false, 3, 4), None)],
                wanted: vec![ChunkCoord::new(1, 1, 1)],
                stats: Stats {
                    ran_s: 7.2,
                    owed_s: 1.0,
                    dropped_s: 0.5,
                    busy_ms: 3.25,
                    t: 100.0,
                    awake: 3,
                    chunks: 9,
                    cell_bytes: 1 << 33,
                    cell_chunks: 12,
                },
            },
            Out::Record(vec![9, 8]),
            Out::Report("{\"x\":1}".into()),
            Out::Restored(Some(4)),
            Out::Restored(None),
        ];
        let b = encode_out(7, &outs);
        assert_eq!(decode_out(&b), Some((7, outs)));
    }

    #[test]
    fn a_malformed_batch_is_refused() {
        // face-a-only: an encoding.
        let b = encode_in(1, &[In::Run(1.0)]);
        assert_eq!(decode_in(&b[..b.len() - 1]), None, "cut short");
        let mut long = b.clone();
        long.push(0);
        assert_eq!(decode_in(&long), None, "trailing bytes");
        assert_eq!(decode_out(&b), None, "not an answer");
        let mut huge = encode_out(1, &[]);
        huge[7..11].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(decode_out(&huge), None, "a count past its bytes");
    }

    #[test]
    fn an_edit_is_told_compactly() {
        // face-a-only: an encoding.
        let b = encode_in(
            1,
            &[In::Edit {
                cell: [0, 0, 0],
                byte: OPEN,
            }],
        );
        assert_eq!(b.len(), 3 + 8 + 14, "an edit is 14 bytes");
    }
}
