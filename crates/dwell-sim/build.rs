//! The app version (RELEASES.md §3). The release workflow passes the exact one as DWELL_VERSION.
//! Without it this is a local build, `<line>.0-dev.local`: a pre-release on the compatibility line
//! in shared/version/line.json (MAJOR.MINOR), not client/package.json's exact version, so a patch
//! release (which raises package.json) recompiles nothing. Build metadata ("+sha") is dropped:
//! worlds record the version without it.

fn main() {
    println!("cargo:rerun-if-env-changed=DWELL_VERSION");
    let line_file = concat!(env!("CARGO_MANIFEST_DIR"), "/../../shared/version/line.json");
    println!("cargo:rerun-if-changed={line_file}");
    let version = match std::env::var("DWELL_VERSION") {
        Ok(v) if !v.is_empty() => v,
        _ => {
            let text = std::fs::read_to_string(line_file).expect("shared/version/line.json");
            let at = text
                .find("\"line\"")
                .expect("a line in shared/version/line.json");
            let rest = &text[at + 6..];
            let start = rest.find('"').expect("line value") + 1;
            let end = start + rest[start..].find('"').expect("line value");
            let line = &rest[start..end];
            let parts: Vec<&str> = line.split('.').collect();
            assert!(
                parts.len() == 2
                    && parts
                        .iter()
                        .all(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit())),
                "shared/version/line.json: line must be MAJOR.MINOR, e.g. 0.52, not {line}"
            );
            format!("{line}.0-dev.local")
        }
    };
    let version = version.split('+').next().unwrap_or_default().to_string();
    println!("cargo:rustc-env=DWELL_APP_VERSION={version}");
}
