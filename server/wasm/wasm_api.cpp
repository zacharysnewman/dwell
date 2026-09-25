// C exports for running the server core in the browser (local mode, ARCHITECTURE.md §2.1). The
// JavaScript host (client/src/local/worker.ts) feeds transport events in, advances time, and reads
// the outbox. Single-threaded Jolt (ADR 0007).
#include <Jolt/Jolt.h>

#include <Jolt/Core/JobSystemSingleThreaded.h>
#include <emscripten/emscripten.h>

#include <cstdint>
#include <cstring>
#include <memory>
#include <vector>

#include "dwell/core/entropy.h"
#include "dwell/core/fixed_step.h"
#include "dwell/core/jolt_runtime.h"
#include "dwell/core/server.h"

namespace {

struct LocalServer {
  dwell::core::JoltRuntime jolt;
  JPH::JobSystemSingleThreaded jobs{JPH::cMaxPhysicsJobs};
  dwell::core::SystemEntropy entropy;
  dwell::core::Server server;
  dwell::core::FixedStep fixed{dwell::protocol::kSimHz};
  std::vector<std::uint8_t> outbox;

  explicit LocalServer(dwell::core::ServerConfig config) : server(std::move(config), entropy, jobs) {}
};

std::unique_ptr<LocalServer> g_server;

void Put32(std::vector<std::uint8_t>& out, std::uint32_t v) {
  for (int i = 0; i < 4; ++i) out.push_back(static_cast<std::uint8_t>(v >> (8 * i)));
}

}  // namespace

extern "C" {

// Creates (or recreates) the local server. Returns 1 on success.
EMSCRIPTEN_KEEPALIVE int dwell_local_create(double world_seed) {
  dwell::core::ServerConfig config;
  config.name = "Local world";
  config.max_players = 1;
  config.world_seed = static_cast<std::uint64_t>(world_seed);
  g_server.reset();
  g_server = std::make_unique<LocalServer>(std::move(config));
  return 1;
}

EMSCRIPTEN_KEEPALIVE void dwell_local_connected(std::uint32_t session, std::uint8_t kind,
                                                const std::uint8_t* binding32) {
  dwell::core::TransportBinding binding;
  std::memcpy(binding.data(), binding32, binding.size());
  g_server->server.OnConnected(session, static_cast<dwell::protocol::TransportKind>(kind), binding);
}

EMSCRIPTEN_KEEPALIVE void dwell_local_disconnected(std::uint32_t session) {
  g_server->server.OnDisconnected(session);
}

EMSCRIPTEN_KEEPALIVE void dwell_local_reliable(std::uint32_t session, std::uint8_t channel,
                                               const std::uint8_t* data, std::uint32_t len) {
  g_server->server.OnReliable(session, static_cast<dwell::protocol::Channel>(channel), {data, len});
}

EMSCRIPTEN_KEEPALIVE void dwell_local_datagram(std::uint32_t session, const std::uint8_t* data,
                                               std::uint32_t len) {
  g_server->server.OnDatagram(session, {data, len});
}

// Adds real elapsed time and runs the due simulation steps; returns the current tick.
EMSCRIPTEN_KEEPALIVE std::uint32_t dwell_local_advance(double elapsed_seconds) {
  const int steps = g_server->fixed.Advance(elapsed_seconds);
  for (int i = 0; i < steps; ++i) g_server->server.Step();
  return g_server->server.tick();
}

// Serializes and clears the outbox. Returns a pointer to
//   [u32 count] then per item: [u32 session][u8 kind][u8 channel][u32 len][bytes]
// valid until the next call; `*out_len` receives the byte length.
EMSCRIPTEN_KEEPALIVE const std::uint8_t* dwell_local_take_outbox(std::uint32_t* out_len) {
  auto& out = g_server->outbox;
  out.clear();
  const auto items = g_server->server.TakeOutbox();
  Put32(out, static_cast<std::uint32_t>(items.size()));
  for (const auto& item : items) {
    Put32(out, item.session);
    out.push_back(static_cast<std::uint8_t>(item.kind));
    out.push_back(static_cast<std::uint8_t>(item.channel));
    Put32(out, static_cast<std::uint32_t>(item.bytes.size()));
    out.insert(out.end(), item.bytes.begin(), item.bytes.end());
  }
  *out_len = static_cast<std::uint32_t>(out.size());
  return out.data();
}

}  // extern "C"
