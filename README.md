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

Requires Node 22+.

```sh
cd client
npm ci
npm run dev          # http://localhost:5173/dwell/
npm run lint && npm run typecheck && npm test
npm run build        # outputs dist/
```

### Server (`server/`)

Requires CMake ≥ 3.24, Ninja, a C++20 compiler, and rustup (the pinned toolchain in
`rust-toolchain.toml` installs automatically). Dependencies (Jolt, Corrosion, doctest) are
fetched at configure time.

```sh
cd server
cmake --preset dev
cmake --build --preset dev
ctest --preset dev
./build/dev/app/dwell_server
```

After changing the Rust crate's C ABI (`server/net/wt/src/lib.rs`), regenerate the header with
`server/net/wt/gen-header.sh` (needs `cargo install cbindgen --version 0.29.4 --locked`).
