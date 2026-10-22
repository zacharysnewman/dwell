//! The world map's fingerprint (`worldmap::FINGERPRINT`): an FNV-1a hash of the sources and data
//! a map is computed from, so a cached map (the browser's IndexedDB, the native disk cache) made by
//! a different build of the generator is never read as this one's (ADR 0049).

use std::path::{Path, PathBuf};

const INPUTS: &[&str] = &[
    "src/base.rs",
    "src/noise.rs",
    "src/island.rs",
    "src/terrain.gen.rs",
    "src/settings.rs",
    "src/settings.gen.rs",
    "src/hydrology",
    "src/worldmap/codec.rs",
    "src/worldmap/mod.rs",
    "../../shared/protocol/constants.json",
];

fn files(p: &Path, out: &mut Vec<PathBuf>) {
    if p.is_dir() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(p)
            .expect("read_dir")
            .map(|e| e.expect("entry").path())
            .collect();
        entries.sort();
        for e in entries {
            files(&e, out);
        }
    } else {
        out.push(p.to_path_buf());
    }
}

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let mut all = Vec::new();
    for i in INPUTS {
        let p = root.join(i);
        println!("cargo:rerun-if-changed={}", p.display());
        files(&p, &mut all);
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for f in &all {
        let name = f
            .strip_prefix(&root)
            .unwrap_or(f)
            .to_string_lossy()
            .replace('\\', "/");
        let text = std::fs::read(f).expect("read input");
        for b in name
            .bytes()
            .chain([0u8])
            .chain(text.into_iter().filter(|&b| b != b'\r'))
        {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("out dir")).join("fingerprint.rs");
    std::fs::write(out, format!("0x{h:016x}")).expect("write fingerprint");
}
