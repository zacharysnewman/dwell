# Dwell — Versioned Releases: Builds by Tag, a Version Launcher, Version-Locked Worlds

> **Status: [built, Phase 6]** — the design of the versioned-releases phase of
> [`IMPLEMENTATION_PLAN.md`](./IMPLEMENTATION_PLAN.md), now implemented. What was built is in
> [`ARCHITECTURE.md`](./ARCHITECTURE.md) (§2.1 deployment, §3.1 CI, §6.4 persistence, §10.5
> versioning) and [ADR 0014](./adr/0014-versioned-releases.md); this file keeps the rationale, the
> release procedure and the setup steps. The first stable release, `0.1.0`, was published on 2026-10-06 (§8).

## 1. Goals (owner, 2026-10-05)

1. **Every build stays playable.** Each build is a **git tag** (with a GitHub Release holding the
   built files), and the app — even the web app — **loads its versions dynamically from those
   tags**.
2. **Worlds record the app version that saved them** and are, for now, **locked to its
   compatibility line** (§6): they open in that version or any later compatible one (SemVer,
   §3), never in an incompatible one. Upgrading worlds across breaking changes is a later option
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

**License — all rights reserved** (owner, 2026-10-05; built). `LICENSE` states that the
repository is public for reference only: no right to copy, modify, distribute or reuse it beyond
viewing and forking on GitHub (GitHub's terms), and no restriction on playing the builds the
owner distributes. The package manifests say `"license": "UNLICENSED"` (npm's term for no license
granted) and the Rust crate points `license-file` at `LICENSE`. Source-available licenses
(PolyForm Strict or Noncommercial, the Business Source License) were the alternatives; one can
still be adopted later.

**Third-party notices.** Builds ship code under licenses that require their notices in copies
(Rapier, wgpu, `wtransport`, `redb`, `lz4_flex` and the other Rust crates, and the client shell's npm
packages). **Built:** `shared/licenses/gen.py` writes `THIRD_PARTY_NOTICES` at the root from
`shared/licenses/third_party.json` (the components that are not Rust crates, today the shell's npm
packages, with their license texts in `texts/`) and `cargo metadata` (every crate linked into
`dwell-server` natively and into `dwell-client` for wasm32, its license text read from the crate;
for a choice of licenses the most permissive is used, and the few crates that publish no license
file get the standard MIT text with their authors). (Before Phase 24 the list also held Jolt
Physics, three.js, zstd, SQLite and the Emscripten runtime, removed with the C++ and Three.js
code; ADR 0028.) **Also built:** the file ships in every build (`THIRD_PARTY_NOTICES.txt`, emitted
by the Vite build and linked from the menu's About screen). Beside the native server it is
**[planned, Phase 23]**: the C++ build copied it next to `dwell_server`, and `cargo build` does not
(ARCHITECTURE.md §10). CI runs `gen.py --check`.

## 3. Builds, versions and tags

- **App version: [Semantic Versioning 2.0.0](https://semver.org)** (owner, 2026-10-05) — one
  `MAJOR.MINOR.PATCH` version for the whole app (client, sim core, generator, protocol).
  `protocolVersion` and the generator versions stay as internal numbers, recorded with each build.
  - **The public API** — what compatibility means, as SemVer requires it to be declared: the saved
    world format, the terrain a seed generates, the network protocol (client ↔ server, and the
    master server's API), and the cross-version browser storage contract (§6). Changes that keep
    all four compatible are compatible changes; anything else is a breaking change. A new
    client → server message is compatible only behind a `Welcome` flag (servers reject unknown
    messages; clients ignore unknown flags), so builds of a line still play together
    (ARCHITECTURE.md §10.5; the first is `RegionEdit`).
  - **Block content and compatibility (registry sync, ADR 0038, from 0.14):** adding blocks or
    states (anywhere in the data: ids are mapped by name at join), adding a value to an existing
    property, and changing looks (colour, textures, render mode, cross, wind, fade, grid, fluid looks,
    sound sets) are compatible. Changing an existing state's shape or rules no longer refuses a join
    (the client adopts the server's, after a prompt) but changes what players of other builds see,
    so treat it as breaking for worlds. Still breaking: removing or renaming a state (adding or
    removing a property renames every state of its block), changing the shapes themselves or the
    adaptive shape rules (code both sides run), changing the terrain a seed generates, and changing
    the registry payload's format.
  - **From `1.0.0`** (launch): MAJOR for breaking changes, MINOR for compatible new features, PATCH
    for compatible fixes.
  - **Before `1.0.0`** SemVer allows anything to change; Dwell follows the common `0.y.z` convention
    (npm's and Cargo's): breaking changes bump MINOR (`0.1.0` → `0.2.0`), compatible features and
    fixes bump PATCH (`0.1.0` → `0.1.1`). Most phases ship at least one breaking release.
  - **The baseline — the first versioned release — is `0.1.0`.**
  - **Dev builds** are SemVer pre-releases of the next version, numbered by the CI run, with the
    commit as build metadata: `0.2.0-dev.42+ab12cd3`. They sort correctly
    (`0.2.0-dev.41` < `0.2.0-dev.42` < `0.2.0`), and every identifier is valid (a purely numeric
    commit hash with a leading zero would not be, so the hash only appears as build metadata).
  - **Tags:** `v<version>` without build metadata — `v0.1.0`, `v0.2.0-dev.42`.
  - The version lives in one place (the client's `package.json`), and the build embeds it; the
    other manifests' own version fields are not the app version. **`package.json` is a floor, not a
    counter:** the next release is the next patch after the newest published release, or
    `package.json`'s version if that is higher (nothing is released yet, or a breaking change raised
    it). So nobody bumps it after a release; it is raised by hand only to start a new line
    (`0.1.x` → `0.2.0`), in the PR that makes the breaking change.
- **Channels:** **stable** (a release: pushing a `v<version>` tag, or a manual "Release" workflow)
  and **dev** (every push to `main`). Both are published; the launcher defaults to stable and
  offers dev in settings.
- **Each build is built for its own path:** Vite `base: '/dwell/v/<version>/'`, the version
  embedded (shown in the HUD and the menu, replacing today's commit SHA overlay). Source maps may
  be published, since the source is public.
- **Publishing:** the release workflow creates a **GitHub Release** for the tag — `v0.2.0`
  (stable) or `v0.2.0-dev.42` (a GitHub pre-release) — and attaches the build as an archive plus its
  `build.json` (version, channel, date, commit, `protocolVersion`, generator versions, minimum
  launcher version). The tags and their releases **are** the builds. Keeping builds as release
  assets, not commits, keeps the repository's history free of build output.
- **The launcher** (a small page, source in `client/launcher/`: it shares the client's toolchain,
  tests and the version library, so it lives in that package rather than a folder of its own) is
  versioned separately and deployed with the site. Each build's `build.json` carries `minLauncher`,
  the launcher it needs; the launcher skips builds that ask for a newer one.

### Releasing

The next version is computed by `client/scripts/release.ts` from the published releases and
`client/package.json` (§3): the next patch after the newest release, or `package.json`'s version if
that is higher. Pushes to `main` publish dev builds of it, pre-releases `v<next>-dev.<run>` —
**except** a push that leaves `package.json` above the newest release, which publishes that version
as a stable release: raising it (the baseline `0.1.0`, or a new line like `0.2.0`) is the deliberate
act, so merging it is the release. Patch releases are manual:

1. To release by hand (a patch, or a version not in `package.json`), run the **Release** workflow (Actions → Release → Run workflow, on `main`):
   it releases the next version. To release a different one — a breaking change's `0.2.0` — type it
   in the *version* box (it must be newer than the newest release and not below `package.json`'s).
   Or push the tag yourself: `git tag v<version> && git push origin v<version>` (same rules; the
   workflow stops with a message if they are broken). The `github-pages` environment deploys only
   from `main` (its deployment branch policy), so a run on a tag cannot deploy the site itself: it
   publishes the release and then **dispatches the Pages workflow on `main`**
   (`release.ts siteDeploy`, tested), which assembles the site from every release, the new one
   included. (v0.13.0's tag run called `pages.yml` directly and its deploy was rejected: "Tag
   "v0.13.0" is not allowed to deploy to github-pages due to environment protection rules",
   2026-10-16; the next run from `main` deployed it.)
2. Check `https://dropkickarcade.com/dwell/` (the launcher opens the new stable build; the menu
   shows its version).
3. Nothing to bump afterwards: later dev builds are pre-releases of the next patch.
   Each push to `main` (and each pushed tag) gets its own Release run, never cancelled or replaced
   by a later one (the workflow's concurrency group is per commit, 2026-10-16): merges released
   back to back each publish their own version. (With one group per branch, GitHub kept only the
   newest waiting run, so an earlier merge's release was dropped and had to be tagged by hand.)
   Runs can therefore overlap, so a release is decided twice in `release.ts` (tested): a push
   releases `package.json`'s version only if it **raised** it (its parent's was lower — a later
   push still above the newest release is a dev build), and just before publishing the run checks
   the remote tag again (`publishAction`): already released from this commit, or the tag at another
   commit for a push, and it publishes nothing; a pushed tag or a run by hand naming a version
   tagged elsewhere fails.
3a. **A patch of an older line (a backport)**, while a newer line is out: commit the fix on the
   line's maintenance branch `release/<line>` (e.g. `release/0.3`, branched from the line's newest
   release tag; its `package.json` stays on the line), then push the tag
   (`git tag v0.3.1 && git push origin v0.3.1`) or run the **Release** workflow on that branch (it
   releases the line's next patch, or the version typed, which must be on the line). The version
   must be newer than the line's newest release; the launcher's latest stays the newest version,
   and the line's worlds move onto the patch. `release.ts` refuses another line's version from the
   branch, and an older line's version from `main`. No dev builds are made of maintenance branches
   by a push (one can be made by hand, 3b).
   The run publishes the GitHub Release and, as for a tag, dispatches the Pages workflow on `main`
   (the `github-pages` environment deploys only from `main`); the site is assembled from all
   releases, so that deploy publishes the patch.
3b. **A dev build of any branch, by hand** (an experiment to try out without merging it, as the
   half-height cells were before their adoption): run the **Release** workflow on that branch with *channel* `dev`
   (leave *version* empty; `release.ts` refuses one). It builds `v<next>-dev.<run>` exactly as a
   push to `main` does — a GitHub pre-release created with `--latest=false`, built for
   `/dwell/v/<version>/`, its `build.json` channel `dev` — so `main`, the stable releases and the
   launcher's latest are untouched. Its title and notes name the branch
   (`Dwell 0.18.4-dev.123 (dev, claude/<branch>)`, `release.ts releaseNotes`, tested), and
   the run dispatches the Pages workflow on `main` as a tag run does. The site's assembly reads
   only the releases and their `build.json`, never the branch, so a build whose commit is not on
   `main` is listed with the other dev builds on the version page ("Show dev builds"). It counts
   among the newest `DEV_KEEP` (10) dev builds (§4): later pushes to `main` prune it like any other.
   The channel defaults to `stable`, the run by hand of step 1.
4. A change that breaks the public API (§3) raises `package.json`'s version (to the next MINOR,
   before `1.0.0`) in its PR, so the merge releases that version as stable and worlds of the old
   line stay with the old builds. (Merge the breaking change only when it should ship: there is no
   dev build of the new line first. To try it before shipping, keep `package.json` where it is
   and release the line by hand later.)

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

**The launcher owns the main menu (owner, 2026-10-14; builds from 0.10.3).** A plain visit to
`/dwell/` shows the main menu itself (`src/ui/menuPage.ts`, the same module a build served at
`/dwell/` in development shows), so the menu is the same whichever build plays and changes with
the launcher, not with releases. Every choice is a route the launcher sends to the build it needs:
playing (`?play=`), joining (`?join=`, `?code=`), and — carried out by the build that will own the
world — creating (`?new=1&name=…&seed=…&type=…`, the latest build) and regenerating
(`?regenerate=<id>`, offered only where that build is 0.10.3 or newer). The build makes the
world before its game client starts and replaces the route with `?play=<id>` at once, so a
reload while the game loads continues the world rather than making another. A build opened with no
route hands over to the launcher (`?say=` carries why). The version page's Play pins the menu to
that build for the visit (`/dwell/?version=…`). The server browser pings with the latest build's
networking, loaded from its fixed-name `tools.js`. Older builds (0.1.0, 0.10.0–0.10.2) already
start from these routes and return to `/dwell/` on Quit; their own menus are never shown. No new
infrastructure: everything is served from Pages, same origin.
