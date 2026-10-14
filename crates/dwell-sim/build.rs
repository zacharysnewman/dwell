//! The app version (RELEASES.md §3): the one in client/package.json is the next release's; the
//! release workflow passes the exact one as DWELL_VERSION. Without it this is a local build, a
//! pre-release of the next version. Build metadata ("+sha") is dropped: worlds record the version
//! without it.

fn main() {
    println!("cargo:rerun-if-env-changed=DWELL_VERSION");
    let package = concat!(env!("CARGO_MANIFEST_DIR"), "/../../client/package.json");
    println!("cargo:rerun-if-changed={package}");
    let version = match std::env::var("DWELL_VERSION") {
        Ok(v) if !v.is_empty() => v,
        _ => {
            let text = std::fs::read_to_string(package).expect("client/package.json");
            let at = text
                .find("\"version\"")
                .expect("a version in client/package.json");
            let rest = &text[at + 9..];
            let start = rest.find('"').expect("version value") + 1;
            let end = start + rest[start..].find('"').expect("version value");
            format!("{}-dev.local", &rest[start..end])
        }
    };
    let version = version.split('+').next().unwrap_or_default().to_string();
    println!("cargo:rustc-env=DWELL_APP_VERSION={version}");
}
