// Networked players (Phase 2h): server input handling, snapshots, prediction and reconciliation
// under simulated latency, jitter and loss, knockback replay, health, death and respawn.
#include <doctest/doctest.h>

#include <Jolt/Jolt.h>

#include <algorithm>
#include <numeric>

#include "netsim.h"

using namespace dwell;
using namespace dwell::test;
using protocol::PlayerEventKind;

namespace {

// A varied movement script on open ground (walk, run, strafe, turn, jump, crouch).
player::Input Wander(int tick, int phase_offset) {
  player::Input i;
  const int phase = (tick / 90 + phase_offset) % 6;
  i.look_yaw = static_cast<float>((tick * 2 + phase_offset * 60) % 360);
  switch (phase) {
    case 0:
      i.move_y = 1;
      break;
    case 1:
      i.move_y = 1;
      i.run = true;
      break;
    case 2:
      i.move_x = 1;
      i.move_y = 0.5f;
      i.jump = tick % 45 < 2;
      break;
    case 3:
      i.move_y = -1;
      i.crouch = true;
      break;
    case 4:
      i.move_x = -1;
      break;
    default:
      break;
  }
  return i;
}

float Percentile(std::vector<float> v, float p) {
  if (v.empty()) return 0;
  std::sort(v.begin(), v.end());
  return v[static_cast<std::size_t>(p * static_cast<float>(v.size() - 1))];
}

// Keeps a wandering player inside a box on open ground (flat world away from the playground).
Script Roam(float cx, float cz, int offset) {
  return [cx, cz, offset](int tick, const SimClient& self) {
    const JPH::Vec3 p = self.predictor->Position();
    if (std::abs(p.GetX() - cx) > 6 || std::abs(p.GetZ() - cz) > 6) return SteerTo(self, cx, cz);
    return Wander(tick, offset);
  };
}

}  // namespace

TEST_SUITE("netcode: server") {
  TEST_CASE("joining spawns a player and snapshots start at SNAPSHOT_HZ") {
    NetSim sim({});
    auto& c = sim.Join({});
    CHECK(sim.server().PlayerHandleOf(c.player_id).has_value());
    sim.Step(10);
    CHECK(c.predictor->active());
    CHECK(c.predictor->stats().resets == 1);
    CHECK(sim.server().HealthOf(c.player_id) == protocol::kMaxHealth);
  }

  TEST_CASE("out-of-range inputs are rejected and never simulated") {
    NetSim sim({});
    auto& c = sim.Join({});
    sim.Step(5);
    const auto before = *sim.server().StatsOf(c.player_id);
    const std::uint32_t seq = c.predictor->next_seq() + 10;
    protocol::PlayerInput bad;
    bad.inputs = {{seq, 127, 127, 0, 0, 0}};  // |move| = √2
    sim.server().OnDatagram(c.session, protocol::Encode(bad));
    protocol::PlayerInput bad_pitch;
    bad_pitch.inputs = {{seq + 1, 0, 0, 0, 0, -32768}};
    sim.server().OnDatagram(c.session, protocol::Encode(bad_pitch));
    protocol::PlayerInput far_ahead;
    far_ahead.inputs = {{seq + 100000, 0, 127, 0, 0, 0}};
    sim.server().OnDatagram(c.session, protocol::Encode(far_ahead));
    const auto after = *sim.server().StatsOf(c.player_id);
    CHECK(after.inputs_rejected - before.inputs_rejected == 3);
    CHECK(after.inputs_received == before.inputs_received);
    // A malformed datagram (unknown button bit) doesn't even decode, and is dropped.
    std::vector<std::uint8_t> bytes =
        protocol::Encode(protocol::PlayerInput{0, {{seq + 2, 0, 0, 0, 0, 0}}});
    bytes[10] = 0x80;
    sim.server().OnDatagram(c.session, bytes);
    CHECK(sim.server().StatsOf(c.player_id)->inputs_received == before.inputs_received);
    // A diagonal of full axes is clamped to the unit circle by the client's quantizer.
    player::Input diagonal;
    diagonal.move_x = diagonal.move_y = 1;
    const auto q = player::QuantizeInput(diagonal, 1);
    CHECK(q.move_x * q.move_x + q.move_y * q.move_y <= 127 * 127 + 254);
  }

  TEST_CASE("input datagrams are rate limited") {
    NetSim sim({});
    auto& c = sim.Join({});
    for (std::uint32_t i = 1; i <= 300; ++i) {
      protocol::PlayerInput m;
      m.inputs = {{i, 0, 0, 0, 0, 0}};
      sim.server().OnDatagram(c.session, protocol::Encode(m));
    }
    const auto stats = *sim.server().StatsOf(c.player_id);
    CHECK(stats.datagrams_dropped == 300 - 2 * protocol::kSimHz);  // 120 per second pass
  }

  TEST_CASE(
      "a hard fall damages the player; a lethal one kills, and every client sees the respawn") {
    NetSim sim({});
    auto& a = sim.Join({});
    auto& b = sim.Join({});
    sim.Step(10);
    // Drop player A from 5 m (lands at ~14 m/s: damaged) and later from 30 m (lethal).
    auto& players = sim.server().players();
    const auto ha = *sim.server().PlayerHandleOf(a.player_id);
    players.Teleport(ha, JPH::Vec3(8.5f, 5.9f, -8.5f), JPH::Vec3::sZero());
    sim.Step(90);
    const int health = sim.server().HealthOf(a.player_id);
    CHECK(health < protocol::kMaxHealth);
    CHECK(health > 0);
    players.Teleport(*sim.server().PlayerHandleOf(a.player_id), JPH::Vec3(8.5f, 30.9f, -8.5f),
                     JPH::Vec3::sZero());
    sim.Step(150);
    CHECK(sim.server().HealthOf(a.player_id) == 0);
    CHECK_FALSE(sim.server().PlayerHandleOf(a.player_id).has_value());
    CHECK_FALSE(a.predictor->active());  // the dead player's own client stops predicting
    auto count = [](const SimClient& c, PlayerEventKind kind) {
      return std::count_if(c.events.begin(), c.events.end(),
                           [kind](auto& e) { return e.kind == kind; });
    };
    for (const SimClient* c : {&a, &b}) {
      CHECK(count(*c, PlayerEventKind::kDamage) == 2);
      CHECK(count(*c, PlayerEventKind::kDeath) == 1);
    }
    CHECK((b.remotes.at(a.player_id).flags & protocol::PlayerFlags::kDead) != 0);
    sim.Step(protocol::kRespawnSeconds * protocol::kSimHz + 30);
    CHECK(sim.server().HealthOf(a.player_id) == protocol::kMaxHealth);
    CHECK(a.predictor->active());
    for (const SimClient* c : {&a, &b}) CHECK(count(*c, PlayerEventKind::kRespawn) == 1);
    CHECK((b.remotes.at(a.player_id).flags & protocol::PlayerFlags::kDead) == 0);
  }

  TEST_CASE("players block each other on the server") {
    NetSim sim({});
    auto& a = sim.Join([](int, const SimClient& self) { return SteerTo(self, 8.5f, -8.5f); });
    auto& b = sim.Join([](int, const SimClient& self) { return SteerTo(self, 8.5f, -8.5f); });
    sim.Step(Ticks(6.0f));
    auto& players = sim.server().players();
    const JPH::Vec3 pa = players.Position(*sim.server().PlayerHandleOf(a.player_id));
    const JPH::Vec3 pb = players.Position(*sim.server().PlayerHandleOf(b.player_id));
    CHECK((pa - pb).Length() > 0.55f);  // two 0.3 m capsules can't overlap
  }
}

TEST_SUITE("netcode: prediction") {
  TEST_CASE("without latency, prediction matches the server and never replays") {
    NetSim sim({});
    auto& c = sim.Join(Roam(8.5f, -8.5f, 0));
    sim.Step(Ticks(20.0f));
    CHECK(c.predictor->stats().snapshots > 300);
    CHECK(c.predictor->stats().replays <= 2);
    CHECK(Percentile(c.ack_errors, 1.0f) < 0.001f);
  }

  TEST_CASE(
      "two clients at 150 ms RTT, 20 ms jitter, 5 % loss: immediate, small corrections, few "
      "replays") {
    NetSim sim({150, 20, 0.05});
    auto& a = sim.Join(Roam(9.0f, -9.0f, 0));
    auto& b = sim.Join(Roam(-9.0f, -12.0f, 3));
    sim.Step(Ticks(3.0f));  // settle the input buffers
    for (SimClient* c : {&a, &b}) {
      c->corrections.clear();
      c->ack_errors.clear();
      CHECK(c->predictor->stats().snaps == 0);
    }
    const auto replays_before = a.predictor->stats().replays;
    const auto snapshots_before = a.predictor->stats().snapshots;
    sim.Step(Ticks(60.0f));
    for (SimClient* c : {&a, &b}) {
      const auto& s = c->predictor->stats();
      MESSAGE("client " << c->player_id << ": " << s.snapshots << " snapshots, " << s.replays
                        << " replays, error at ack p95 " << Percentile(c->ack_errors, 0.95f)
                        << " m, p99 " << Percentile(c->ack_errors, 0.99f) << " m; corrections max "
                        << Percentile(c->corrections, 1.0f) << " m, snaps " << s.snaps);
      // Steady-state correction error under ~5 cm (the position error at each acknowledged
      // input, i.e. what a replay would have to correct).
      CHECK(Percentile(c->ack_errors, 0.95f) < 0.05f);
      CHECK(Percentile(c->corrections, 1.0f) < 0.2f);  // no large corrections at all
      CHECK(s.snaps == 0);
    }
    const float replay_ratio =
        static_cast<float>(a.predictor->stats().replays - replays_before) /
        static_cast<float>(a.predictor->stats().snapshots - snapshots_before);
    CHECK(replay_ratio < 0.5f);  // most snapshots need no replay
    // Each client sees the other move (remote state in snapshots).
    CHECK(a.remotes.count(b.player_id) == 1);
    CHECK(b.remotes.count(a.player_id) == 1);
  }

  TEST_CASE("input takes effect on the very next predicted tick (no round trip)") {
    NetSim sim({150, 20, 0.05});
    bool go = false;
    auto& c = sim.Join([&go](int, const SimClient&) {
      player::Input i;
      i.move_y = go ? 1.0f : 0.0f;
      return i;
    });
    sim.Step(Ticks(1.0f));
    go = true;
    sim.Step(1);
    CHECK(c.predictor->Velocity().GetZ() > 0.5f);
  }

  TEST_CASE("the debug launch pad knockback replays smoothly at 150 ms RTT") {
    core::ServerConfig config;
    config.spawn[0] = 2.5f;  // slot 0 spawns 1.5 m to the left of this
    config.spawn[2] = -4.75f;
    NetSim sim({150, 20, 0.05}, config);
    auto& c = sim.Join([](int tick, const SimClient& self) {
      return tick < Ticks(4.0f) ? SteerTo(self, 0.5f, -5.5f) : player::Input{};
    });
    sim.Step(Ticks(6.0f));
    const auto knockbacks = std::count_if(c.events.begin(), c.events.end(), [](auto& e) {
      return e.kind == PlayerEventKind::kKnockback;
    });
    CHECK(knockbacks >= 1);
    CHECK(c.predictor->stats().knockback_replays >= 1);
    CHECK(c.predictor->stats().snaps == 0);
    MESSAGE("largest rendered step per tick: " << c.max_render_step << " m");
    CHECK(c.max_render_step < 0.8f);  // continuous: the ~2 m correction is smoothed, not popped
  }

  TEST_CASE("bumping into another player corrects smoothly") {
    NetSim sim({150, 20, 0.05});
    auto& a = sim.Join([](int tick, const SimClient& self) {
      return tick < Ticks(3.0f) ? SteerTo(self, 9.0f, -9.0f) : SteerTo(self, 9.0f, -4.0f);
    });
    auto& b = sim.Join([](int tick, const SimClient& self) {
      return tick < Ticks(3.0f) ? SteerTo(self, 9.0f, -4.0f) : SteerTo(self, 9.0f, -9.0f);
    });
    sim.Step(Ticks(8.0f));
    for (SimClient* c : {&a, &b}) {
      MESSAGE("client " << c->player_id << ": max rendered step " << c->max_render_step
                        << " m, snaps " << c->predictor->stats().snaps);
      CHECK(c->predictor->stats().snaps == 0);
      CHECK(c->max_render_step < 0.3f);
    }
  }
}
