//! Where world maps live in a process (ADR 0049): one per face seed, shared (`Arc`) by every
//! generator of that face. A map is computed on first use, or installed from bytes first (the
//! browser builds it in a worker and hands each WASM instance the bytes, so no instance computes it
//! on its own thread). A native process also keeps a copy on disk (`set_cache_dir`, or
//! `$DWELL_MAP_CACHE`, or the temp directory), so a server restarting, or each test process of a
//! suite, reads it instead of computing it again. Two threads asking for the same seed wait for
//! one computation.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use dwell_protocol::world_size::WorldSize;

use super::{FINGERPRINT, WorldMap, codec};

type Slot = Arc<OnceLock<Arc<WorldMap>>>;

/// A map's place: its face seed and its world's size (BIG_MAP.md M2; a seed's maps at two sizes
/// are two maps).
type Key = (u64, WorldSize);

/// The maps of this process, most recently used last. A world uses two (its faces); a few more
/// are kept for tools and tests that visit several seeds.
static MAPS: Mutex<Vec<(Key, Slot)>> = Mutex::new(Vec::new());
const KEEP: usize = 8;

static DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Where a native process keeps maps on disk (a server's world directory).
pub fn set_cache_dir(dir: PathBuf) {
    *DIR.lock().expect("map dir") = Some(dir);
}

fn slot(seed: Key) -> Slot {
    let mut maps = MAPS.lock().expect("world maps");
    if let Some(at) = maps.iter().position(|(s, _)| *s == seed) {
        let entry = maps.remove(at);
        let slot = entry.1.clone();
        maps.push(entry);
        return slot;
    }
    if maps.len() >= KEEP {
        maps.remove(0);
    }
    let slot: Slot = Arc::default();
    maps.push((seed, slot.clone()));
    slot
}

/// The map of face seed `seed`: the installed or cached one, else computed now (and kept).
pub fn for_seed(seed: u64) -> Arc<WorldMap> {
    for_seed_sized(seed, WorldSize::DEFAULT)
}

/// The map of face seed `seed` in a world of `size` (BIG_MAP.md M2).
pub fn for_seed_sized(seed: u64, size: WorldSize) -> Arc<WorldMap> {
    slot((seed, size))
        .get_or_init(|| {
            if let Some(m) = disk::read(seed, size) {
                return Arc::new(m);
            }
            let m = WorldMap::compute_sized(seed, size);
            disk::write(&m);
            Arc::new(m)
        })
        .clone()
}

/// Whether a map for `seed` is ready in this process (installed or computed).
pub fn is_ready(seed: u64) -> bool {
    is_ready_sized(seed, WorldSize::DEFAULT)
}

/// [`is_ready`] in a world of `size`.
pub fn is_ready_sized(seed: u64, size: WorldSize) -> bool {
    MAPS.lock()
        .expect("world maps")
        .iter()
        .any(|(s, slot)| *s == (seed, size) && slot.get().is_some())
}

/// Computes the maps of several seeds at once (natively on threads; one after another in WASM).
pub fn prefetch(seeds: &[u64]) {
    prefetch_sized(seeds, WorldSize::DEFAULT)
}

/// [`prefetch`] in a world of `size`.
pub fn prefetch_sized(seeds: &[u64], size: WorldSize) {
    let missing: Vec<u64> = seeds
        .iter()
        .copied()
        .filter(|&s| !is_ready_sized(s, size))
        .collect();
    if missing.len() < 2 {
        for seed in missing {
            for_seed_sized(seed, size);
        }
        return;
    }
    #[cfg(not(target_family = "wasm"))]
    std::thread::scope(|s| {
        for seed in missing {
            s.spawn(move || {
                for_seed_sized(seed, size);
            });
        }
    });
    #[cfg(target_family = "wasm")]
    for seed in missing {
        for_seed_sized(seed, size);
    }
}

/// Installs a map from its bytes (`build_bytes`); returns its seed. Refused when damaged or made
/// by another build.
pub fn install(bytes: &[u8]) -> Result<u64, String> {
    let seed = codec::verify(bytes)?;
    let m = codec::decode(bytes)?;
    let size = m.size;
    let s = slot((seed, size));
    if s.get().is_some() {
        // The same map kept; another one for the seed (a test world's, ADR 0074 B7, after the
        // seed's own was computed or installed here, or the other way round) replaces it, so a
        // generator built after the install never uses the wrong ground.
        if s.get()
            .is_some_and(|k| k.worldgen_text() == m.worldgen_text())
        {
            return Ok(seed);
        }
        replace((seed, size), m);
        return Ok(seed);
    }
    let _ = s.set(Arc::new(m));
    Ok(seed)
}

/// Puts `m` in seed `seed`'s place (a fresh slot; generators holding the old map keep it).
fn replace(seed: Key, m: WorldMap) {
    let mut maps = MAPS.lock().expect("world maps");
    maps.retain(|(s, _)| *s != seed);
    if maps.len() >= KEEP {
        maps.remove(0);
    }
    let slot: Slot = Arc::default();
    let _ = slot.set(Arc::new(m));
    maps.push((seed, slot));
}

/// Computes face seed `seed`'s map and returns its bytes (the browser's builder worker).
pub fn build_bytes(seed: u64) -> Vec<u8> {
    build_bytes_sized(seed, WorldSize::DEFAULT)
}

/// [`build_bytes`] in a world of `size`.
pub fn build_bytes_sized(seed: u64, size: WorldSize) -> Vec<u8> {
    codec::encode(&for_seed_sized(seed, size))
}

/// The cache key of a seed's map in this build (`<fingerprint>-<settings hash>-<seed>`, hex).
pub fn key(seed: u64) -> String {
    key_sized(seed, WorldSize::DEFAULT)
}

/// [`key`] in a world of `size`: today's key at 1×, `-x<scale>` after it for a bigger world.
pub fn key_sized(seed: u64, size: WorldSize) -> String {
    let k = format!(
        "{FINGERPRINT:016x}-{:016x}-{seed:016x}",
        super::SETTINGS_HASH
    );
    if size.is_default() {
        k
    } else {
        format!("{k}-x{}", size.radius_scale)
    }
}

#[cfg(not(target_family = "wasm"))]
mod disk {
    use super::*;

    fn dir() -> PathBuf {
        if let Some(d) = DIR.lock().expect("map dir").clone() {
            return d;
        }
        std::env::var_os("DWELL_MAP_CACHE")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("dwell-world-maps"))
    }

    fn path(seed: u64, size: WorldSize) -> PathBuf {
        dir().join(format!("{}.dwmap", key_sized(seed, size)))
    }

    pub fn read(seed: u64, size: WorldSize) -> Option<WorldMap> {
        let bytes = std::fs::read(path(seed, size)).ok()?;
        codec::decode(&bytes)
            .ok()
            .filter(|m| m.seed == seed && m.size == size)
    }

    pub fn write(m: &WorldMap) {
        let d = dir();
        if std::fs::create_dir_all(&d).is_err() {
            return;
        }
        let target = path(m.seed, m.size);
        let tmp = d.join(format!(
            "{}.{}.tmp",
            key_sized(m.seed, m.size),
            std::process::id() as u64 ^ (std::thread::current().id().as_u64_hash())
        ));
        if std::fs::write(&tmp, codec::encode(m)).is_ok() && std::fs::rename(&tmp, &target).is_err()
        {
            let _ = std::fs::remove_file(&tmp);
        }
    }

    trait ThreadHash {
        fn as_u64_hash(&self) -> u64;
    }
    impl ThreadHash for std::thread::ThreadId {
        fn as_u64_hash(&self) -> u64 {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            self.hash(&mut h);
            h.finish()
        }
    }
}

#[cfg(target_family = "wasm")]
mod disk {
    use super::{WorldMap, WorldSize};
    pub fn read(_seed: u64, _size: WorldSize) -> Option<WorldMap> {
        None
    }
    pub fn write(_m: &WorldMap) {}
}
