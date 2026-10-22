//! Where world maps live in a process (ADR 0049): one per face seed and map settings (their hash,
//! `WorldSettings::map_hash`), shared (`Arc`) by every generator of that face. A map is computed on first use, or installed from bytes first (the
//! browser builds it in a worker and hands each WASM instance the bytes, so no instance computes it
//! on its own thread). A native process also keeps a copy on disk (`set_cache_dir`, or
//! `$DWELL_MAP_CACHE`, or the temp directory), so a server restarting, or each test process of a
//! suite, reads it instead of computing it again. Two threads asking for the same seed wait for
//! one computation.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use super::{FINGERPRINT, WorldMap, codec};
use crate::settings::MapSettings;

type Slot = Arc<OnceLock<Arc<WorldMap>>>;
/// (face seed, settings hash).
type Key = (u64, u64);

/// The maps of this process, most recently used last. A world uses two (its faces); a few more
/// are kept for tools and tests that visit several seeds.
static MAPS: Mutex<Vec<(Key, Slot)>> = Mutex::new(Vec::new());
const KEEP: usize = 8;

static DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Where a native process keeps maps on disk (a server's world directory).
pub fn set_cache_dir(dir: PathBuf) {
    *DIR.lock().expect("map dir") = Some(dir);
}

fn slot(key: Key) -> Slot {
    let mut maps = MAPS.lock().expect("world maps");
    if let Some(at) = maps.iter().position(|(k, _)| *k == key) {
        let entry = maps.remove(at);
        let slot = entry.1.clone();
        maps.push(entry);
        return slot;
    }
    if maps.len() >= KEEP {
        maps.remove(0);
    }
    let slot: Slot = Arc::default();
    maps.push((key, slot.clone()));
    slot
}

/// The map of face seed `seed` under the data files' settings.
pub fn for_seed(seed: u64) -> Arc<WorldMap> {
    for_settings(seed, &MapSettings::default())
}

/// The map of face seed `seed` under map settings `s`: the installed or cached one, else computed
/// now (and kept).
pub fn for_settings(seed: u64, s: &MapSettings) -> Arc<WorldMap> {
    slot((seed, s.hash))
        .get_or_init(|| {
            if let Some(m) = disk::read(seed, s.hash) {
                return Arc::new(m);
            }
            let m = WorldMap::compute_with(seed, s);
            disk::write(&m);
            Arc::new(m)
        })
        .clone()
}

/// Whether a map for `seed` under settings of hash `settings` is ready in this process (installed
/// or computed).
pub fn is_ready(seed: u64, settings: u64) -> bool {
    MAPS.lock()
        .expect("world maps")
        .iter()
        .any(|(k, slot)| *k == (seed, settings) && slot.get().is_some())
}

/// Computes the maps of several seeds at once (natively on threads; one after another in WASM).
pub fn prefetch(seeds: &[u64], s: &MapSettings) {
    let missing: Vec<u64> = seeds
        .iter()
        .copied()
        .filter(|&seed| !is_ready(seed, s.hash))
        .collect();
    if missing.len() < 2 {
        for seed in missing {
            for_settings(seed, s);
        }
        return;
    }
    #[cfg(not(target_family = "wasm"))]
    std::thread::scope(|scope| {
        for seed in missing {
            scope.spawn(move || {
                for_settings(seed, s);
            });
        }
    });
    #[cfg(target_family = "wasm")]
    for seed in missing {
        for_settings(seed, s);
    }
}

/// Installs a map from its bytes (`build_bytes`); returns its seed. Refused when damaged or made
/// by another build.
pub fn install(bytes: &[u8]) -> Result<u64, String> {
    let (seed, settings) = codec::verify(bytes)?;
    let s = slot((seed, settings));
    if s.get().is_some() {
        return Ok(seed);
    }
    let m = codec::decode(bytes)?;
    let _ = s.set(Arc::new(m));
    Ok(seed)
}

/// Computes face seed `seed`'s map under settings `s` and returns its bytes (the browser's builder
/// worker).
pub fn build_bytes(seed: u64, s: &MapSettings) -> Vec<u8> {
    codec::encode(&for_settings(seed, s))
}

/// The cache key of a seed's map under settings of hash `settings` in this build
/// (`<fingerprint>-<settings hash>-<seed>`, hex).
pub fn key(seed: u64, settings: u64) -> String {
    format!("{FINGERPRINT:016x}-{settings:016x}-{seed:016x}")
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

    fn path(seed: u64, settings: u64) -> PathBuf {
        dir().join(format!("{}.dwmap", key(seed, settings)))
    }

    pub fn read(seed: u64, settings: u64) -> Option<WorldMap> {
        let bytes = std::fs::read(path(seed, settings)).ok()?;
        codec::decode(&bytes)
            .ok()
            .filter(|m| m.seed == seed && m.settings == settings)
    }

    pub fn write(m: &WorldMap) {
        let d = dir();
        if std::fs::create_dir_all(&d).is_err() {
            return;
        }
        let target = path(m.seed, m.settings);
        let tmp = d.join(format!(
            "{}.{}.tmp",
            key(m.seed, m.settings),
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
    use super::WorldMap;
    pub fn read(_seed: u64, _settings: u64) -> Option<WorldMap> {
        None
    }
    pub fn write(_m: &WorldMap) {}
}
