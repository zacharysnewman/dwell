//! The dedicated server's host logic: launch options, world settings and permissions, the version
//! lock on worlds (RELEASES.md §6, formerly version_lock_test.py on SQLite files), the heartbeat
//! and the invite link; and a smoke run of the real binary.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use dwell_server::*;
use dwell_sim::app_version::APP_VERSION;
use dwell_sim::server::EditPolicy;
use dwell_storage::{SaveBatch, WorldDb, WorldMeta};

/// A unique scratch directory, removed on drop.
struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "dwell-server-{}-{}-{name}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn file(&self, name: &str) -> String {
        self.0.join(name).to_string_lossy().into_owned()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

/// A world file "saved by" `version` (empty: before versioned releases).
fn world_saved_by(path: &str, version: &str) {
    let db = WorldDb::open(std::path::Path::new(path)).unwrap();
    let meta = WorldMeta {
        world_seed: 7,
        generator_version: 0,
        spawn: None,
        world_tick: 0,
        created_at: 0,
        saved_at: 0,
        app_version_created: version.into(),
        app_version_last: version.into(),
    };
    db.save(&SaveBatch {
        meta: Some(meta),
        ..SaveBatch::default()
    })
    .unwrap();
}

fn file_bytes(path: &str) -> Vec<u8> {
    std::fs::read(path).unwrap()
}

#[test]
fn options_parse_and_reject_what_they_should() {
    let o = parse_options(&args(&[
        "--port",
        "5000",
        "--tags",
        " pve, creative,,",
        "--edits",
        "ops",
        "--max-players",
        "4",
        "--heartbeat",
        "999",
    ]))
    .unwrap();
    assert_eq!(o.port, 5000);
    assert_eq!(o.tags, ["pve", "creative"]);
    assert_eq!(o.heartbeat_s, 60, "clamped");
    assert!(o.settings.contains(&("edits".into(), "ops".into())));
    assert!(o.settings.contains(&("max_players".into(), "4".into())));
    for bad in [
        &["--edits", "some"][..],
        &["--visibility", "secret"],
        &["--port"],
        &["--what", "x"],
        &["--help"],
    ] {
        assert!(parse_options(&args(bad)).is_err(), "{bad:?}");
    }
}

#[test]
fn settings_apply_to_the_server_config() {
    let mut c = dwell_sim::server::ServerConfig::default();
    apply_setting(&mut c, "name", "Hill");
    apply_setting(&mut c, "max_players", "3");
    apply_setting(&mut c, "flight", "nobody");
    apply_setting(&mut c, "autosave_seconds", "0");
    apply_setting(&mut c, "allow_list", "1");
    assert_eq!(
        (c.name.as_str(), c.max_players, c.flight, c.autosave_seconds),
        ("Hill", 3, EditPolicy::Nobody, 1)
    );
    assert!(c.allow_list.is_some());
}

#[test]
fn a_new_world_keeps_the_launch_settings_grants_and_server_key() {
    let s = Scratch::new("settings");
    let world = s.file("w.dwellworld");
    let op = "11".repeat(32);
    let mut o = parse_options(&args(&["--world", &world, "--name", "Keep", "--op", &op])).unwrap();
    let lines = open_world(&mut o).unwrap();
    assert!(lines.iter().any(|l| l.contains("new")), "{lines:?}");
    assert_eq!(o.server.ops, vec![[0x11; 32]]);
    let key = o.server_key;
    drop(o);
    // Reopened without options: the stored settings, the op and the same server key.
    let mut again = parse_options(&args(&["--world", &world])).unwrap();
    open_world(&mut again).unwrap();
    assert_eq!(again.server.name, "Keep");
    assert_eq!(again.server.ops, vec![[0x11; 32]]);
    assert_eq!(again.server_key, key);
    drop(again); // the world file is locked while open
    // A bad key is refused.
    let mut bad = parse_options(&args(&["--world", &world, "--ban", "zz"])).unwrap();
    assert!(
        open_world(&mut bad)
            .unwrap_err()
            .contains("not a device public key")
    );
}

#[test]
fn the_version_lock_opens_its_own_line_and_refuses_others_untouched() {
    let s = Scratch::new("lock");
    // Its own version: opened.
    let own = s.file("own.dwellworld");
    world_saved_by(&own, APP_VERSION);
    let mut o = parse_options(&args(&["--world", &own])).unwrap();
    assert!(open_world(&mut o).is_ok());
    drop(o);
    // Another line, a newer build on this line, and an unversioned world: refused, naming the
    // version to run, and the file is not touched.
    let line = dwell_sim::app_version::compatibility_line(APP_VERSION);
    let newer = format!("{APP_VERSION}.9999"); // a later pre-release of this build: newer, maybe another line
    for (name, version, says) in [
        (
            "other.dwellworld",
            "99.0.0".to_string(),
            "line 99".to_string(),
        ),
        (
            "old.dwellworld",
            String::new(),
            "before versioned releases".to_string(),
        ),
        ("newer.dwellworld", newer, "Dwell".to_string()),
    ] {
        let path = s.file(name);
        world_saved_by(&path, &version);
        let before = file_bytes(&path);
        let mut o = parse_options(&args(&["--world", &path])).unwrap();
        let err = open_world(&mut o).unwrap_err();
        assert!(
            err.contains(&says),
            "{name}: {err} (this build {APP_VERSION}, line {line})"
        );
        drop(o);
        assert!(file_bytes(&path) == before, "{name} was modified");
    }
}

#[test]
fn the_invite_link_and_the_heartbeat_carry_the_version_and_addresses() {
    let l = Listening {
        port: 4433,
        rtc_port: 4434,
        cert_hex: "ab".into(),
        ice_ufrag: "u".into(),
        ice_pwd: "p".into(),
    };
    let link = invite_link("http://x/dwell/", "::1", &l);
    assert_eq!(
        link,
        format!("http://x/dwell/?join=[::1]:4433&cert=ab&rtc=4434&ice=u:p&v={APP_VERSION}")
    );
    let mut o = parse_options(&args(&["--advertise", "10.0.0.2", "--tags", "pve"])).unwrap();
    o.server.name = "Quote \"me\"".into();
    let body: serde_json::Value =
        serde_json::from_str(&heartbeat_body(&o, &l, &["192.168.1.5".into()], 3)).unwrap();
    assert_eq!(body["name"], "Quote \"me\"");
    assert_eq!(body["players"], 3);
    assert_eq!(body["advertise"], "10.0.0.2");
    assert_eq!(body["appVersion"], APP_VERSION);
    assert_eq!(body["ice"], "u:p");
    assert_eq!(body["lan"][0], "192.168.1.5");
    let o = parse_options(&[]).unwrap();
    let body: serde_json::Value = serde_json::from_str(&heartbeat_body(&o, &l, &[], 0)).unwrap();
    assert!(
        body.get("advertise").is_none(),
        "only an explicit --advertise is handed out"
    );
}

#[test]
fn the_binary_starts_prints_its_invite_link_and_saves_on_sigint() {
    let s = Scratch::new("smoke");
    let world = s.file("smoke.dwellworld");
    let mut child = Command::new(env!("CARGO_BIN_EXE_dwell-server"))
        .args([
            "--world",
            &world,
            "--visibility",
            "none",
            "--port",
            "0",
            "--rtc-port",
            "0",
            "--generator",
            "0",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary runs");
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut seen = String::new();
    while Instant::now() < deadline {
        let mut line = String::new();
        if out.read_line(&mut line).unwrap() == 0 {
            break;
        }
        seen.push_str(&line);
        if line.starts_with("invite link:") {
            break;
        }
    }
    assert!(
        seen.contains("invite link:") && seen.contains(&format!("&v={APP_VERSION}")),
        "{seen}"
    );
    // SIGINT: a clean shutdown that saves the world.
    Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .unwrap();
    let mut rest = String::new();
    std::io::Read::read_to_string(&mut out, &mut rest).unwrap();
    let status = child.wait().unwrap();
    assert!(status.success(), "{status}: {rest}");
    assert!(rest.contains("world saved"), "{rest}");
    let db = WorldDb::open(std::path::Path::new(&world)).unwrap();
    assert_eq!(db.load_meta().unwrap().app_version_last, APP_VERSION);
}
