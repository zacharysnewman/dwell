// Protocol codec tests against the shared golden vectors (shared/protocol/vectors.txt). The field
// values below mirror shared/protocol/make_vectors.py and client/src/protocol/messages.test.ts.
#include <doctest/doctest.h>

#include <fstream>
#include <map>
#include <sstream>
#include <string>
#include <vector>

#include "dwell/protocol/bytes.h"
#include "dwell/protocol/messages.h"

using namespace dwell::protocol;

namespace {

std::map<std::string, std::vector<std::uint8_t>> LoadVectors() {
  std::ifstream in(DWELL_PROTOCOL_VECTORS);
  REQUIRE(in.good());
  std::map<std::string, std::vector<std::uint8_t>> out;
  std::string line;
  while (std::getline(in, line)) {
    if (line.empty() || line[0] == '#') continue;
    std::istringstream ls(line);
    std::string name, hex;
    ls >> name >> hex;
    std::vector<std::uint8_t> bytes;
    for (std::size_t i = 0; i + 1 < hex.size(); i += 2) {
      bytes.push_back(static_cast<std::uint8_t>(std::stoi(hex.substr(i, 2), nullptr, 16)));
    }
    out[name] = bytes;
  }
  return out;
}

template <std::size_t N>
std::array<std::uint8_t, N> Seq(std::uint8_t start, std::uint8_t step = 1) {
  std::array<std::uint8_t, N> a{};
  for (std::size_t i = 0; i < N; ++i) a[i] = static_cast<std::uint8_t>(start + i * step);
  return a;
}

ControllerState TestController(std::uint8_t flags) {
  ControllerState c;
  c.flags = flags;
  c.current_x = 1.5f;
  c.current_z = -2.25f;
  c.external_x = 0.5f;
  c.contribution_x = 2.0f;
  c.contribution_z = -2.25f;
  c.accumulated_y = 3.0f;
  c.target_y = -1.0f;
  c.ground_velocity_y = 0.25f;
  c.ground_kind = GroundKind::kPlayer;
  c.ground_id = 7;
  c.buffer_ticks = 11;
  c.coyote_ticks = 5;
  c.step_grace = 2;
  return c;
}

LocalPlayerState TestLocal(std::uint8_t flags, std::uint8_t health, PlayerState state,
                           ControllerState controller) {
  LocalPlayerState l;
  l.position[0] = 10.5f;
  l.position[1] = 0.9f;
  l.position[2] = -3.25f;
  l.velocity[0] = 5.0f;
  l.velocity[1] = -0.5f;
  l.flags = flags;
  l.health = health;
  l.state = state;
  l.controller = controller;
  return l;
}

PhysicsSnapshot TestSnapshot() {
  PhysicsSnapshot m;
  m.server_tick = 603;
  m.ack_input_seq = 42;
  auto c = TestController(ControllerFlags::kGrounded | ControllerFlags::kClimbing |
                          ControllerFlags::kHasReleased);
  c.ladder_x = -5;
  c.ladder_y = 64;
  c.ladder_z = 1000000;
  c.released_x = -5;
  c.released_z = 7;
  m.local =
      TestLocal(PlayerFlags::kGrounded | PlayerFlags::kClimbing, 87, PlayerState::kClimbing, c);
  m.local.input_buffer = 3;
  m.local.last_knockback_seq = 40;
  m.remotes.push_back({3,
                       {1, 2, 3},
                       {0.5f, -8.0f, 0.000061035156f},
                       16384,
                       -8192,
                       PlayerState::kRunning,
                       PlayerFlags::kGrounded});
  m.remotes.push_back({9,
                       {-1, 0, 65504.0f},
                       {65504.0f, -0.0f, 1.0f},
                       0,
                       0,
                       PlayerState::kSwimming,
                       PlayerFlags::kSwimming | PlayerFlags::kDead});
  return m;
}

PlayerEvent TestEvent(PlayerEventKind kind) {
  PlayerEvent e;
  e.kind = kind;
  e.player_id = 5;
  e.server_tick = 1200;
  e.input_seq = 77;
  return e;
}

std::map<std::string, Message> Expected() {
  PlayerInput input;
  input.last_snapshot_tick = 300;
  input.inputs = {{41, 127, -127, InputButtons::kJump | InputButtons::kRun, -16384, 32767},
                  {42, 0, 90, InputButtons::kCrouch, 12345, -100}};
  PhysicsSnapshot minimal;
  minimal.server_tick = 3;
  minimal.local = TestLocal(0, 100, PlayerState::kIdle, TestController(0));
  PlayerEvent knockback = TestEvent(PlayerEventKind::kKnockback);
  knockback.vector[1] = 14.0f;
  knockback.vector[2] = -0.5f;
  PlayerEvent damage = TestEvent(PlayerEventKind::kDamage);
  damage.amount = 17;
  damage.cause = DamageCause::kFall;
  PlayerEvent death = TestEvent(PlayerEventKind::kDeath);
  death.cause = DamageCause::kCrush;
  PlayerEvent respawn = TestEvent(PlayerEventKind::kRespawn);
  respawn.vector[0] = 0.5f;
  respawn.vector[2] = 0.5f;
  return {
      {"player_input", input},
      {"physics_snapshot", TestSnapshot()},
      {"physics_snapshot_min", minimal},
      {"player_event_knockback", knockback},
      {"player_event_damage", damage},
      {"player_event_death", death},
      {"player_event_respawn", respawn},
      {"datagram_ping", DatagramPing{0x01020304, 1234.5}},
      {"datagram_pong", DatagramPong{7, 0.25, 600}},
      {"status_request", StatusRequest{}},
      {"status_response", StatusResponse{1, "Dwell Test", "h\xC3\xA9llo \xE2\x9C\x93", 3, 8,
                                         StatusResponse::kFlagOnlineMode}},
      {"client_hello", ClientHello{1, "0.1.0", Seq<32>(0), "Zack"}},
      {"challenge", Challenge{Seq<32>(0xA0)}},
      {"client_auth", ClientAuth{Seq<64>(0, 3)}},
      {"welcome", Welcome{42, 0x0123456789ABCDEFull, 7, 123456}},
      {"reject", Reject{RejectReason::kProtocolVersion, "Server runs protocol 2"}},
      {"ping", Ping{9, 1000.0}},
      {"pong", Pong{9, 1000.0, 60, 5000.125}},
  };
}

}  // namespace

TEST_CASE("protocol: encoding matches golden vectors") {
  const auto vectors = LoadVectors();
  for (const auto& [name, message] : Expected()) {
    CAPTURE(name);
    REQUIRE(vectors.count(name) == 1);
    CHECK(Encode(message) == vectors.at(name));
  }
}

TEST_CASE("protocol: decoding golden vectors round-trips") {
  const auto vectors = LoadVectors();
  for (const auto& [name, message] : Expected()) {
    CAPTURE(name);
    const auto decoded = Decode(vectors.at(name));
    REQUIRE(decoded.has_value());
    CHECK(decoded->index() == message.index());
    CHECK(Encode(*decoded) == vectors.at(name));
  }
}

TEST_CASE("protocol: malformed vectors are rejected") {
  int count = 0;
  for (const auto& [name, bytes] : LoadVectors()) {
    if (name[0] != '!') continue;
    CAPTURE(name);
    CHECK_FALSE(Decode(bytes).has_value());
    ++count;
  }
  CHECK(count >= 15);
}

TEST_CASE("protocol: auth transcript layout") {
  const auto vectors = LoadVectors();
  CHECK(AuthTranscript(Seq<32>(0xA0), Seq<32>(0x10), Seq<32>(0)) == vectors.at("auth_transcript"));
}

TEST_CASE("protocol: over-long strings are truncated on a UTF-8 boundary when encoding") {
  ClientHello hello{1, "v", {}, std::string(kDisplayNameMaxBytes - 1, 'a') + "\xC3\xA9"};
  const auto decoded = Decode(Encode(hello));
  REQUIRE(decoded.has_value());
  const auto& name = std::get<ClientHello>(*decoded).display_name;
  CHECK(name == std::string(kDisplayNameMaxBytes - 1, 'a'));
}

TEST_CASE("protocol: snapshot fields survive decoding (f16 velocities included)") {
  const auto decoded = Decode(Encode(TestSnapshot()));
  REQUIRE(decoded.has_value());
  const auto& m = std::get<PhysicsSnapshot>(*decoded);
  CHECK(m.local.controller.ladder_z == 1000000);
  CHECK(m.local.controller.released_z == 7);
  REQUIRE(m.remotes.size() == 2);
  CHECK(m.remotes[0].velocity[1] == -8.0f);
  CHECK(m.remotes[1].velocity[0] == 65504.0f);
  CHECK(m.remotes[1].flags == (PlayerFlags::kSwimming | PlayerFlags::kDead));
}

TEST_CASE("protocol: half floats round to nearest even and saturate to infinity") {
  CHECK(FloatToHalf(1.0f) == 0x3C00);
  CHECK(FloatToHalf(-2.0f) == 0xC000);
  CHECK(FloatToHalf(65504.0f) == 0x7BFF);
  CHECK(FloatToHalf(1e6f) == 0x7C00);
  CHECK(FloatToHalf(5.9604645e-08f) == 0x0001);         // smallest subnormal
  CHECK(FloatToHalf(1.0f + 1.0f / 2048.0f) == 0x3C00);  // tie → even
  CHECK(FloatToHalf(1.0f + 3.0f / 2048.0f) == 0x3C02);  // tie → even (up)
  int mismatches = 0;
  for (std::uint32_t h = 0; h < 0x7C00; ++h) {
    if (FloatToHalf(HalfToFloat(static_cast<std::uint16_t>(h))) != h) ++mismatches;
  }
  CHECK(mismatches == 0);  // every finite half round-trips
}
