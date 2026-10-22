//! The dedicated server's host logic (ARCHITECTURE.md §4, §10.1), kept out of `main.rs` so it is
//! tested: launch options, the world file's settings and permissions, the version lock on worlds
//! (RELEASES.md §6), the master heartbeat and the invite link.

use std::path::PathBuf;

use dwell_protocol::{PROTOCOL_VERSION, PublicKey};
use dwell_sim::app_version::{APP_VERSION, world_version_error};
use dwell_sim::server::{EditPolicy, ServerConfig, WorldSettings};
use dwell_storage::{PermissionEntry, PermissionKind, WorldDb, WorldStore};

/// The deployed master server (ARCHITECTURE.md §10.3); `--master` overrides it.
pub const DEFAULT_MASTER: &str = "https://dwell-master.dropkick.workers.dev";

pub const USAGE: &str =
    "usage: dwell-server [--world FILE] [--port N] [--rtc-port N] [--advertise IP]
                    [--name NAME] [--motd TEXT] [--max-players N] [--edits POLICY]
                    [--flight POLICY] [--seed N] [--generator N] [--preset NAME]
                    [--worldgen SETTINGS] [--op KEY] [--ban KEY]
                    [--client-url URL] [--master URL] [--visibility public|unlisted|none]
                    [--tags A,B]
  --world FILE    the world file (default world.dwellworld; created if missing; \"\" keeps
                  the world in memory only)
  --advertise IP  address players use to reach this server (invite links, WebRTC)
  --seed N, --generator N  a new world's seed and generator: 8 = procedural terrain
                  (default), 0 = flat (a saved world keeps its own)
  --preset NAME   a new world's generation preset: default, highlands, archipelago,
                  lakeCountry (shared/worldgen/settings.json; a saved world keeps its own)
  --worldgen S    a new world's generation settings, id:value pairs over the preset, e.g.
                  land:0.8,ranges:5,rain:1.5 (sliders: land, coast, height, ranges, rain,
                  erosion, lakes, snow, treeline, trees, portals, bedrock)
  --name, --motd, --max-players, --edits everyone|ops|nobody, --flight everyone|ops|nobody
                  saved in the world's settings (--flight: who may use creative flight)
  --op KEY, --ban KEY  grant op or ban a player (hex device public key), saved in the world
  --master URL    the master server (default https://dwell-master.dropkick.workers.dev)
  --visibility V  unlisted (default): players join by code or address and see it on its
                  own network; public: also in the lobby list; none: never contacts the
                  master
  --tags A,B      tags players can search the lobby list by (e.g. pve,creative)
  --heartbeat S   seconds between master heartbeats (default 30; testing)";

pub struct Options {
    pub port: u16,
    /// 0 = port + 1.
    pub rtc_port: u16,
    pub advertise: String,
    /// `--advertise` given: also the address the master hands out.
    pub advertise_given: bool,
    pub master: String,
    /// unlisted (by code, address and on its network), public (also in the lobby list) or none.
    pub visibility: String,
    pub heartbeat_s: u64,
    pub tags: Vec<String>,
    /// Ed25519 seed for the master, kept in the world's settings.
    pub server_key: [u8; 32],
    pub server: ServerConfig,
    pub client_url: String,
    /// Empty: nothing is saved.
    pub world: String,
    /// Launch options that also change the world's settings table (§6.4, §10.1).
    pub settings: Vec<(String, String)>,
    /// Grants (kind, hex key).
    pub grants: Vec<(PermissionKind, String)>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            port: 4433,
            rtc_port: 0,
            advertise: "127.0.0.1".into(),
            advertise_given: false,
            master: DEFAULT_MASTER.into(),
            visibility: "unlisted".into(),
            heartbeat_s: 30,
            tags: Vec::new(),
            server_key: [0; 32],
            server: ServerConfig::default(),
            client_url: "http://localhost:5173/dwell/".into(),
            world: "world.dwellworld".into(),
            settings: Vec::new(),
            grants: Vec::new(),
        }
    }
}

fn policy_ok(v: &str) -> bool {
    matches!(v, "everyone" | "ops" | "nobody")
}

/// Bad or missing launch options, or `--help`: print [`USAGE`].
#[derive(Debug, PartialEq, Eq)]
pub struct UsageError;

/// Parses the launch options (without the program name).
pub fn parse_options(args: &[String]) -> Result<Options, UsageError> {
    let mut o = Options::default();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        if arg == "--help" || arg == "-h" {
            return Err(UsageError);
        }
        let v = it.next().ok_or(UsageError)?.as_str();
        match arg.as_str() {
            "--port" => o.port = v.parse().map_err(|_| UsageError)?,
            "--rtc-port" => o.rtc_port = v.parse().map_err(|_| UsageError)?,
            "--advertise" => {
                o.advertise = v.into();
                o.advertise_given = true;
            }
            "--master" => o.master = v.into(),
            "--visibility" if matches!(v, "public" | "unlisted" | "none") => {
                o.visibility = v.into()
            }
            "--tags" => {
                o.tags = v
                    .split(',')
                    .map(|t| t.replace(' ', ""))
                    .filter(|t| !t.is_empty())
                    .collect()
            }
            "--heartbeat" => o.heartbeat_s = v.parse::<u64>().map_err(|_| UsageError)?.clamp(2, 60),
            "--world" => o.world = v.into(),
            "--name" | "--motd" | "--max-players" => {
                o.settings.push((arg[2..].replace('-', "_"), v.into()))
            }
            "--edits" | "--flight" if policy_ok(v) => o.settings.push((arg[2..].into(), v.into())),
            "--op" => o.grants.push((PermissionKind::Op, v.into())),
            "--ban" => o.grants.push((PermissionKind::Ban, v.into())),
            "--seed" => o.server.world_seed = v.parse().map_err(|_| UsageError)?,
            "--generator" => o.server.generator_version = v.parse().map_err(|_| UsageError)?,
            "--preset" => {
                let mut preset = WorldSettings::preset(v).ok_or(UsageError)?;
                preset
                    .apply_encoded(&o.server.worldgen.encode())
                    .map_err(|_| UsageError)?;
                o.server.worldgen = preset;
            }
            "--worldgen" => o.server.worldgen.apply_encoded(v).map_err(|_| UsageError)?,
            "--client-url" => o.client_url = v.into(),
            _ => return Err(UsageError),
        }
    }
    Ok(o)
}

fn policy(v: &str) -> EditPolicy {
    match v {
        "ops" => EditPolicy::Ops,
        "nobody" => EditPolicy::Nobody,
        _ => EditPolicy::Everyone,
    }
}

/// Applies a setting to the server config (§10.1): stored settings first, launch options after.
pub fn apply_setting(c: &mut ServerConfig, key: &str, value: &str) {
    match key {
        "name" => c.name = value.into(),
        "motd" => c.motd = value.into(),
        "max_players" => c.max_players = value.parse().unwrap_or(c.max_players),
        "edits" => c.edits = policy(value),
        "flight" => c.flight = policy(value),
        "autosave_seconds" => c.autosave_seconds = value.parse::<u32>().unwrap_or(1).max(1),
        "allow_list" if value == "1" && c.allow_list.is_none() => c.allow_list = Some(Vec::new()),
        _ => {}
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn parse_hex<const N: usize>(text: &str) -> Option<[u8; N]> {
    if text.len() != 2 * N || !text.is_ascii() {
        return None;
    }
    let mut out = [0u8; N];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&text[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}

/// Opens the world file and folds its settings and permissions (plus the launch options, which
/// are saved into it) into the config; the store goes into `o.server.store`. Returns what to
/// print, or why the world can't be opened (nothing is written to a world this build may not
/// open).
pub fn open_world(o: &mut Options) -> Result<Vec<String>, String> {
    for (key, value) in &o.settings {
        apply_setting(&mut o.server, key, value);
    }
    // The server's key for the master (§10.1): new each run unless the world keeps it.
    getrandom::fill(&mut o.server_key).map_err(|e| e.to_string())?;
    if o.world.is_empty() {
        return Ok(vec![
            "world: in memory only (--world \"\"): nothing is saved".into(),
        ]);
    }
    let path = PathBuf::from(&o.world);
    // Worlds are locked to their compatibility line (RELEASES.md §6): refuse one this build may
    // not open, naming the version to run. Its meta is read read-only, so a refused file is never
    // opened for writing.
    let cannot = |e: String| format!("cannot open world file {}: {e}", o.world);
    if let Some(meta) = WorldDb::peek_meta(&path).map_err(cannot)? {
        let locked = world_version_error(APP_VERSION, &meta.app_version_last);
        if !locked.is_empty() {
            return Err(cannot(locked));
        }
    }
    let store = WorldStore::open_file(&path).map_err(cannot)?;
    let db = store.db().clone();
    let meta = db.load_meta();
    match db.setting("server_key").and_then(|k| parse_hex::<32>(&k)) {
        Some(k) => o.server_key = k,
        None => db.set_setting("server_key", &hex(&o.server_key))?,
    }
    let c = &mut o.server;
    for key in [
        "name",
        "motd",
        "max_players",
        "edits",
        "flight",
        "autosave_seconds",
        "allow_list",
    ] {
        if let Some(value) = db.setting(key) {
            apply_setting(c, key, &value);
        }
    }
    for (key, value) in &o.settings {
        apply_setting(c, key, value);
        db.set_setting(key, value)?;
    }
    for (kind, text) in &o.grants {
        let key: PublicKey =
            parse_hex(text).ok_or_else(|| format!("not a device public key: {text}"))?;
        db.set_permission(&PermissionEntry {
            key,
            kind: *kind,
            reason: String::new(),
            by: None,
            at: 0,
        })?;
    }
    for e in db.permissions() {
        match e.kind {
            PermissionKind::Op => c.ops.push(e.key),
            PermissionKind::Ban => c.banned.push(e.key),
            PermissionKind::Allow => {
                if let Some(list) = &mut c.allow_list {
                    list.push(e.key);
                }
            }
        }
    }
    let mut lines = Vec::new();
    if let Some(m) = &meta
        && (m.world_seed != c.world_seed
            || m.generator_version != c.generator_version
            || m.worldgen != c.worldgen.encode())
    {
        lines.push(format!(
            "world: {} keeps its own seed {}, generator {} and generation settings {:?}",
            o.world, m.world_seed, m.generator_version, m.worldgen
        ));
    }
    lines.push(format!(
        "world: {} ({}, {} modified chunks)",
        o.world,
        if meta.is_some() { "loaded" } else { "new" },
        db.chunk_index().len()
    ));
    c.store = Some(store);
    Ok(lines)
}

/// This machine's LAN addresses (IPv4 and non-link-local IPv6, no loopback), reported to the
/// master so players on the same network can type them (§10.3).
pub fn lan_addresses() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for a in if_addrs::get_if_addrs().unwrap_or_default() {
        if a.is_loopback() {
            continue;
        }
        let ip = a.ip();
        if let std::net::IpAddr::V6(v6) = ip
            && v6.segments()[0] & 0xffc0 == 0xfe80
        {
            continue; // link-local
        }
        let text = ip.to_string();
        if !out.contains(&text) && out.len() < 8 {
            out.push(text);
        }
    }
    out
}

/// What the front-end listens on, for the invite link and the heartbeat.
pub struct Listening {
    pub port: u16,
    pub rtc_port: u16,
    pub cert_hex: String,
    pub ice_ufrag: String,
    pub ice_pwd: String,
}

/// The invite link: `v` is this server's app version, so the launcher opens a client build on its
/// compatibility line (RELEASES.md §5). IPv6 literals get brackets in host:port.
pub fn invite_link(client_url: &str, advertise: &str, l: &Listening) -> String {
    let host = if advertise.contains(':') {
        format!("[{advertise}]")
    } else {
        advertise.to_string()
    };
    format!(
        "{client_url}?join={host}:{}&cert={}&rtc={}&ice={}:{}&v={APP_VERSION}",
        l.port, l.cert_hex, l.rtc_port, l.ice_ufrag, l.ice_pwd
    )
}

/// The heartbeat's JSON body (ARCHITECTURE.md §10.3, `POST /v1/servers`).
pub fn heartbeat_body(o: &Options, l: &Listening, lan: &[String], players: usize) -> String {
    let c = &o.server;
    let mut body = serde_json::json!({
        "port": l.port,
        "rtcPort": l.rtc_port,
        "ice": format!("{}:{}", l.ice_ufrag, l.ice_pwd),
        "cert": l.cert_hex,
        "lan": lan,
        "name": c.name,
        "motd": c.motd,
        "players": players,
        "maxPlayers": c.max_players,
        "protocol": PROTOCOL_VERSION,
        "appVersion": APP_VERSION,
        "visibility": o.visibility,
        "tags": o.tags,
        "heartbeatS": o.heartbeat_s,
    });
    if o.advertise_given {
        body["advertise"] = serde_json::Value::from(o.advertise.clone());
    }
    body.to_string()
}
