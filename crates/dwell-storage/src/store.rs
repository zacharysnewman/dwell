//! The server's handle on its world file (§6.4): loads on the tick thread, saves committed on an
//! I/O thread (natively) or on the caller's thread (the browser, no threads). redb's readers never
//! wait for the writer, so one database serves both.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

use crate::{SaveBatch, WorldDb};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveResult {
    pub id: u64,
    pub ok: bool,
    pub error: String,
}

#[derive(Default)]
struct Shared {
    queue: VecDeque<(u64, SaveBatch)>,
    completed: Vec<SaveResult>,
    busy: bool,
    stop: bool,
}

pub struct WorldStore {
    db: Arc<WorldDb>,
    shared: Arc<(Mutex<Shared>, Condvar)>,
    thread: Option<JoinHandle<()>>,
    next_id: u64,
}

impl WorldStore {
    /// `threaded`: commit on an I/O thread; otherwise saves commit inline.
    pub fn new(db: Arc<WorldDb>, threaded: bool) -> Self {
        let shared: Arc<(Mutex<Shared>, Condvar)> = Arc::default();
        let thread = threaded.then(|| {
            let (db, shared) = (db.clone(), shared.clone());
            std::thread::Builder::new()
                .name("dwell-save".into())
                .spawn(move || run(&db, &shared))
                .expect("save thread")
        });
        Self {
            db,
            shared,
            thread,
            next_id: 1,
        }
    }

    /// Opens `path` natively with the I/O thread.
    pub fn open_file(path: &std::path::Path) -> Result<Self, String> {
        Ok(Self::new(WorldDb::open(path)?, true))
    }

    pub fn db(&self) -> &Arc<WorldDb> {
        &self.db
    }
    pub fn threaded(&self) -> bool {
        self.thread.is_some()
    }

    /// Queues (threaded) or commits the batch; returns its id.
    pub fn save(&mut self, batch: SaveBatch) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let (lock, cv) = &*self.shared;
        if self.thread.is_some() {
            lock.lock().expect("store").queue.push_back((id, batch));
            cv.notify_all();
        } else {
            let r = self.db.save(&batch);
            lock.lock().expect("store").completed.push(SaveResult {
                id,
                ok: r.is_ok(),
                error: r.err().unwrap_or_default(),
            });
        }
        id
    }

    /// Saves finished since the last call, in order.
    pub fn take_completed(&self) -> Vec<SaveResult> {
        std::mem::take(&mut self.shared.0.lock().expect("store").completed)
    }

    /// Blocks until every queued save has been committed.
    pub fn flush(&self) {
        let (lock, cv) = &*self.shared;
        let mut s = lock.lock().expect("store");
        while !s.queue.is_empty() || s.busy {
            s = cv.wait(s).expect("store");
        }
    }
}

fn run(db: &WorldDb, shared: &(Mutex<Shared>, Condvar)) {
    let (lock, cv) = shared;
    loop {
        let (id, batch) = {
            let mut s = lock.lock().expect("store");
            while s.queue.is_empty() && !s.stop {
                s = cv.wait(s).expect("store");
            }
            let Some(job) = s.queue.pop_front() else {
                return;
            }; // stopping with nothing queued
            s.busy = true;
            job
        };
        let r = db.save(&batch);
        let mut s = lock.lock().expect("store");
        s.completed.push(SaveResult {
            id,
            ok: r.is_ok(),
            error: r.err().unwrap_or_default(),
        });
        s.busy = false;
        cv.notify_all();
    }
}

impl Drop for WorldStore {
    /// Commits what is queued, then stops the thread.
    fn drop(&mut self) {
        if let Some(t) = self.thread.take() {
            self.shared.0.lock().expect("store").stop = true;
            self.shared.1.notify_all();
            let _ = t.join();
        }
    }
}
