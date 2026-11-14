//! The local world's server on its own thread: the browser's local-mode worker
//! (`client/src/local/worker.ts`) one for one. Every 4 ms it takes the client's messages, advances
//! the core by the real time since the last pass and sends its outbox back; once a second it sends
//! the figures the worker posts (`stats`: ticks, mean tick time, worst advance, the tick's phases
//! and counts), which the perf line and the scenario reports read.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use dwell_client::local::{LOCAL_SESSION, LocalCore};
use dwell_protocol::{Channel, TransportKind};
use dwell_sim::server::OutKind;

pub enum Up {
    Reliable(Vec<u8>),
    Datagram(Vec<u8>),
}

pub enum Down {
    Reliable(Channel, Vec<u8>),
    Datagram(Vec<u8>),
    Stats(ServerStats),
    /// The world could not start (e.g. a file locked to another version).
    Failed(String),
}

/// One second of the server (the worker's `stats` message).
#[derive(Clone, Debug, Default)]
pub struct ServerStats {
    pub ticks: u32,
    pub tick_mean_ms: f64,
    pub advance_max_ms: f64,
    pub busy_ms: f64,
    pub phases_ms: Vec<(&'static str, f64)>,
    pub counts: Vec<(&'static str, u64)>,
}

impl ServerStats {
    pub fn json(&self) -> serde_json::Value {
        let phases: serde_json::Map<_, _> = self
            .phases_ms
            .iter()
            .map(|(n, v)| ((*n).to_string(), (*v).into()))
            .collect();
        let counts: serde_json::Map<_, _> = self
            .counts
            .iter()
            .map(|(n, v)| ((*n).to_string(), (*v).into()))
            .collect();
        serde_json::json!({
            "ticks": self.ticks,
            "tickMeanMs": self.tick_mean_ms,
            "advanceMaxMs": self.advance_max_ms,
            "busyMs": self.busy_ms,
            "phasesMs": phases,
            "counts": counts,
        })
    }
}

pub struct Options {
    pub seed: u64,
    pub generator: u32,
    pub at: Option<[f64; 3]>,
    pub world_file: Option<PathBuf>,
    /// Threads for the server's generation and LOD (the browser's one server-jobs worker: 1).
    pub threads: usize,
}

pub struct ServerThread {
    pub up: Sender<Up>,
    pub down: Receiver<Down>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ServerThread {
    pub fn start(o: Options) -> Self {
        let (up, up_rx) = channel::<Up>();
        let (down_tx, down) = channel::<Down>();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let thread = std::thread::Builder::new()
            .name("local-server".into())
            .spawn(move || run(o, up_rx, down_tx, stopping))
            .expect("the server thread");
        Self {
            up,
            down,
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for ServerThread {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run(o: Options, up: Receiver<Up>, down: Sender<Down>, stop: Arc<AtomicBool>) {
    let storage = o.world_file.as_ref().and_then(|path| {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|e| eprintln!("dwell-native: the world file {path:?}: {e}"))
            .ok()?;
        redb::backends::FileBackend::new(file)
            .map_err(|e| eprintln!("dwell-native: the world file {path:?}: {e}"))
            .ok()
    });
    let threads = o.threads;
    let core = LocalCore::create_tuned(
        dwell_world::Face::A,
        false,
        o.at,
        false,
        false,
        o.seed,
        o.generator,
        storage,
        Box::new(dwell_sim::entropy::SystemEntropy),
        // The browser's water worker is a thread of its own: here one too.
        dwell_sim::water::host::Placement::Thread,
        false,
        |c| {
            // The browser's server-jobs worker does generation and the LOD's sections off the
            // tick; natively the server's own pools do (`threads` each).
            c.worldgen_threads = threads;
            c.lod_threads = Some(threads);
        },
    );
    let mut core = match core {
        Ok(c) => c,
        Err(e) => {
            let _ = down.send(Down::Failed(format!("{e:?}")));
            return;
        }
    };
    if !core.storage_error.is_empty() {
        eprintln!(
            "dwell-native: the world is not saved ({})",
            core.storage_error
        );
    }
    core.connected(LOCAL_SESSION, TransportKind::Loopback, [0; 32]);
    let mut last = Instant::now();
    let mut second = Instant::now();
    let mut stats = ServerStats::default();
    let mut tick: Option<u32> = None;
    let mut ms_total = 0.0;
    while !stop.load(Ordering::Relaxed) {
        while let Ok(m) = up.try_recv() {
            match m {
                Up::Reliable(b) => core.reliable(LOCAL_SESSION, Channel::Control, &b),
                Up::Datagram(b) => core.datagram(LOCAL_SESSION, &b),
            }
        }
        let now = Instant::now();
        let t = core.advance((now - last).as_secs_f64());
        let ms = now.elapsed().as_secs_f64() * 1000.0;
        last = now;
        stats.busy_ms += ms;
        if let Some(prev) = tick
            && t != prev
        {
            stats.ticks += t - prev;
            ms_total += ms;
            stats.advance_max_ms = stats.advance_max_ms.max(ms);
        }
        tick = Some(t);
        for o in core.take_outbox() {
            let m = match o.kind {
                OutKind::Reliable(c) => Down::Reliable(c, o.bytes),
                OutKind::Datagram => Down::Datagram(o.bytes),
                OutKind::Close => continue,
            };
            if down.send(m).is_err() {
                return;
            }
        }
        if second.elapsed() >= Duration::from_secs(1) {
            second = Instant::now();
            stats.tick_mean_ms = if stats.ticks > 0 {
                ms_total / f64::from(stats.ticks)
            } else {
                0.0
            };
            stats.phases_ms = core.server().tick_phases().to_vec();
            stats.counts = core.server().tick_counts().to_vec();
            let _ = down.send(Down::Stats(std::mem::take(&mut stats)));
            ms_total = 0.0;
        }
        // The worker's `setInterval(…, 4)`.
        std::thread::sleep(Duration::from_millis(4).saturating_sub(now.elapsed()));
    }
    if core.persisted {
        core.save();
    }
}
