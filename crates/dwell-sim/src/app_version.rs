//! The app version (RELEASES.md §3, §6): Semantic Versioning 2.0.0 and the compatibility lines that
//! lock worlds to builds. Mirrors client/src/version/semver.ts; both are checked against
//! shared/version/vectors.txt.

use std::cmp::Ordering;

/// The version this build carries, without build metadata: "0.1.0" for a release, "0.2.0-dev.42"
/// for a dev build (build.rs: DWELL_VERSION, or client/package.json's as a local pre-release).
pub const APP_VERSION: &str = env!("DWELL_APP_VERSION");

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// Pre-release identifiers; empty for a release.
    pub pre: Vec<String>,
}

fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit())
}
fn ident_ok(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
}
fn number(s: &str) -> Option<u64> {
    if !all_digits(s) || (s.len() > 1 && s.starts_with('0')) {
        return None;
    }
    s.parse().ok()
}

/// Strict SemVer 2.0.0 (no leading "v", no missing parts); build metadata is accepted and dropped.
pub fn parse_version(text: &str) -> Option<Version> {
    let (text, build) = match text.split_once('+') {
        Some((t, b)) => (t, Some(b)),
        None => (text, None),
    };
    if let Some(build) = build
        && !build.split('.').all(ident_ok)
    {
        return None;
    }
    let (core, pre) = match text.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (text, None),
    };
    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let mut v = Version {
        major: number(parts[0])?,
        minor: number(parts[1])?,
        patch: number(parts[2])?,
        pre: Vec::new(),
    };
    if let Some(pre) = pre {
        for id in pre.split('.') {
            if !ident_ok(id) || (all_digits(id) && id.len() > 1 && id.starts_with('0')) {
                return None;
            }
            v.pre.push(id.to_string());
        }
    }
    Some(v)
}

fn compare_pre(a: &[String], b: &[String]) -> Ordering {
    // A release outranks its pre-releases (SemVer §11.3).
    match (a.is_empty(), b.is_empty()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        _ => {}
    }
    for i in 0..a.len().max(b.len()) {
        let (Some(x), Some(y)) = (a.get(i), b.get(i)) else {
            return a.len().cmp(&b.len());
        };
        let order = match (all_digits(x), all_digits(y)) {
            // Numeric identifiers: by value (no leading zeros, so by length, then text).
            (true, true) => x.len().cmp(&y.len()).then_with(|| x.cmp(y)),
            // Numeric identifiers rank below alphanumeric ones.
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (false, false) => x.cmp(y),
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    Ordering::Equal
}

/// Precedence of `a` against `b` (build metadata ignored).
pub fn compare_versions(a: &Version, b: &Version) -> Ordering {
    (a.major, a.minor, a.patch)
        .cmp(&(b.major, b.minor, b.patch))
        .then_with(|| compare_pre(&a.pre, &b.pre))
}

/// The compatibility line: "0.1" before 1.0.0, MAJOR from it, the exact version for a
/// pre-release. Empty for an invalid version.
pub fn compatibility_line(version: &str) -> String {
    let Some(v) = parse_version(version) else {
        return String::new();
    };
    if !v.pre.is_empty() {
        return format!("{}.{}.{}-{}", v.major, v.minor, v.patch, v.pre.join("."));
    }
    if v.major == 0 {
        format!("0.{}", v.minor)
    } else {
        v.major.to_string()
    }
}

/// Whether a build may open a world last saved by `world_last`: the same line, and not older.
pub fn can_open_world(build: &str, world_last: &str) -> bool {
    let (Some(b), Some(w)) = (parse_version(build), parse_version(world_last)) else {
        return false;
    };
    compatibility_line(build) == compatibility_line(world_last)
        && compare_versions(&b, &w) != Ordering::Less
}

/// Why `build` may not open a world last saved by `world_last` ("" if it may): a message for the
/// player naming the version to use. An empty `world_last` is a world saved before versioned
/// releases, which no build opens.
pub fn world_version_error(build: &str, world_last: &str) -> String {
    if world_last.is_empty() {
        return "this world was saved before versioned releases and can't be opened by any version"
            .into();
    }
    if can_open_world(build, world_last) {
        return String::new();
    }
    let line = compatibility_line(world_last);
    if line.is_empty() {
        return format!("this world records an unreadable version ({world_last})");
    }
    if line == compatibility_line(build) {
        return format!(
            "this world was last saved by Dwell {world_last}, newer than this build ({build}); run Dwell {world_last} or later on the {line} line"
        );
    }
    format!(
        "this world was saved by Dwell {world_last} (line {line}), which this build ({build}) can't open; run a Dwell build on that line"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shared_vectors_hold() {
        let text = include_str!("../../../shared/version/vectors.txt");
        let mut checked = 0;
        for line in text
            .lines()
            .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        {
            let f: Vec<&str> = line.split_whitespace().collect();
            match f[0] {
                "compare" => {
                    let (a, b) = (parse_version(f[1]).unwrap(), parse_version(f[2]).unwrap());
                    let want = match f[3] {
                        "-1" => Ordering::Less,
                        "0" => Ordering::Equal,
                        _ => Ordering::Greater,
                    };
                    assert_eq!(compare_versions(&a, &b), want, "{line}");
                }
                "line" => assert_eq!(
                    compatibility_line(f[1]),
                    if f[2] == "-" { "" } else { f[2] },
                    "{line}"
                ),
                "open" => assert_eq!(can_open_world(f[1], f[2]), f[3] == "YES", "{line}"),
                "invalid" => assert!(
                    parse_version(f.get(1).copied().unwrap_or("")).is_none(),
                    "{line}"
                ),
                other => panic!("unknown vector kind {other}"),
            }
            checked += 1;
        }
        assert!(checked > 20, "{checked}");
    }

    #[test]
    fn this_build_has_a_valid_version_and_messages_name_the_line() {
        assert!(parse_version(APP_VERSION).is_some(), "{APP_VERSION}");
        assert!(can_open_world(APP_VERSION, APP_VERSION));
        assert!(world_version_error("0.2.0", "0.1.4").contains("line 0.1"));
        assert!(world_version_error("0.1.3", "0.1.4").contains("newer than this build"));
        assert!(world_version_error("0.1.3", "").contains("before versioned releases"));
        assert_eq!(world_version_error("0.1.4", "0.1.3"), "");
    }
}
