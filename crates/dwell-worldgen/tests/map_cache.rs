//! The world maps' disk cache (`worldmap::cache`): a map with a world config (the Realistic
//! world's, the tests') is computed once and read back byte-identical, and a cache directory keeps
//! only this build's maps (owner, 2026-11-23: "Just ensuring the stored data gets cleared when it's
//! outdated and never going to be used again"). Its own test binary: it points the process's cache
//! at a scratch directory.

use std::cell::Cell;
use std::path::PathBuf;

use dwell_worldgen::worldmap::{self, FINGERPRINT, WorldMap, cache, codec};

/// A fresh scratch directory for one test (removed by the test at its end).
fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("dwell-map-cache-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("scratch dir");
    d
}

#[test]
fn a_directory_keeps_only_this_builds_maps() {
    // face-a-only: file names only; no terrain.
    let d = scratch("sweep");
    let ours = format!("{FINGERPRINT:016x}");
    let other = if ours == "0123456789abcdef" {
        "fedcba9876543210"
    } else {
        "0123456789abcdef"
    };
    let files = [
        (
            format!("{other}-0000000000000000-0000000000000007.dwmap"),
            false,
        ),
        (
            format!("{other}-0000000000000000-0000000000000007-wabc.dwmap"),
            false,
        ),
        (
            format!("{other}-0000000000000000-0000000000000007.123.tmp"),
            false,
        ),
        (
            format!("{ours}-0000000000000000-0000000000000007.dwmap"),
            true,
        ),
        (
            format!("{ours}-0000000000000000-0000000000000007-wabc.dwmap"),
            true,
        ),
        ("notes.txt".to_owned(), true),
        ("mine.dwmap".to_owned(), true),
    ];
    for (f, _) in &files {
        std::fs::write(d.join(f), b"x").expect("write");
    }
    assert_eq!(cache::sweep(&d), 3);
    for (f, kept) in &files {
        assert_eq!(d.join(f).exists(), *kept, "{f}");
    }
    assert_eq!(cache::sweep(&d), 0, "a second sweep finds nothing");
    std::fs::remove_dir_all(&d).expect("clean up");
}

#[test]
fn a_world_configs_map_is_built_once_and_read_back_the_same() {
    // face-a-only: the cache stores bytes; one face's map exercises it.
    let d = scratch("worldgen");
    // An outdated map already there: swept the first time the cache opens the directory.
    let stale = d.join("0123456789abcdef-0000000000000000-0000000000000007.dwmap");
    std::fs::write(&stale, b"old").expect("write");
    worldmap::set_cache_dir(d.clone());
    let text = "test:realistic";
    let e = dwell_worldgen::config::world_config(text)
        .unwrap()
        .expect("the config");
    let h = e.height.expect("its height");
    let ground = format!("{h:?}");
    let builds = Cell::new(0);
    let build = || {
        builds.set(builds.get() + 1);
        WorldMap::for_height(7, h).with_worldgen(text).unwrap()
    };
    let first = cache::for_worldgen(7, text, &ground, build);
    assert_eq!(builds.get(), 1);
    assert!(
        !stale.exists() || FINGERPRINT == 0x0123_4567_89ab_cdef,
        "the outdated map swept"
    );
    let again = cache::for_worldgen(7, text, &ground, || {
        builds.set(builds.get() + 1);
        unreachable!("read from the cache")
    });
    assert_eq!(builds.get(), 1, "read from disk, not built again");
    assert_eq!(
        codec::encode(&again),
        codec::encode(&first),
        "byte-identical"
    );
    assert_eq!(again.worldgen_text(), text);
    // The same through the helper the tests use, and equal to a map computed fresh.
    let helper = WorldMap::for_height_with_worldgen(7, h, text).expect("the config");
    assert_eq!(builds.get(), 1);
    assert_eq!(
        codec::encode(&helper),
        codec::encode(&build()),
        "the cached map is the computed one"
    );
    // Another seed's, config's or ground's map is a different entry.
    let key = cache::worldgen_key(7, text, &ground);
    assert_ne!(key, cache::worldgen_key(8, text, &ground));
    assert_ne!(key, cache::worldgen_key(7, "test:other", &ground));
    let mut other = h;
    other.inland_rise += 1.0;
    assert_ne!(key, cache::worldgen_key(7, text, &format!("{other:?}")));
    assert!(d.join(format!("{key}.dwmap")).exists());
    std::fs::remove_dir_all(&d).expect("clean up");
}
