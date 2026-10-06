# 0014. Versioned releases: builds as tagged GitHub Releases behind a launcher, worlds locked to their compatibility line

- Status: Accepted
- Date: 2026-10-06
- Resolves: ARCHITECTURE.md Open Decisions #20 (release pipeline)
- Supersedes: the deployment parts of ARCHITECTURE.md §2.1 (one build deployed on every push to
  `main`) and ADR 0005's note that older builds "are kept at `/dwell/v/<version>/`", which this
  makes concrete

## Context

Until now every push to `main` replaced the one deployed client. Worlds saved by an earlier build
could break under a later one (generator and format changes land phase by phase), and a player
could not join a server that had not updated. The owner's direction (2026-10-05, recorded in
[`RELEASES.md`](../RELEASES.md)): every build stays playable; worlds record the version that saved
them and are locked to its compatibility line for now; the repository stays public under a
restrictive license. The constraint is the same as for the master (ADR 0013): free CI and hosting.

## Options considered

1. **A private source repository and a public deploy repository.** Hides the source, but private
   repositories get a monthly allowance of Actions minutes that Dwell's CI would exceed, Pages from a
   private repository needs a paid plan, the history is already public, and the shipped JavaScript
   and WASM are downloadable anyway. Rejected; a license covers what the split was for.
2. **Serve versions straight from release assets or a CDN by tag.** Release downloads carry no CORS
   headers or script content types, the page's CSP is `script-src 'self'` / `worker-src 'self'`,
   module workers and WASM need the same origin, and every version must share one origin's storage
   (worlds, settings, the device key). Rejected.
3. **Builds as GitHub Releases; one Pages site assembled from them, each version in its own
   directory behind a launcher.** Same origin, same storage, free. Chosen.
4. **Lock worlds to the exact build** instead of a compatibility line. Simple, but every fix
   release would strand its players' worlds. Rejected for compatible releases; kept for dev builds,
   which promise nothing.

## Decision

- **The app version is Semantic Versioning 2.0.0**, one version for the client, sim core,
  generator and protocols, kept in one place, `client/package.json`: the version the *next release*
  will have. The public API that SemVer needs declared is the saved world format, the terrain a seed
  generates, the network protocols (client ↔ server and the master's API) and the cross-version
  browser storage contract. Before `1.0.0`, breaking changes bump MINOR and compatible ones PATCH.
  The baseline release is `0.1.0`. `protocolVersion` and the generator versions stay as internal
  numbers recorded in each build's `build.json`.
- **Channels.** A push to `main` publishes a **dev** build, a pre-release of the next version,
  `v<version>-dev.<run>` (build metadata `+<sha>` is embedded, not in the tag). A pushed
  `v<version>` tag, or the Release workflow run by hand, publishes the **stable** release of
  `package.json`'s version; the tag must match it. `client/scripts/release.ts` decides this and is
  unit tested.
- **Each build is made for its own path**, Vite `base: '/dwell/v/<version>/'`, with the version
  embedded (`DWELL_VERSION`, also read by CMake so the WASM core and the native server agree),
  published as a GitHub Release holding `dwell-<version>.tar.gz` and `build.json`.
- **The site is assembled from the releases** by `pages.yml`, called at the end of each release
  (a workflow's `GITHUB_TOKEN` cannot trigger another) and runnable by hand: every stable release
  and the newest 10 dev builds, a generated `versions.json`, and the launcher at the root. Older dev
  releases and their tags are deleted. The site's size is reported against Pages' 1 GB limit.
- **The launcher** (`client/launcher`, a page of its own, the only unversioned one) reads
  `versions.json` and replaces itself with the build the address needs: a world's (the newest
  published build on its compatibility line, not older than the version that last saved it,
  `?play=`), a host's (the newest on its line, `?v=<host version>` carried by invite and code
  links), or the latest stable (dev if the player chose it in About). Everything the game opens goes
  through `/dwell/`, so Back to the menu and shared links never name a version path.
- **Worlds are locked to their compatibility line.** The world file's `meta` holds
  `app_version_created` and `app_version_last` (written on every save); the browser's world index
  records `appVersion`. A build opens a world only on the same line (MAJOR from `1.0.0`,
  MAJOR.MINOR before, the exact version for a pre-release) and at or after `app_version_last`.
  Saves from before the baseline (no version) are ignored, listed in the menu only to be deleted,
  and never migrated. `dwell_server` and the WASM core refuse a locked file, naming the version to
  run, without touching it.
- **Shared storage is a cross-version contract:** append-only, unknown fields and unreadable
  records preserved on rewrite (tested).
- **Multiplayer:** dedicated servers and friend-world rooms report their app version to the master,
  which passes it along; any build on the host's line can join. The server browser labels hosts on
  another line and joins them through the launcher; join-by-code and the handshake's rejection
  (which now names the server's version) cover the rest. The master's own deploy is unchanged.
- **License:** all rights reserved; `THIRD_PARTY_NOTICES` ships in every build and beside the native
  server, and CI fails when it is stale.

## Consequences

- Every later phase may change the world format, block registry and generator freely: a world only
  ever opens in builds of its own line, so none needs migrating. Upgrading worlds across lines is
  the `FUTURE.md` item.
- The first release must be made by the owner (a tag or the manual workflow, RELEASES.md §8), and
  `client/package.json` must be bumped after each stable release, or later dev builds sort below it
  (the workflow warns).
- A dev build's worlds are playable only while that exact build is still published (the newest 10),
  and old stable builds are never pruned automatically: when the site nears its limit they are
  dropped by hand, keeping the newest build of every line that has worlds.
- Deployment is now a release: the site changes when a release is published or the site workflow is
  run, not on every push alone.
- The launcher is versioned by a single `minLauncher` field in each build's `build.json`; a change
  that a deployed launcher cannot serve bumps it.
- To reverse: serve a single build at `/dwell/` again (the build still works there; the launcher is
  an extra layer, and `go()` keeps working against a one-build site, as in Electron).

## Amendment (2026-10-06): the version follows the newest release

As first decided, `client/package.json` was "the version the next release will have" and had to be
bumped by hand after each release, or later dev builds sorted below it. The first real release run
made the cost clear, so the rule is now: **the next version is the next patch after the newest
published release, or `package.json`'s version if that is higher.** `package.json` is a floor,
raised by hand only to start a new line (a breaking change); the manual Release run can also name a
version. Dev builds are pre-releases of the computed version. The decision is otherwise unchanged;
the consequence about bumping `package.json` after a release no longer applies. Logic and tests:
`client/scripts/release.ts`.
