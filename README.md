# dwell

Server-authoritative multiplayer voxel sandbox with large-scale dynamic physics.
Web client: <https://dropkickarcade.com/dwell/>

- [Architecture](docs/ARCHITECTURE.md)
- [Physics player controller](docs/PLAYER_CONTROLLER.md)
- [Implementation plan](docs/IMPLEMENTATION_PLAN.md)
- [Future plans](docs/FUTURE.md)
- [Decision records](docs/adr/README.md)

## Development

### Client (`client/`)

Requires Node 22+. Local mode needs the WASM core, built with Emscripten 6.0.10 (`EMSDK` set, e.g.
`source ~/emsdk/emsdk_env.sh`).

```sh
cd client
npm ci
npm run build:wasm   # server core → public/wasm (local mode)
npm run dev          # http://localhost:5173/dwell/ — no query: local world
npm run lint && npm run typecheck && npm test
npm run build        # outputs dist/
npm run e2e          # Playwright against a native server (build the server first)
```

### Server (`server/`)

Requires CMake ≥ 3.24, Ninja, a C++20 compiler, and rustup (the pinned toolchain in
`rust-toolchain.toml` installs automatically). Dependencies (Jolt, Monocypher, Corrosion, doctest)
are fetched at configure time.

```sh
cd server
cmake --preset dev
cmake --build --preset dev
ctest --preset dev
./build/dev/app/dwell_server --help
./build/dev/app/dwell_server   # prints an invite link to open in the client
```

`--advertise <ip>` sets the address in the invite link (use your LAN or public IP for other
players; the WebRTC fallback needs an IP, not a hostname). WebTransport listens on UDP 4433 and
WebRTC on the next port by default.

After changing the Rust crate's C ABI (`server/net/wt/src/lib.rs`), regenerate the header with
`server/net/wt/gen-header.sh` (needs `cargo install cbindgen --version 0.29.4 --locked`). After
editing `shared/protocol/constants.json`, run `node shared/protocol/gen.mjs` and
`python3 shared/protocol/make_vectors.py`.

### Desktop (`platforms/electron/`)

```sh
cd platforms/electron
npm ci
npm start                                   # local world (uses ../../client/dist)
npx electron . "--join=?join=…&cert=…"      # join a server
```
