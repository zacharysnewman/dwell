# 0001. WebTransport server library: Rust `wtransport` behind a C ABI

- Status: Accepted
- Date: 2026-09-25
- Resolves: ARCHITECTURE.md Open Decisions #1

## Context

The authoritative server is C++20 (Jolt Physics). Clients connect over WebTransport
(HTTP/3 over QUIC): reliable bidirectional/unidirectional streams plus unreliable datagrams
(ARCHITECTURE.md §8). Local development also needs short-lived self-signed certificates that
browsers accept through `serverCertificateHashes`.

The C/C++ ecosystem has mature QUIC stacks, but few provide a complete WebTransport server
layer (extended CONNECT, session management, HTTP/3 datagrams, WebTransport streams).

## Options considered

1. **Rust `wtransport` behind a C ABI.** A mature, pure-Rust WebTransport implementation (on
   `quinn`), built as a static library and linked into the C++ server. Full WebTransport today;
   certificate helpers for development; simple `cargo` build pulled into CMake through Corrosion.
   Cost: a Rust toolchain in the server build and a thin FFI layer to maintain.
2. **Google QUICHE (C++).** Chromium's stack, with a WebTransport server. Pure C++ and closest
   to browser behavior, but Bazel-built, difficult to embed in CMake, and little documented for
   use outside Chromium.
3. **msquic / lsquic / ngtcp2 + nghttp3 (C).** Easy to embed and well maintained, but
   WebTransport support is missing or incomplete; we would likely build the session layer.
4. **Separate gateway process** (Go `webtransport-go` or Rust). Keeps the C++ build isolated but
   adds a process and a local hop on every packet.

## Decision

Use **Rust `wtransport`**, wrapped in a small Rust crate (`server/net/wt`) that exposes a narrow
**C ABI**, statically linked into the C++ server's network front-end (`server/net`).

- The C ABI is message-oriented and owns no game logic: listen / accept session / close;
  open, accept, write, and read streams; send and receive datagrams; certificate generation and
  hash reporting for development. Events are delivered to C++ through a polled queue drained
  once per server tick, so the Rust async runtime (tokio) never calls into the simulation.
- Byte buffers cross the boundary as `(ptr, len)` with explicit ownership: Rust-allocated
  buffers are freed through an ABI function, never by C++ `free`.
- The C header is generated with `cbindgen` and checked in; CI fails if it is stale.
- `server/core` is unaffected: it stays pure C++ and still compiles to WASM. `server/net` is
  native-only.

## Consequences

- **Build:** the server requires a stable Rust toolchain (pinned via `rust-toolchain.toml`) in
  addition to CMake/C++. Corrosion integrates the crate into the CMake build; CI caches cargo.
- **Threading:** tokio runs on its own worker threads inside the server process; the main loop
  only exchanges messages through lock-free queues at tick boundaries.
- **Fallback transport:** superseded by ADR 0008 — no WebSocket; dedicated servers also accept
  WebRTC (`str0m`) in the same crate, behind the same C ABI.
- **Verification:** Phase 1 starts with a spike connecting Chrome (dev certificate via
  `serverCertificateHashes`) and Electron, exchanging a reliable stream message and a datagram.
- **Reversal:** everything above the C ABI is transport-agnostic. If the spike or later work
  fails, Google QUICHE (option 2) can replace the crate behind the same interface without
  touching `server/core` or the protocol.
