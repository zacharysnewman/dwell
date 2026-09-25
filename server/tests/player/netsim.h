#pragma once

// In-process multiplayer harness for prediction/reconciliation tests (PLAYER_CONTROLLER.md §8,
// §10): the real Server and one Predictor per client, joined by simulated links with latency,
// jitter, and datagram loss. Time advances in whole ticks; clients and server tick together.
#include <Jolt/Jolt.h>

#include <Jolt/Core/JobSystemSingleThreaded.h>
#include <monocypher-ed25519.h>

#include <algorithm>
#include <cmath>
#include <deque>
#include <functional>
#include <map>
#include <memory>
#include <random>
#include <vector>

#include "dwell/core/jolt_runtime.h"
#include "dwell/core/server.h"
#include "dwell/player/net.h"
#include "dwell/player/predictor.h"

namespace dwell::test {

inline int Ticks(float seconds) { return static_cast<int>(std::lround(seconds * 60.0f)); }

struct LinkConditions {
  double rtt_ms = 0;     // round trip; each direction gets half
  double jitter_ms = 0;  // uniform ± jitter per packet
  double loss = 0;       // datagram loss probability (reliable messages are retransmitted)
};

// One direction of a connection.
class Link {
 public:
  struct Packet {
    double at;
    std::uint64_t order;
    bool reliable;
    protocol::Channel channel;
    std::vector<std::uint8_t> bytes;
  };

  Link(LinkConditions c, std::uint32_t seed) : c_(c), rng_(seed) {}

  void Send(double now, bool reliable, protocol::Channel channel, std::vector<std::uint8_t> bytes) {
    std::uniform_real_distribution<double> u(0.0, 1.0);
    const bool lost = u(rng_) < c_.loss;
    if (lost && !reliable) return;
    double at = now + c_.rtt_ms / 2 + (u(rng_) * 2 - 1) * c_.jitter_ms;
    if (reliable) {
      if (lost) at += c_.rtt_ms;          // one retransmission
      at = std::max(at, last_reliable_);  // ordered
      last_reliable_ = at;
    }
    queue_.push_back({at, next_order_++, reliable, channel, std::move(bytes)});
  }

  std::vector<Packet> Deliver(double now) {
    std::vector<Packet> due;
    for (auto it = queue_.begin(); it != queue_.end();) {
      if (it->at <= now) {
        due.push_back(std::move(*it));
        it = queue_.erase(it);
      } else {
        ++it;
      }
    }
    std::sort(due.begin(), due.end(), [](const Packet& a, const Packet& b) {
      return a.at != b.at ? a.at < b.at : a.order < b.order;
    });
    return due;
  }

 private:
  LinkConditions c_;
  std::mt19937 rng_;
  std::deque<Packet> queue_;
  double last_reliable_ = 0;
  std::uint64_t next_order_ = 0;
};

struct SimClient;
using Script = std::function<player::Input(int tick, const SimClient& self)>;

struct SimClient {
  core::SessionId session = 0;
  std::uint16_t player_id = 0;
  core::VoxelWorld world{core::GeneratorFor(core::kGeneratorPlayground)};
  std::unique_ptr<player::Predictor> predictor;
  Link up, down;
  Script script;
  std::uint32_t last_snapshot_tick = 0;
  std::deque<protocol::InputFrame> recent;
  // Metrics.
  std::vector<float> corrections;  // per replay
  std::vector<float> ack_errors;   // per compared snapshot
  std::vector<protocol::PlayerEvent> events;
  std::map<std::uint16_t, protocol::RemotePlayerState> remotes;  // latest view of others
  JPH::Vec3 last_render = JPH::Vec3::sZero();
  float max_render_step = 0;
  std::uint32_t replays_seen = 0;

  SimClient(LinkConditions c, std::uint32_t seed, JPH::JobSystem& jobs,
            const player::PlayerControllerConfig& config)
      : up(c, seed * 2 + 1), down(c, seed * 2 + 2) {
    predictor = std::make_unique<player::Predictor>(world, jobs, config);
  }

  JPH::Vec3 Render() const { return predictor->Position() + predictor->RenderOffset(); }
};

class NetSim {
 public:
  explicit NetSim(LinkConditions conditions, core::ServerConfig config = {})
      : conditions_(conditions), server_(std::move(config), entropy_, jobs_) {}

  SimClient& Join(Script script) {
    const core::SessionId id = static_cast<core::SessionId>(clients_.size() + 1);
    auto client = std::make_unique<SimClient>(conditions_, id, jobs_, server_.player_config());
    client->session = id;
    client->script = std::move(script);
    // Handshake without latency (the link matters for gameplay only).
    core::TransportBinding binding{};
    server_.OnConnected(id, protocol::TransportKind::kWebTransport, binding);
    std::array<std::uint8_t, 32> seed;
    seed.fill(static_cast<std::uint8_t>(id));
    std::array<std::uint8_t, 64> secret;
    protocol::PublicKey key;
    crypto_ed25519_key_pair(secret.data(), key.data(), seed.data());
    Control(id, protocol::ClientHello{protocol::kProtocolVersion, "sim", key, "Sim"});
    protocol::Nonce nonce{};
    for (auto& out : server_.TakeOutbox()) {
      if (auto m = protocol::Decode(out.bytes)) {
        if (auto* c = std::get_if<protocol::Challenge>(&*m)) nonce = c->nonce;
      }
    }
    const auto transcript = protocol::AuthTranscript(nonce, binding, key);
    protocol::Signature sig;
    crypto_ed25519_sign(sig.data(), secret.data(), transcript.data(), transcript.size());
    Control(id, protocol::ClientAuth{sig});
    for (auto& out : server_.TakeOutbox()) {
      if (auto m = protocol::Decode(out.bytes)) {
        if (auto* w = std::get_if<protocol::Welcome>(&*m)) client->player_id = w->player_id;
      }
    }
    clients_.push_back(std::move(client));
    return *clients_.back();
  }

  void Step(int ticks = 1) {
    for (int i = 0; i < ticks; ++i) StepOnce();
  }

  core::Server& server() { return server_; }
  int tick() const { return tick_; }

 private:
  void Control(core::SessionId id, const protocol::Message& m) {
    server_.OnReliable(id, protocol::Channel::kControl, protocol::Encode(m));
  }

  double Now() const { return tick_ * (1000.0 / protocol::kSimHz); }

  void ClientReceive(SimClient& c) {
    for (auto& p : c.down.Deliver(Now())) {
      const auto m = protocol::Decode(p.bytes);
      if (!m) continue;
      if (const auto* snap = std::get_if<protocol::PhysicsSnapshot>(&*m)) {
        c.last_snapshot_tick = std::max(c.last_snapshot_tick, snap->server_tick);
        for (const auto& r : snap->remotes) {
          c.remotes[r.player_id] = r;
          if (r.flags & protocol::PlayerFlags::kDead) {
            c.predictor->RemoveRemote(r.player_id);
          } else {
            // Proxies sit at the latest snapshot position, dead-reckoned between snapshots (a
            // slightly-past proxy corrects far more gently in player-vs-player bumps than one
            // extrapolated to the predicted present; measured).
            const float lead = 0.0f;
            c.predictor->SetRemote(r.player_id,
                                   JPH::Vec3(r.position[0], r.position[1], r.position[2]),
                                   JPH::Vec3(r.velocity[0], r.velocity[1], r.velocity[2]),
                                   (r.flags & protocol::PlayerFlags::kCrouched) != 0, lead);
          }
        }
        const auto replays = c.predictor->stats().replays;
        const auto snapshots = c.predictor->stats().snapshots;
        c.predictor->OnSnapshot(*snap);
        if (c.predictor->stats().snapshots > snapshots) {
          c.ack_errors.push_back(c.predictor->stats().last_error);
        }
        if (c.predictor->stats().replays > replays) {
          c.corrections.push_back(c.predictor->stats().last_correction);
        }
      } else if (const auto* e = std::get_if<protocol::PlayerEvent>(&*m)) {
        c.events.push_back(*e);
        if (e->kind == protocol::PlayerEventKind::kKnockback && e->player_id == c.player_id) {
          c.predictor->OnKnockback(e->input_seq,
                                   JPH::Vec3(e->vector[0], e->vector[1], e->vector[2]));
        }
      }
    }
  }

  void ClientTick(SimClient& c) {
    if (!c.predictor->active()) return;
    const player::Input input = c.script ? c.script(tick_, c) : player::Input{};
    const protocol::InputFrame frame = player::QuantizeInput(input, c.predictor->next_seq());
    c.predictor->Tick(frame);
    c.recent.push_back(frame);
    while (c.recent.size() > protocol::kMaxInputsPerDatagram) c.recent.pop_front();
    protocol::PlayerInput msg;
    msg.last_snapshot_tick = c.last_snapshot_tick;
    msg.inputs.assign(c.recent.begin(), c.recent.end());
    c.up.Send(Now(), false, protocol::Channel::kControl, protocol::Encode(msg));
    const JPH::Vec3 render = c.Render();
    if (c.predictor->stats().ticks > 1) {
      c.max_render_step = std::max(c.max_render_step, (render - c.last_render).Length());
    }
    c.last_render = render;
  }

  void StepOnce() {
    for (auto& c : clients_) ClientReceive(*c);
    for (auto& c : clients_) ClientTick(*c);
    for (auto& c : clients_) {
      for (auto& p : c->up.Deliver(Now())) {
        if (p.reliable) {
          server_.OnReliable(c->session, p.channel, p.bytes);
        } else {
          server_.OnDatagram(c->session, p.bytes);
        }
      }
    }
    server_.Step();
    ++tick_;
    for (auto& out : server_.TakeOutbox()) {
      for (auto& c : clients_) {
        if (c->session != out.session || out.kind == core::Outgoing::Kind::kClose) continue;
        c->down.Send(Now(), out.kind == core::Outgoing::Kind::kReliable, out.channel,
                     std::move(out.bytes));
      }
    }
  }

  class SeqEntropy final : public core::Entropy {
   public:
    void Fill(std::span<std::uint8_t> out) override {
      for (auto& b : out) b = next_++;
    }

   private:
    std::uint8_t next_ = 1;
  };

  LinkConditions conditions_;
  core::JoltRuntime runtime_;
  JPH::JobSystemSingleThreaded jobs_{JPH::cMaxPhysicsJobs};
  SeqEntropy entropy_;
  core::Server server_;
  std::vector<std::unique_ptr<SimClient>> clients_;
  int tick_ = 0;
};

// Steers towards `target` (x, z) at walking pace; stops within 0.3 m.
inline player::Input SteerTo(const SimClient& c, float x, float z, bool run = false) {
  const JPH::Vec3 p = c.predictor->Position();
  const float dx = x - p.GetX(), dz = z - p.GetZ();
  player::Input input;
  if (dx * dx + dz * dz < 0.09f) return input;
  input.look_yaw = std::atan2(dx, dz) * 180.0f / 3.14159265f;
  input.move_y = 1.0f;
  input.run = run;
  return input;
}

}  // namespace dwell::test
