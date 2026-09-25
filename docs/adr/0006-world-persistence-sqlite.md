# 0006. World persistence: one SQLite database per world, holding all data

- Status: Accepted
- Date: 2026-09-25
- Resolves: ARCHITECTURE.md Open Decisions #5

## Context

Player hosting (ADR 0003) makes persistence mandatory. The same world must be saved by
dedicated servers (native disk), friend worlds and single-player (browser, inside the sim-core
worker), and the mobile apps, and must move between them as one export file. Saves must be
atomic (no half-written worlds), incremental (autosave without tick hitches), versioned, and
backed up on dedicated servers. Unmodified chunks never need storing: worldgen reproduces them
from the seed (ARCHITECTURE.md §6.3).

## Options considered

1. **Custom region files** (Minecraft Java): tailored, but we would own crash safety,
   fragmentation, and compaction; a folder is awkward to export from a browser.
2. **SQLite, one file per world:** transactional, single file, mature tooling and online backup,
   the same C++ code natively and in WASM; costs a few hundred KB of WASM and a browser storage
   adapter.
3. **LevelDB/RocksDB** (Minecraft Bedrock): awkward in browsers, folder-based.
4. **Per-platform storage** (files natively, IndexedDB in browsers): two formats plus conversion.

## Decision

Each world is **one SQLite database file**, and **all world data lives in it** — including world
settings and permissions. Nothing about a world is kept in side files.

- SQLite is compiled into `server/core` (`core/storage`), used identically by the native server
  and the WASM build.
- **Storage backends (SQLite VFS):**
  - Native: the standard file VFS, WAL journal mode.
  - Browser: a VFS over OPFS `FileSystemSyncAccessHandle`s in the sim-core worker. This needs no
    cross-origin isolation (important on GitHub Pages). Files live under a `dwell/` OPFS
    directory (ADR 0005).
  - Capacitor: the OPFS VFS where the WebView supports sync access handles; otherwise a native
    file VFS through a plugin (verified per platform in Phase 7).
- **Schema (v1 sketch):**
  - `meta` — format version, world seed, generator version, spawn, world time, created/updated.
  - `settings` — server/world settings: name, MOTD, icon, max players, visibility, password
    hash, online/offline mode, physics and view-distance caps, autosave and backup policy.
  - `chunks` — `(cx, cy, cz)` primary key, revision, generator version, blob. Only modified
    chunks. Blob = the same palette + RLE encoding as `ChunkData Explicit`, zstd-compressed.
  - `players` — device public key (primary key), display name, blob (position, health, later
    inventory), first/last seen.
  - `bodies` — in-flight Tier 1 clusters (voxel layout, transform, velocities), so a world saved
    mid-collapse resumes it.
  - `permissions` — ops, bans, allow-list, keyed by public key, with reason/by/when.
- **Save policy:** autosave every `AUTOSAVE_SECONDS` writes dirty chunks, players, bodies, and
  meta in **one transaction**; also on shutdown and, for friend worlds, when the host
  backgrounds. Writes are prepared on the tick thread and committed off it (native: I/O thread;
  browser: within the worker, between ticks).
- **Backups (dedicated servers):** rotating backups via SQLite's online backup API on a schedule
  from `settings`.
- **Export/import:** a `.dwellworld` file is the database itself (optionally with an embedded
  preview image in `meta`). The same file imports into a dedicated server, a browser, or an app.
- **Operator tooling:** admin commands (in-game and server console) edit `settings` and
  `permissions`; a server CLI can set values before first start. The only non-database inputs
  to a dedicated server are process-level launch options (world file path, bind address/port).
- **Migrations:** `meta.format_version` plus ordered C++ migrations run on open.
- **zstd** is compiled into the core as well, available for large network transfers.

## Consequences

- A world is a single portable file with atomic, crash-safe saves on every platform.
- The WASM build grows by SQLite + zstd (a few hundred KB compressed).
- Operators edit settings through commands or the CLI rather than a hand-edited file; the
  server can print and import settings as JSON for convenience, but the database stays the source
  of truth.
- Supersedes the "config file" wording in ADR 0003's operator section for world settings.
