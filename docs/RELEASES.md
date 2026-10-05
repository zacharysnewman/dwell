# Dwell — Versioned Releases: Builds by Tag, a Version Launcher, Version-Locked Worlds

> **Status: [planned]** — the design for the versioned-releases phase of
> [`IMPLEMENTATION_PLAN.md`](./IMPLEMENTATION_PLAN.md). As it lands, the built parts move into
> [`ARCHITECTURE.md`](./ARCHITECTURE.md) (§2.1 deployment, §3.1 CI, §6.4 persistence, §10.5
> versioning) and an ADR; this file keeps the rationale and the setup steps.

## 1. Goals (owner, 2026-10-05)

1. **Every build stays playable.** Each build is a **git tag** (with a GitHub Release holding the
   built files), and the app — even the web app — **loads its versions dynamically from those
   tags**.
2. **Worlds record the app version they were created with** and are, for now, **locked** to it: a
   world always opens in its own version. Upgrading worlds is a later option
   ([`FUTURE.md`](./FUTURE.md)).
3. **The source stays public, under a restrictive license.** A split into a private source repo
   and a public deploy repo was considered and dropped (§2): a license covers what the split was
   for, while the split would have cost the free CI pipeline.

## 2. One public repository, and a license

The repository stays **public** at `zacharysnewman/dwell`, and the site stays at
`https://dropkickarcade.com/dwell/` (ADR 0005). Nothing is renamed or moved.

**Why not a private source repo with a public deploy repo** (considered 2026-10-05, rejected):
private repositories get a monthly allowance of GitHub-hosted Actions minutes (2,000 on the free
plan) where public ones are unlimited, and Dwell's CI (native and WASM builds, two origins,
Playwright e2e, Electron smoke) would likely exceed it; Pages from a private repo needs a paid plan; the
history is already public; and the shipped JavaScript and WASM are downloadable anyway. Running
the CI in the public repo against private source would leak source through public logs and
caches. The owner's aim — that others may read the code but not reuse it — is what a license
does.

**License** (a deliverable, chosen by the owner — not legal advice). Today the repository has no
license file, which by default reserves all rights (GitHub's terms still let people view and
fork it on GitHub), but says so nowhere. Options to weigh:

- **All rights reserved**, stated in a `LICENSE` file: reading allowed (by GitHub's terms), no
  other use. Simplest; contributions would need a separate agreement.
- **A source-available license** that grants limited rights, e.g. **PolyForm Strict** (use for
  personal, noncommercial purposes only; no changes or redistribution) or **PolyForm
  Noncommercial** (noncommercial use including changes); or the **Business Source License**
  (source-available now, converting to an open license after a set date).

Whichever is chosen goes in `LICENSE` at the root, in `package.json` / `Cargo.toml` license fields,
and in the README.

**Third-party notices.** Builds ship code under licenses that require their notices in copies
(Jolt Physics, Three.js, `wtransport` and the Rust crates, zstd, and others; SQLite is public
domain). Each build includes a generated `THIRD_PARTY_NOTICES` file (from the npm, cargo and CMake
dependency lists), linked from the menu's About screen and shipped beside the native server.

## 3. Builds, versions and tags

- **App version:** one semantic version for the whole app — client, sim core, generator, protocol —
  e.g. `0.12.0`. `protocolVersion` and the generator versions stay as internal numbers and are
  recorded with each build. Dev builds are `0.12.0-dev.<yyyymmdd>.<sha7>`.
- **Channels:** **stable** (a release: pushing a `v<version>` tag, or a manual "Release" workflow)
  and **dev** (every push to `main`). Both are published; the launcher defaults to stable and
  offers dev in settings.
- **Each build is built for its own path:** Vite `base: '/dwell/v/<version>/'`, the version
  embedded (shown in the HUD and the menu, replacing today's commit SHA overlay). Source maps may
  be published, since the source is public.
- **Publishing:** the release workflow creates a **GitHub Release** for the tag — `v<version>`
  (stable) or `dev-<version>` (a pre-release) — and attaches the build as an archive plus its
  `build.json` (version, channel, date, commit, `protocolVersion`, generator versions, minimum
  launcher version). The tags and their releases **are** the builds. Keeping builds as release
  assets, not commits, keeps the repository's history free of build output.
- **The launcher** (a small page, source in `launcher/`) is versioned separately and deployed with
  the site.

## 4. The site, assembled from the tags

The Pages workflow (`pages.yml`, triggered after each release and on launcher changes) assembles
the site **from the releases** and deploys it with `actions/deploy-pages`, as today:

```
/dwell/index.html              the launcher
/dwell/versions.json           the manifest, generated from the releases' build.json files
/dwell/v/<version>/...         each release's build, unpacked, immutable
```

- **Which releases:** every stable release; the newest `DEV_KEEP` (≈ 10) dev pre-releases. Older
  dev pre-releases and their tags are deleted by the workflow.
- **Size:** a Pages site is limited to 1 GB. At ~5–10 MB per build (measure) that is ~100+ builds.
  When it gets close, drop the oldest stable builds that no world or server uses (§6 counts them
  through the master).
- **Caching:** version directories never change; the launcher and the manifest are small and
  short-lived (Pages caches for ~10 minutes).
- All of this runs in the public repository's free Actions minutes.

**Why serve versions from Pages and not straight from release assets or a CDN by tag:** release
asset downloads are not served with CORS headers or the right content types for scripts; the app's
CSP is `script-src 'self'` / `worker-src 'self'`; module workers and WASM need same-origin (or extra
wrapping) to load; and all versions must share the origin's storage (worlds, settings, device
key). Same-origin version directories satisfy all of that with no exceptions.

## 5. The launcher

`https://dropkickarcade.com/dwell/` is the launcher — tiny, stable, and the only page that is not
versioned:

1. Fetch `versions.json` (same origin; GitHub's tags API is not needed at runtime and its rate
   limits are avoided).
2. Pick the version:
   - `?play=<world id>` → the world's recorded version (§6);
   - a friend-world code or invite → the host's version (the master reports it, §7);
   - a dedicated server address → the latest stable; on a version mismatch the server's `Reject`
     names its version and the client offers to reopen in it;
   - otherwise → the latest stable (or dev, if chosen in settings), which shows the main menu.
3. Navigate to `/dwell/v/<version>/` with the same query, by `location.replace` (no extra history
   entry).

**Back to the menu always goes to `/dwell/`**, so a player is never stuck in an old version's
menu. Links shared by players use `/dwell/?…`, never a version path, so they keep working.
Missing or unreachable versions show a clear message with the choice to open the latest.

## 6. Worlds locked to their version

- **Recorded:** a world's file stores `app_version_created` and `app_version_last` in its metadata
  (ADR 0006: the world file holds all data), and the browser's world index (`dwell.worlds`) record
  stores `appVersion`.
- **Saves from before the launcher are not carried over** (owner, 2026-10-05). The first versioned
  release — the **baseline** — starts clean: world files and index records without an app version
  are ignored (the menu offers to delete them, so they do not hold storage quota), and no earlier
  format is migrated. From the baseline on, version locking means **no world ever needs
  migrating**: a world keeps opening in the build that wrote it, so later phases may change the
  world file format, the block registry and the generator freely (upgrading worlds is the
  [`FUTURE.md`](./FUTURE.md) item).
- **Locked:** a build opens a world only if `app_version_created` equals its own version; the
  launcher routes `?play=` there. The menu (in the latest version) lists **all** worlds with a
  version badge; playing one from another version navigates to it through the launcher.
- **Dedicated servers:** `dwell_server` refuses a world created by another version, naming the
  version to run (an override flag is left for the later upgrade work).
- **Shared storage is a cross-version contract.** Every version reads and writes the same
  localStorage index, settings and IndexedDB keys, so their formats become **append-only**: new
  fields optional, and **unknown fields preserved** on rewrite (older builds must not drop a field
  a newer build added). The baseline release must already follow this, or it will strip
  `appVersion` from records it rewrites; a test pins it.
- **Retention:** a stable build that any saved world depends on must stay published. The
  launcher can tell the player when a world's version is no longer available (and offer export,
  Phase 16).

## 7. Multiplayer and the master server

- Friend-world rooms and dedicated-server listings carry the host's **app version** (as well as
  `protocolVersion`), so the server browser and join-by-code open the matching client build
  directly (this replaces the Phase 16 item "the server browser offers the build matching an
  incompatible server").
- The handshake's version check stays; its rejection names the server's app version.
- The master server's own deploy is unchanged (it runs from this repository's CI with its
  secrets).

## 8. Manual setup (owner)

1. Choose the license (§2); the phase adds the files.
2. In the repository settings, allow the release workflow to create releases (Actions' workflow
   permissions: read and write, or `contents: write` in the workflow).
3. Run the first stable release; check `https://dropkickarcade.com/dwell/` loads it through the
   launcher, the main menu works, and new worlds record the version.

## 9. How the phase is checked

- A release produces a tag and a GitHub Release holding the build and `build.json`; the site lists it
  in `versions.json` and serves it at `/dwell/v/<version>/`.
- e2e (against a locally assembled site with two versions): the launcher opens the latest stable;
  `?play=` opens a world in its recorded version; an invite opens the host's version; Back returns
  to `/dwell/`; a world from another version is listed with its badge and opens in its version.
- A world created in version A refuses to open in version B (browser and `dwell_server`), with a
  message naming A.
- Cross-version storage: an older build rewriting the world index preserves fields it does not
  know (a test with a record carrying an extra field).
- Worlds saved before the baseline are ignored without errors and can be deleted from the menu.
- `LICENSE` present and referenced from the package manifests and README; every build carries
  `THIRD_PARTY_NOTICES`, reachable from the menu.
