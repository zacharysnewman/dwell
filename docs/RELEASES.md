# Dwell — Versioned Releases: a Public Deploy Repo, Builds by Tag, Version-Locked Worlds

> **Status: [planned]** — the design for the versioned-releases phase of
> [`IMPLEMENTATION_PLAN.md`](./IMPLEMENTATION_PLAN.md). As it lands, the built parts move into
> [`ARCHITECTURE.md`](./ARCHITECTURE.md) (§2.1 deployment, §3.1 CI, §6.4 persistence, §10.5
> versioning) and an ADR; this file keeps the rationale and the setup steps.

## 1. Goals (owner, 2026-10-05)

1. **Keep the source private.** This repository becomes private; the public only sees built files.
2. **A public deploy repo** serves the web app (GitHub Pages) and holds one **git tag per build**.
   This repository builds and pushes to it; nothing is built in the public repo.
3. **The app loads its versions dynamically from those tags** — every tagged build stays playable,
   even the web app can switch between them.
4. **Worlds record the app version they were created with** and are, for now, **locked** to it:
   a world always opens in its own version. (Upgrading worlds to newer versions is a later option,
   [`FUTURE.md`](./FUTURE.md).)

## 2. Repositories and URLs

| Repository | Visibility | Contents |
|---|---|---|
| Source (this repo, renamed, e.g. `zacharysnewman/dwell-source`) | **Private** | All source, docs, CI, the release workflow, the master server |
| Deploy (`zacharysnewman/dwell`) | **Public** | The launcher, one tagged commit per build, a Pages workflow; no source |

**The URL stays `https://dropkickarcade.com/dwell/`** if the deploy repo takes the name `dwell`
(the user site's custom domain serves each public repo at `/<repo>/`, ADR 0005), so the source
repo is renamed first. Browser storage is per *origin*, not per path, so saved worlds, settings and
the device key carry over either way; keeping the path just keeps links working.

Things to know before switching (§8 has the steps):

- **Actions minutes.** Public repositories get free unlimited GitHub-hosted Actions minutes;
  private ones get a monthly allowance (2,000 minutes on the free plan). Dwell's CI is heavy
  (native and WASM builds, two origins, Playwright e2e, Electron smoke). Options: a paid plan,
  a self-hosted runner, or running the heavy jobs on fewer events (e.g. e2e on PRs to `main` only).
  Measure a month of usage before switching.
- **Pages from a private repo** needs a paid plan — which is why the public deploy repo hosts it.
- **History already published.** The source has been public; making it private hides future work,
  not what was already cloned or forked.
- **Shipped code is still visible.** The web app's JavaScript and WASM are downloadable by
  design. Publish builds **without source maps** (keep them as private CI artifacts for debugging).
- **Renaming then reusing the name** ends GitHub's redirect from the old name, so every remote,
  integration (the Claude GitHub app, Cloudflare deploy hooks, badges) must point at the new names.

## 3. Builds, versions and tags

- **App version:** one semantic version for the whole app — client, sim core, generator, protocol —
  e.g. `0.12.0`. `protocolVersion` and the generator versions stay as internal numbers and are
  recorded with each build. Dev builds are `0.12.0-dev.<yyyymmdd>.<sha7>`.
- **Channels:** **stable** (a release: a `v<version>` tag pushed in the source repo, or a manual
  "Release" workflow) and **dev** (every push to `main`). Both are published; the launcher
  defaults to stable and offers dev in settings.
- **Each build is built for its own path:** Vite `base: '/dwell/v/<version>/'`, the version
  embedded (shown in the HUD and the menu, replacing today's commit SHA overlay).
- **Publishing (source repo CI):** after the build, commit its files to the deploy repo's
  `builds` branch as one commit whose tree is *only that build* plus `build.json` (version,
  channel, date, source commit, `protocolVersion`, generator versions, minimum launcher version),
  and push a tag on it: `v<version>` (stable) or `dev-<version>` (dev). Credentials: a
  fine-grained token or deploy key with write access to the deploy repo only
  (`DEPLOY_REPO_TOKEN`, a secret of the source repo). The tags **are** the builds.
- **The launcher** (a small page, its source in this repo under `launcher/`) is pushed to the
  deploy repo's `main` branch when it changes.

## 4. The deploy repo's site

A small, public workflow in the deploy repo (`pages.yml`, triggered by tag pushes and `main`)
assembles the Pages site **from the tags** and deploys it with `actions/deploy-pages`:

```
/dwell/index.html              the launcher (from main)
/dwell/versions.json           the manifest, generated from the tags' build.json files
/dwell/v/<version>/...         each tag's tree (git archive), immutable
```

- **Which tags:** every stable tag; the newest `DEV_KEEP` (≈ 10) dev tags. Older dev tags may be
  deleted.
- **Size:** a Pages site is limited to 1 GB. At ~5–10 MB per build (measure) that is ~100+ builds.
  When it gets close, drop the oldest stable builds that no world or server uses (§6 counts them
  through the master), or serve old tags from a CDN that reads GitHub tags (jsDelivr) behind a CSP
  exception. Because each tag commit holds only its build, the repo's checked-out tree stays small;
  its history grows by each build's compressed blobs.
- **Caching:** version directories never change; the launcher and the manifest are small and
  short-lived (Pages caches for ~10 minutes).

**Why serve versions from Pages and not straight from a CDN by tag:** the app's CSP is
`script-src 'self'` / `worker-src 'self'`, module workers and WASM need same-origin (or extra
wrapping) to load, and all versions must share the origin's storage (worlds, settings, device
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
  stores `appVersion`. Worlds that exist before the first versioned release are stamped with that
  release's version (the baseline).
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
- The master server's own deploy is unchanged (it runs from the private repo's CI with its
  secrets).

## 8. Manual setup (owner)

1. Rename this repository (e.g. to `dwell-source`); update local remotes, the Claude GitHub app's
   repository access and Claude Code environments, and any Cloudflare/GitHub integrations.
2. Create the public repository `zacharysnewman/dwell` with branches `main` (launcher, Pages
   workflow) and `builds`; set Pages to "GitHub Actions". Add `.nojekyll` (not needed with Actions
   deploys, harmless).
3. Create a fine-grained token (contents: write on the deploy repo only) or a deploy key, and store
   it in the source repo as `DEPLOY_REPO_TOKEN`.
4. Run the first release from the source repo; check `https://dropkickarcade.com/dwell/` loads it
   through the launcher and that existing local worlds appear and play.
5. Disable the source repo's own Pages workflow, then make the source repo **private**.
6. Watch Actions minutes for a month (§2).

## 9. How the phase is checked

- A release from the source repo produces a tag in the deploy repo whose tree is exactly the build
  plus `build.json`; the deploy repo's site lists it in `versions.json` and serves it at
  `/dwell/v/<version>/`.
- e2e (against a locally assembled site with two versions): the launcher opens the latest stable;
  `?play=` opens a world in its recorded version; an invite opens the host's version; Back returns
  to `/dwell/`; a world from another version is listed with its badge and opens in its version.
- A world created in version A refuses to open in version B (browser and `dwell_server`), with a
  message naming A.
- Cross-version storage: an older build rewriting the world index preserves fields it does not
  know (a test with a record carrying an extra field).
- Existing worlds (created before the baseline) are stamped with the baseline version and open.
- No source maps in published builds; the source repo is private and the site still deploys.
