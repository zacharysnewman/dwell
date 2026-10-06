# dwell

Server-authoritative multiplayer voxel sandbox with large-scale dynamic physics.
Web client: <https://dropkickarcade.com/dwell/>

- [Architecture](docs/ARCHITECTURE.md)
- [Physics player controller](docs/PLAYER_CONTROLLER.md)
- [Implementation plan](docs/IMPLEMENTATION_PLAN.md)
- [Future plans](docs/FUTURE.md)
- [Decision records](docs/adr/README.md)
- [License](LICENSE) — all rights reserved; public for reference only ([third-party notices](THIRD_PARTY_NOTICES))

## Development

### Client (`client/`)

Requires Node 22+. Local mode needs the WASM core, built with Emscripten 6.0.10 (`EMSDK` set, e.g.
`source ~/emsdk/emsdk_env.sh`).

```sh
cd client
npm ci
npm run build:wasm   # server core → public/wasm (local mode)
npm run dev          # http://localhost:5173/dwell/ — the main menu (?local=1: straight into a local world)
npm run lint && npm run typecheck && npm test
npm run build        # outputs dist/
npm run e2e          # Playwright against a native server and a local master (build the server
                     # first; `npm ci` in services/master)
npm run e2e:site     # the launcher against a locally assembled three-version site (no server needed)
```

**Versions and releases** ([`docs/RELEASES.md`](docs/RELEASES.md)): the app version is Semantic
Versioning; the next version follows the newest release (`client/package.json` is a floor, raised
only to start a new line). A build for
another version: `DWELL_VERSION=0.2.0 npm run build` (served at `/dwell/v/0.2.0/`); the native
server and the WASM core read the same version from `DWELL_VERSION` or `package.json` when CMake
configures. Pushes to `main` publish dev builds as GitHub Releases; to release, run the Release
workflow (optionally naming a version) or push a `v<version>` tag; merging a PR that raises
`package.json` above the newest release releases that version. Local builds are `<version>-dev.local`
and serve at `/dwell/` with no launcher; worlds are locked to their version's compatibility line.

**Playing:** the main menu lists your worlds (create one with a name and seed; regenerate or delete it) and joins a friend's world by its code (e.g. `KQ7-XM4`) or a server from a pasted invite link. In a game, Esc (or ☰) opens the game menu: Resume, Host… (in your own worlds: choose how many guests and who may build and fly, then share the code, link or QR code; keep the page open while hosting), Quit to main menu, and the settings. Click the view to capture the mouse. WASD move, Space jump, Shift run, C (or Ctrl)
crouch, double-tap Space to fly (creative flight: Space/C up and down, Shift faster, the higher the
faster — high enough to see the whole disc; while flying, the Flight speed slider (top right) or − / = sets a minimum speed; servers choose who may with `--flight`), F3 debug
overlay (`?debug=1` opens it on load), F4 terrain map. The
whole-world view (LOD) is on by default; `?lodcolors=1` tints it by level, `?lod=0` turns it off. `?netsim=150,20,5` simulates 150 ms RTT,
20 ms jitter and 5 % loss.

**Touch (phones, tablets; play in landscape):** drag on the left half for a floating joystick
(push past the ring to run), drag on the right half to look, and use the Jump and Crouch (hold)
and Run (toggle) buttons; Fly toggles creative flight and the ⓘ button (top right) the debug
overlay. On iOS, *Share → Add to Home Screen* runs it full screen. Safari has no
WebTransport, so use local mode or a WebRTC invite there.
The default world is procedural terrain (oceans, beaches, plains, forests, deserts, snowy land,
mountains, caves); `?seed=N` picks the seed. `?world=playground` loads the movement playground
instead (slab stairs, a block step, a doorway, a crawlspace, a ladder, a pool, and an orange launch
pad just behind the spawn), and `?world=flat` a flat world. Terrain streams in around the player;
the client generates untouched chunks itself in worker threads, and `?chunks=full` asks the server
to send every chunk instead (full-chunk mode).

### Server (`server/`)

Requires CMake ≥ 3.24, Ninja, a C++20 compiler, and rustup (the pinned toolchain in
`rust-toolchain.toml` installs automatically). Dependencies (Jolt, Monocypher, Corrosion, doctest)
are fetched at configure time.

```sh
cd server
cmake --preset dev
cmake --build --preset dev
ctest --preset dev
DWELL_UPDATE_GOLDEN=1 ./build/dev/tests/dwell_tests -ts="player: scenario"  # after intended controller changes
DWELL_UPDATE_GOLDEN=1 ./build/dev/tests/dwell_tests -ts="worldgen: golden"   # after a generator version bump
./build/dev/app/dwell_server --help
./build/dev/app/dwell_server   # prints an invite link, and its join code once registered
./build/dev/tools/dwell_worldgen_inspect 0 0 0 32          # ASCII biome map of seed 0 (32 m per character)
./build/dev/tools/dwell_worldgen_inspect 0 0 0 1 slice     # vertical section through the origin
```

`--seed N` and `--generator N` (2 = procedural terrain, the default; 1 = movement playground;
0 = flat) choose the world.

The server registers with the master server (unlisted by default), so players can join it by its
**join code**, by typing its **address** in the Join box (e.g. `192.168.1.50` on the same
network), or pick it under **On your network**. `--visibility public` also lists it in the
server browser (`--tags pve,creative` to be found by search); `--visibility none` keeps it off the master;
`--master <url>` points at another master (e.g. `http://localhost:8787` for a local `wrangler
dev`). `--advertise <ip>` sets the address in the invite link and the one the master hands out
(use your public IP or host name for players elsewhere; the WebRTC fallback needs an IP, not a
hostname). WebTransport listens on UDP 4433 and WebRTC on the next port by default; forward both
for players outside your network.

After changing the Rust crate's C ABI (`server/net/wt/src/lib.rs`), regenerate the header with
`server/net/wt/gen-header.sh` (needs `cargo install cbindgen --version 0.29.4 --locked`). After
editing `shared/protocol/constants.json`, run `node shared/protocol/gen.mjs` and
`python3 shared/protocol/make_vectors.py`.

After adding, removing or upgrading a dependency that ships in a build (a CMake library, a client
runtime `dependency`, a Rust crate, or the Emscripten version), update
`shared/licenses/third_party.json` (and its `texts/`) if it is not a crate, then run
`python3 shared/licenses/gen.py` (needs `cargo`) to regenerate `THIRD_PARTY_NOTICES`;
`--check` fails if it is out of date.

### Desktop (`platforms/electron/`)

```sh
cd platforms/electron
npm ci
npm start                                   # local world (uses ../../client/dist)
npx electron . "--join=?join=…&cert=…"      # join a server
```

## License

Copyright (c) 2026 Zachary Newman. **All rights reserved.** This repository is public for
reference only; it is not open source, and no license is granted to copy, modify, distribute or
reuse it — see [LICENSE](LICENSE). Third-party components keep their own licenses; see
[THIRD_PARTY_NOTICES](THIRD_PARTY_NOTICES).
