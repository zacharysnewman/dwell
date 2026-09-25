// C exports of the sim core for browsers (ADR 0007: single-threaded Jolt). Two independent uses,
// each in its own module instance:
//  - dwell_local_*: the authoritative server for local mode (ARCHITECTURE.md §2.1), hosted in a
//    worker by client/src/local/worker.ts;
//  - dwell_client_*: the client's own sim (client/src/sim/clientCore.ts) on the main thread:
//    prediction and reconciliation of the local player, remote-player proxies, and the terrain
//    faces the renderer draws (PLAYER_CONTROLLER.md §8).
#include <Jolt/Jolt.h>

#include <Jolt/Core/JobSystemSingleThreaded.h>
#include <emscripten/emscripten.h>

#include <array>
#include <cstdint>
#include <cstring>
#include <memory>
#include <vector>

#include "dwell/core/entropy.h"
#include "dwell/core/fixed_step.h"
#include "dwell/core/jolt_runtime.h"
#include "dwell/core/server.h"
#include "dwell/core/terrain_collision.h"
#include "dwell/player/net.h"
#include "dwell/player/predictor.h"

namespace {

// Jolt's process-wide setup, once per module instance.
void EnsureJolt() { static dwell::core::JoltRuntime runtime; }

struct LocalServer {
  int jolt = (EnsureJolt(), 0);
  JPH::JobSystemSingleThreaded jobs{JPH::cMaxPhysicsJobs};
  dwell::core::SystemEntropy entropy;
  dwell::core::Server server;
  dwell::core::FixedStep fixed{dwell::protocol::kSimHz};
  std::vector<std::uint8_t> outbox;

  explicit LocalServer(dwell::core::ServerConfig config)
      : server(std::move(config), entropy, jobs) {}
};

std::unique_ptr<LocalServer> g_server;

struct ClientSim {
  int jolt = (EnsureJolt(), 0);
  JPH::JobSystemSingleThreaded jobs{JPH::cMaxPhysicsJobs};
  dwell::core::VoxelWorld world;
  dwell::player::PlayerControllerConfig config;
  dwell::player::Predictor predictor;
  std::uint32_t events = 0;  // controller events since the last state read
  float landed_speed = 0;
  std::array<float, 64> state{};
  std::vector<dwell::core::RenderFace> faces;

  ClientSim(std::uint32_t generator_version, std::uint64_t world_seed)
      : world(dwell::core::GeneratorFor(generator_version, world_seed)),
        predictor(world, jobs, config) {}
};

std::unique_ptr<ClientSim> g_client;

void Put32(std::vector<std::uint8_t>& out, std::uint32_t v) {
  for (int i = 0; i < 4; ++i) out.push_back(static_cast<std::uint8_t>(v >> (8 * i)));
}

}  // namespace

extern "C" {

// Creates (or recreates) the local server. Returns 1 on success.
EMSCRIPTEN_KEEPALIVE int dwell_local_create(double world_seed, std::uint32_t generator_version) {
  dwell::core::ServerConfig config;
  config.name = "Local world";
  config.max_players = 1;
  config.world_seed = static_cast<std::uint64_t>(world_seed);
  config.generator_version = generator_version;
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

// --- client sim ------------------------------------------------------------------------------

// Creates (or recreates) the client sim for a world (generator version and u64 seed, from
// Welcome). Returns 1 on success.
EMSCRIPTEN_KEEPALIVE int dwell_client_create(std::uint32_t generator_version, std::uint32_t seed_lo,
                                             std::uint32_t seed_hi) {
  g_client.reset();
  g_client = std::make_unique<ClientSim>(generator_version,
                                         (static_cast<std::uint64_t>(seed_hi) << 32) | seed_lo);
  return 1;
}

EMSCRIPTEN_KEEPALIVE std::uint32_t dwell_client_next_seq() {
  return g_client->predictor.next_seq();
}

// Predicts one tick with a quantized input (see client/src/predict/input.ts).
EMSCRIPTEN_KEEPALIVE void dwell_client_tick(std::uint32_t seq, int move_x, int move_y,
                                            std::uint32_t buttons, int yaw, int pitch) {
  dwell::protocol::InputFrame f;
  f.seq = seq;
  f.move_x = static_cast<std::int8_t>(move_x);
  f.move_y = static_cast<std::int8_t>(move_y);
  f.buttons = static_cast<std::uint16_t>(buttons);
  f.yaw = static_cast<std::int16_t>(yaw);
  f.pitch = static_cast<std::int16_t>(pitch);
  g_client->predictor.Tick(f);
  const auto& c = g_client->predictor.controller();
  g_client->events |= c.events;
  if (c.events & dwell::player::Events::kLanded) g_client->landed_speed = c.landed_speed;
}

// A PhysicsSnapshot datagram for this client (raw bytes). Returns 0 if it doesn't decode.
EMSCRIPTEN_KEEPALIVE int dwell_client_snapshot(const std::uint8_t* data, std::uint32_t len) {
  const auto m = dwell::protocol::Decode({data, len});
  const auto* snap = m ? std::get_if<dwell::protocol::PhysicsSnapshot>(&*m) : nullptr;
  if (!snap) return 0;
  g_client->predictor.OnSnapshot(*snap);
  return 1;
}

EMSCRIPTEN_KEEPALIVE void dwell_client_knockback(std::uint32_t input_seq, float x, float y,
                                                 float z) {
  g_client->predictor.OnKnockback(input_seq, JPH::Vec3(x, y, z));
}

EMSCRIPTEN_KEEPALIVE void dwell_client_set_remote(std::uint32_t player_id, float x, float y,
                                                  float z, float vx, float vy, float vz,
                                                  int crouched, float lead_seconds) {
  g_client->predictor.SetRemote(static_cast<std::uint16_t>(player_id), JPH::Vec3(x, y, z),
                                JPH::Vec3(vx, vy, vz), crouched != 0, lead_seconds);
}

EMSCRIPTEN_KEEPALIVE void dwell_client_remove_remote(std::uint32_t player_id) {
  g_client->predictor.RemoveRemote(static_cast<std::uint16_t>(player_id));
}

// Fills and returns the 64-float state block (layout in client/src/sim/clientCore.ts); events
// accumulated since the previous call are reported once.
EMSCRIPTEN_KEEPALIVE const float* dwell_client_state() {
  ClientSim& sim = *g_client;
  const auto& p = sim.predictor;
  const auto& c = p.controller();
  const auto& s = p.stats();
  auto& o = sim.state;
  o.fill(0);
  auto put3 = [&](int at, JPH::Vec3 v) {
    o[at] = v.GetX();
    o[at + 1] = v.GetY();
    o[at + 2] = v.GetZ();
  };
  o[0] = p.active() ? 1.0f : 0.0f;
  put3(1, p.Position());
  put3(4, p.RenderOffset());
  put3(7, p.Velocity());
  o[10] = p.HalfHeight();
  o[11] = static_cast<float>(c.state);
  o[12] = static_cast<float>(dwell::player::ToNet(c, {}).flags);
  o[13] = static_cast<float>(sim.events);
  o[14] = sim.landed_speed;
  o[15] = c.platform.yaw_delta;
  put3(16, c.ground.normal);
  o[19] = static_cast<float>(c.ground.ground.kind);
  o[20] = c.ground.gap;
  o[21] = c.ground.ceiling_blocked ? 1.0f : 0.0f;
  o[22] = c.ground.touching_wall ? 1.0f : 0.0f;
  put3(23, c.horizontal.current);
  put3(26, c.horizontal.external);
  put3(29, c.target_velocity);
  o[32] = c.swim.submerged;
  o[33] = static_cast<float>(s.ticks);
  o[34] = static_cast<float>(s.snapshots);
  o[35] = static_cast<float>(s.replays);
  o[36] = static_cast<float>(s.snaps);
  o[37] = static_cast<float>(s.knockback_replays);
  o[38] = s.last_correction;
  o[39] = s.last_error;
  o[40] = static_cast<float>(p.next_seq());
  o[41] = static_cast<float>(s.resets);
  o[42] = p.config().body.eye_height;
  o[43] = p.config().body.crouch_eye_height;
  o[44] = p.config().movement.max_step_height;
  o[45] = p.config().body.radius;
  o[46] = p.config().body.standing_height;
  sim.events = 0;
  sim.landed_speed = 0;
  return o.data();
}

// Visible faces of a chunk for rendering: returns a pointer to `*out_count` 8-byte RenderFaces
// (x, y, z, face, u16 material, u16 reserved), valid until the next call.
EMSCRIPTEN_KEEPALIVE const std::uint8_t* dwell_client_chunk_faces(int cx, int cy, int cz,
                                                                  std::uint32_t* out_count) {
  g_client->faces = dwell::core::BuildRenderFaces(g_client->world, {cx, cy, cz});
  *out_count = static_cast<std::uint32_t>(g_client->faces.size());
  return reinterpret_cast<const std::uint8_t*>(g_client->faces.data());
}

}  // extern "C"
