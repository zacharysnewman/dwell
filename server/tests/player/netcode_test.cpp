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
    const JPH::Vec3 p = self.Position();
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
    players.Teleport(ha, ToWorld(8.5, 5.9, -8.5), JPH::Vec3::sZero());
    sim.Step(90);
    const int health = sim.server().HealthOf(a.player_id);
    CHECK(health < protocol::kMaxHealth);
    CHECK(health > 0);
    players.Teleport(*sim.server().PlayerHandleOf(a.player_id), ToWorld(8.5, 30.9, -8.5),
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
    const JPH::Vec3 pa = ToLocal(players.Position(*sim.server().PlayerHandleOf(a.player_id)));
    const JPH::Vec3 pb = ToLocal(players.Position(*sim.server().PlayerHandleOf(b.player_id)));
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
    core::ServerConfig config{.generator_version = core::kGeneratorPlayground};
    config.spawn = {2.5f, 0.0f, -4.75f};  // slot 0 spawns 1.5 m to the left of this
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

TEST_CASE("predictor: remote proxy right after the first snapshot") {
  core::JoltRuntime runtime;
  JPH::JobSystemSingleThreaded jobs(JPH::cMaxPhysicsJobs);
  core::VoxelWorld world(core::GeneratorFor(1));
  player::PlayerControllerConfig config;
  player::Predictor p(world, jobs, config);
  protocol::PhysicsSnapshot snap;
  snap.server_tick = 3;
  snap.local.position[1] = 0.9f;
  snap.local.position[2] = -0.25f;
  snap.local.controller.flags = protocol::ControllerFlags::kGrounded;
  snap.local.controller.ground_kind = protocol::GroundKind::kTerrain;
  p.OnSnapshot(snap);
  p.SetRemote(1, JPH::RVec3(-1, 0, -0.25), JPH::Vec3::sZero(), false, 0);
  for (int i = 0; i < 10; ++i) p.Tick(player::QuantizeInput({}, p.next_seq()));
  CHECK(p.active());
}

TEST_SUITE("netcode: block edits") {
  TEST_CASE("a block placed by one player blocks another on the server and in its prediction") {
    core::ServerConfig config{.generator_version = core::kGeneratorPlayground};
    config.spawn = {20.5, 0.0, 0.5};  // open ground; slot 0 at x 19, slot 1 at x 20
    NetSim sim({100, 10, 0.0}, config);
    bool walk = false;
    auto& a = sim.Join(nullptr, /*stream=*/true);
    auto& b = sim.Join(
        [&walk](int, const SimClient& self) {
          return walk ? SteerTo(self, 20.5f, 8.0f) : player::Input{};
        },
        /*stream=*/true);
    sim.Step(Ticks(1.0f));

    // A builds a wall two blocks high across B's path at z = 3 (edits spaced for the rate limit).
    for (int y : {-1, 0}) {
      for (int x : {19, 20, 21}) {
        sim.Send(a, protocol::BlockEditRequest{protocol::BlockEditAction::kPlace,
                                               {WorldCellX(x), y, 3},
                                               2,
                                               core::Materials::kStone});
        sim.Step(8);
      }
    }
    sim.Step(Ticks(0.5f));
    auto& server_world = sim.server().world();
    for (int y : {0, 1}) {
      for (int x : {19, 20, 21}) {
        CAPTURE(x);
        CAPTURE(y);
        CHECK(server_world.GetVoxel(WorldCellX(x), y, 3) == core::Materials::kStone);
        CHECK(a.world.GetVoxel(WorldCellX(x), y, 3) == core::Materials::kStone);
        CHECK(b.world.GetVoxel(WorldCellX(x), y, 3) == core::Materials::kStone);
      }
    }
    CHECK(sim.server().StatsOf(a.player_id)->edits_applied == 6);
    CHECK(b.modifications.size() >= 2);

    // B walks into it: stopped by the wall on the server and in its own prediction.
    walk = true;
    float server_max = -10, client_max = -10;
    for (int i = 0; i < Ticks(3.0f); ++i) {
      sim.Step(1);
      const auto h = *sim.server().PlayerHandleOf(b.player_id);
      server_max = std::max(server_max, ToLocal(sim.server().players().Position(h)).GetZ());
      client_max = std::max(client_max, b.Position().GetZ());
    }
    MESSAGE("closest approach: server " << server_max << ", client " << client_max);
    CHECK(server_max > 2.5f);  // it did walk up to the wall
    CHECK(server_max < 3.0f - 0.29f);
    CHECK(client_max < 3.0f - 0.29f);
  }
}

// Creative flight (§8.3): who may fly is the server's call.
TEST_SUITE("netcode: flight policy") {
  auto fly_script = [](int, const SimClient&) {
    player::Input i;
    i.fly = true;
    i.jump = true;
    return i;
  };

  TEST_CASE("everyone may fly by default: Welcome says so and the server flies the player") {
    NetSim sim({});
    auto& c = sim.Join(fly_script);
    CHECK((c.welcome_flags & protocol::WelcomeFlags::kFlight) != 0);
    sim.Step(10);
    const float start = c.Position().GetY();
    sim.Step(Ticks(1.0f));
    CHECK(c.Position().GetY() > start + 5.0f);
  }

  TEST_CASE("with flight off the server ignores the fly bit") {
    core::ServerConfig config{.generator_version = core::kGeneratorPlayground};
    config.flight = core::EditPolicy::kNobody;
    NetSim sim({}, config);
    auto& c = sim.Join(fly_script);
    CHECK((c.welcome_flags & protocol::WelcomeFlags::kFlight) == 0);
    sim.Step(10);
    const float start = c.Position().GetY();
    sim.Step(Ticks(1.0f));
    // Jumping in place: never much above the start once the prediction is corrected.
    CHECK(c.Position().GetY() < start + 2.0f);
  }
}
