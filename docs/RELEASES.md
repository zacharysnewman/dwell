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
(Jolt Physics, three.js, `wtransport` and the other Rust crates, zstd, the Emscripten runtime and
others; SQLite is public domain). **Built:** `shared/licenses/gen.py` writes `THIRD_PARTY_NOTICES`
at the root from `shared/licenses/third_party.json` (the CMake, npm and Emscripten components,
with their license texts in `texts/`) and `cargo metadata` (every crate linked into the server,
its license text read from the crate; for a choice of licenses the most permissive is used, and
the few crates that publish no license file get the standard MIT text with their authors).
**Also built:** the file ships in every build (`THIRD_PARTY_NOTICES.txt`, emitted by the Vite build
and linked from the menu's About screen) and beside the native server (copied next to
`dwell_server` after each build and installed with it), and CI runs `gen.py --check`.

## 3. Builds, versions and tags

- **App version: [Semantic Versioning 2.0.0](https://semver.org)** (owner, 2026-10-05) — one
  `MAJOR.MINOR.PATCH` version for the whole app (client, sim core, generator, protocol).
  `protocolVersion` and the generator versions stay as internal numbers, recorded with each build.
  - **The public API** — what compatibility means, as SemVer requires it to be declared: the saved
    world format, the terrain a seed generates, the network protocol (client ↔ server, and the
    master server's API), and the cross-version browser storage contract (§6). Changes that keep
    all four compatible are compatible changes; anything else is a breaking change.
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
  - **Previews** are dev builds of a pull request, `0.2.1-pr.63.9` (PR 63, run 9) with the PR's
    head commit as build metadata: pre-releases of the next version, never releases. They sort
    above `-dev.N` builds, so the launcher and the manifest's `latestDev` skip them by name
    (`isPreviewVersion`): a preview opens only when asked for (`?version=`), or to open a world or
    host it saved itself.
  - **Tags:** `v<version>` without build metadata — `v0.1.0`, `v0.2.0-dev.42`, `v0.2.1-pr.63.9`.
  - The version lives in one place (the client's `package.json`), and the build embeds it; the
    other manifests' own version fields are not the app version. **`package.json` is a floor, not a
    counter:** the next release is the next patch after the newest published release, or
    `package.json`'s version if that is higher (nothing is released yet, or a breaking change raised
    it). So nobody bumps it after a release; it is raised by hand only to start a new line
    (`0.1.x` → `0.2.0`), in the PR that makes the breaking change.
- **Channels:** **stable** (a release: pushing a `v<version>` tag, or a manual "Release" workflow)
  and **dev** (every push to `main`, and a preview of every pull request from this repository).
  Both are published; the launcher defaults to stable and offers dev in settings. A preview is
  built when the PR is opened, pushed to or reopened, in parallel with CI (a newer push cancels the
  older preview's run), and its link is commented on the PR: so a change can be tried before it
  merges. A fork's pull request gets none (it cannot publish).
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
   workflow stops with a message if they are broken).
2. Check `https://dropkickarcade.com/dwell/` (the launcher opens the new stable build; the menu
   shows its version).
3. Nothing to bump afterwards: later dev builds are pre-releases of the next patch.
3a. **A patch of an older line (a backport)**, while a newer line is out: commit the fix on the
   line's maintenance branch `release/<line>` (e.g. `release/0.3`, branched from the line's newest
   release tag; its `package.json` stays on the line), then push the tag
   (`git tag v0.3.1 && git push origin v0.3.1`) or run the **Release** workflow on that branch (it
   releases the line's next patch, or the version typed, which must be on the line). The version
   must be newer than the line's newest release; the launcher's latest stays the newest version,
   and the line's worlds move onto the patch. `release.ts` refuses another line's version from the
   branch, and an older line's version from `main`. No dev builds are made of maintenance branches.
   The run publishes the GitHub Release, but its site update is refused: the `github-pages`
   environment deploys only from `main`. The site is assembled from all releases, so the next
   deploy from `main` publishes the patch — run the **Pages** workflow on `main` (Actions → Pages →
   Run workflow), or let the next release from `main` do it. (Allowing `release/*` in the
   environment's deployment branches would let the backport deploy itself.)
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

- **Which releases:** every stable release; the newest `DEV_KEEP` (≈ 10) dev pre-releases; and
  the newest preview of each **open** pull request, at most `PREVIEW_KEEP` (5) — previews never
  count against `DEV_KEEP`. Older dev pre-releases, superseded previews and those of closed or
  merged PRs, and their tags, are deleted by the workflow (a closed PR's preview goes at the next
  deploy).
- **Manual setup (previews):** the `github-pages` environment must allow deploys from pull
  requests — Settings → Environments → github-pages → deployment branches: add `refs/pull/*/merge`
  (or allow all branches) — as for `release/*` above; until then a preview's release is published
  but its site update is refused.
- **Size:** a Pages site is limited to 1 GB. At ~5–10 MB per build (measure) that is ~100+ builds.
  When it gets close, drop the oldest stable builds that no world or server uses (§6 counts them
  through the master).
- **Caching:** version directories never change; the launcher and the manifest are small and
  short-lived (Pages caches for ~10 minutes). The workflow keeps the unpacked directories in a
  rolling Actions cache, so a deploy downloads only builds new since the last one (the Pages
  artifact is still the whole site: it is replaced, not patched).
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
   - `?play=<world id>` → the newest published build that may open the world (§6);
   - a friend-world code or invite → the newest published build on the host's compatibility line
     (the master reports the host's version, §7);
   - a dedicated server address → the latest stable; if the server is on another compatibility
     line, its `Reject` names its version and the client offers to reopen in a build of that line;
   - otherwise → the latest stable (or dev, if chosen in settings), which shows the main menu.
   - an invite or code link carries `v=<host version>` (`dwell_server` prints it, hosts' share
     links add it; the game also redirects to `?code=…&v=…` after the master reports a host on
     another line), which the launcher routes the same way as the master's version;
   - `?version=<version>` pins a build (developers); the player's channel choice (`dwell.channel`,
     the version page's tick) picks dev instead of stable for the latest.
3. Navigate to `/dwell/v/<version>/` with the same query (minus `version`), by `location.replace`
   (no extra history entry), after checking that the version's `build.json` loads and names it.

**Version page [built, Phase 6b]:** a plain visit to `/dwell/` first shows a launcher screen (**Play Dwell
<latest stable>** and **Choose version**; it waits for a click, no auto-continue), so the choice does not
depend on the build last played; links naming a world, game or build skip it. `/dwell/?versions` lists every published build so players can
choose one themselves; the main menu's **Versions** link opens it, and it holds the "Show dev builds" tick (`dwell.channel`). It lives in the launcher because published
builds are immutable. A choice opens that build for the visit only, so Back to the menu still
returns to the latest. Worlds stay locked to their line (§6), and the page shows which of the
player's worlds each build can open.

**Back to the menu always goes to `/dwell/`**, so a player is never stuck in an old version's
menu. Links shared by players use `/dwell/?…`, never a version path, so they keep working.
Missing or unreachable versions show a clear message with the choice to open the latest.

## 6. Worlds locked to their compatibility line

- **Recorded:** a world's file stores `app_version_created` and `app_version_last` (the version that
  last saved it) in its metadata (ADR 0006: the world file holds all data), and the browser's world
  index (`dwell.worlds`) record stores `appVersion` (= the last).
- **Compatibility line** (owner, 2026-10-05, following SemVer §3): versions that share MAJOR — and,
  before `1.0.0`, MINOR (`0.1.x`) — are compatible: same world format, same generated terrain, same
  protocols. A **pre-release** (dev build) promises no compatibility: its worlds are locked to that
  exact build.
- **Saves from before the launcher are not carried over** (owner, 2026-10-05). The first versioned
  release — the **baseline** — starts clean: world files and index records without an app version
  are ignored (the menu offers to delete them, so they do not hold storage quota), and no earlier
  format is migrated. From the baseline on, locking means **no world ever needs migrating**: a
  world only ever opens in builds of its own compatibility line, so later phases may change the
  world file format, the block registry and the generator freely (upgrading worlds is the
  [`FUTURE.md`](./FUTURE.md) item).
- **Opening:** a build opens a world only if it is on the world's compatibility line **and** its
  version is at least `app_version_last` — never older, because a compatible release may add data
  an older build would not understand (forward compatibility is not promised). The launcher routes
  `?play=` to the newest published build that qualifies, so a world moves onto fixes and compatible
  features automatically. The menu (in the latest version) lists **all** worlds with a version
  badge; playing one from another line navigates to it through the launcher.
- **Dedicated servers:** `dwell_server` refuses a world from another compatibility line, or last
  saved by a newer version, naming the version to run (an override flag is left for the later
  upgrade work).
- **Shared storage is a cross-version contract.** Every version reads and writes the same
  localStorage index, settings and IndexedDB keys, so their formats become **append-only**: new
  fields optional, and **unknown fields preserved** on rewrite (older builds must not drop a field
  a newer build added). The baseline release must already follow this, or it will strip
  `appVersion` from records it rewrites; a test pins it.
- **Retention:** the newest stable build of every compatibility line that saved worlds may use
  must stay published; older patch releases of a line can be pruned (their worlds open in the
  line's newest build). The launcher tells the player when no build of a world's line is
  available (and offers export, Phase 17).

## 7. Multiplayer and the master server

- Friend-world rooms and dedicated-server listings carry the host's **app version** (as well as
  `protocolVersion`). The protocol is part of the public API (§3), so any build on the host's
  compatibility line can join: the server browser marks other lines incompatible, and join-by-code
  opens the newest build of the host's line directly (this replaces the Phase 17 item "the server browser offers the build matching an
  incompatible server").
- The handshake's version check stays; its rejection names the server's app version.
- The master server's own deploy is unchanged (it runs from this repository's CI with its
  secrets).

## 8. Manual setup (owner)

1. ~~Choose the license (§2).~~ Done: all rights reserved (2026-10-05).
2. Nothing to change in the repository settings for the workflows: they declare the permissions they
   need (`contents: write` to create and prune releases, `pages: write` and `id-token: write` to
   deploy). If the repository's default workflow permissions are restricted, that is fine; if
   *organization* policy blocks `contents: write` for workflows, allow it.
3. ~~The first stable release, `0.1.0` (the baseline).~~ Done: published on 2026-10-06 by the push
   that merged #47 (`package.json` said `0.1.0` and nothing was released). Still worth a look: that
   `https://dropkickarcade.com/dwell/` loads it through the launcher, the main menu works, and new
   worlds record the version.
4. Nothing to bump afterwards (**Releasing**, step 3).

## 9. How the phase is checked

Automated in CI unless noted: `shared/version/vectors.txt` through both implementations (TypeScript
and C++, native and WASM); `dwell_version_lock` (the real `dwell_server` on world files saved by other
versions); the world index contract tests; `client/scripts/*.test.ts` (which releases the site holds,
the manifest, what a release run builds); the launcher's unit tests; and `npm run e2e:site` (below).
Not automated: the workflows themselves; their first real runs were the merges of #46 and #47 (§8).

- A release produces a tag and a GitHub Release holding the build and `build.json`; the site lists it
  in `versions.json` and serves it at `/dwell/v/<version>/`.
- e2e (against a locally assembled site with two versions): the launcher opens the latest stable;
  `?play=` opens a world in the newest build of its line; an invite opens a build of the host's
  line; Back returns to `/dwell/`; a world from another line is listed with its badge and opens in
  its line.
- Compatibility (with `0.1.0`, `0.1.1`, `0.2.0` and `0.2.0-dev.1` builds): a world saved by `0.1.0`
  opens in `0.1.1`; one saved by `0.1.1` does not open in `0.1.0`; neither opens in `0.2.0`; a
  dev build's world opens only in that dev build — in the browser and in `dwell_server`, with a
  message naming the version to use.
- Cross-version storage: an older build rewriting the world index preserves fields it does not
  know (a test with a record carrying an extra field).
- Worlds saved before the baseline are ignored without errors and can be deleted from the menu.
- `LICENSE` present and referenced from the package manifests and README; every build carries
  `THIRD_PARTY_NOTICES`, reachable from the menu.
