#pragma once

// A Server with test clients that join through the real handshake (streaming and block edit
// tests). Messages are exchanged directly, without links.
#include <doctest/doctest.h>

#include <Jolt/Jolt.h>

#include <Jolt/Core/JobSystemSingleThreaded.h>
#include <monocypher-ed25519.h>

#include <map>

#include "dwell/core/jolt_runtime.h"
#include "dwell/core/server.h"

namespace dwell::test {

using namespace dwell::core;
using namespace dwell::protocol;

class CountingEntropy final : public Entropy {
 public:
  void Fill(std::span<std::uint8_t> out) override {
    for (auto& b : out) b = next_++;
  }

 private:
  std::uint8_t next_ = 1;
};

struct Client {
  std::array<std::uint8_t, 64> secret{};
  PublicKey public_key{};
  explicit Client(std::uint8_t seed_byte) {
    std::array<std::uint8_t, 32> seed;
    seed.fill(seed_byte);
    crypto_ed25519_key_pair(secret.data(), public_key.data(), seed.data());
  }
};

struct Fixture {
  JoltRuntime runtime;
  JPH::JobSystemSingleThreaded jobs{JPH::cMaxPhysicsJobs};
  CountingEntropy entropy;
  Server server;
  TransportBinding binding{};

  explicit Fixture(ServerConfig config) : server(std::move(config), entropy, jobs) {}

  void Send(SessionId id, const Message& m) { server.OnReliable(id, Channel::kControl, Encode(m)); }

  // Reliable messages per session since the last call (outbox drained).
  std::map<SessionId, std::vector<Message>> TakeAll() {
    std::map<SessionId, std::vector<Message>> out;
    for (auto& o : server.TakeOutbox()) {
      if (o.kind != Outgoing::Kind::kReliable) continue;
      auto m = Decode(o.bytes);
      REQUIRE(m.has_value());
      out[o.session].push_back(std::move(*m));
    }
    return out;
  }

  // Messages to `id` since the last call (outbox drained), with their encoded sizes.
  std::vector<std::pair<Message, std::size_t>> Take(SessionId id) {
    std::vector<std::pair<Message, std::size_t>> out;
    for (auto& o : server.TakeOutbox()) {
      if (o.session != id || o.kind != Outgoing::Kind::kReliable) continue;
      auto m = Decode(o.bytes);
      REQUIRE(m.has_value());
      out.emplace_back(std::move(*m), o.bytes.size());
    }
    return out;
  }

  Welcome Join(SessionId id, const Client& who) {
    server.OnConnected(id, TransportKind::kWebTransport, binding);
    Send(id, ClientHello{kProtocolVersion, "test", who.public_key, "Tester"});
    const auto nonce = std::get<Challenge>(Take(id).at(0).first).nonce;
    const auto transcript = AuthTranscript(nonce, binding, who.public_key);
    Signature sig{};
    crypto_ed25519_sign(sig.data(), who.secret.data(), transcript.data(), transcript.size());
    Send(id, ClientAuth{sig});
    return std::get<Welcome>(Take(id).at(0).first);
  }

  // Chunk messages over `ticks` ticks.
  std::vector<ChunkData> Chunks(SessionId id, int ticks,
                                std::vector<ChunkUnload>* unloads = nullptr) {
    std::vector<ChunkData> out;
    for (int i = 0; i < ticks; ++i) {
      server.Step();
      for (auto& [m, size] : Take(id)) {
        if (auto* c = std::get_if<ChunkData>(&m)) out.push_back(std::move(*c));
        if (auto* u = std::get_if<ChunkUnload>(&m); u && unloads) unloads->push_back(*u);
      }
    }
    return out;
  }

  void MoveTo(std::uint16_t player_id, float x, float feet_y, float z) {
    const auto h = *server.PlayerHandleOf(player_id);
    auto& players = server.players();
    players.Teleport(h, JPH::RVec3(x, feet_y + players.HalfHeight(h), z), JPH::Vec3::sZero());
  }
};

inline ServerConfig Flat() {
  return {.generator_version = kGeneratorFlat, .spawn = std::array<double, 3>{0.5, 0.0, 0.5}};
}

inline ChunkCoord CoordOf(const ChunkData& m) { return {m.coord[0], m.coord[1], m.coord[2]}; }

}  // namespace dwell::test
