// Server core: status query, join handshake with device-key identity (ADR 0004), and pings.
#include <doctest/doctest.h>

#include <Jolt/Jolt.h>

#include <Jolt/Core/JobSystemSingleThreaded.h>
#include <monocypher-ed25519.h>

#include "dwell/core/jolt_runtime.h"
#include "dwell/core/server.h"

using namespace dwell::core;
using namespace dwell::protocol;

namespace {

class FixedEntropy final : public Entropy {
 public:
  void Fill(std::span<std::uint8_t> out) override {
    for (auto& b : out) b = next_++;
  }

 private:
  std::uint8_t next_ = 0x55;
};

struct Identity {
  std::array<std::uint8_t, 64> secret{};
  PublicKey public_key{};
  explicit Identity(std::uint8_t seed_byte) {
    std::array<std::uint8_t, 32> seed;
    seed.fill(seed_byte);
    crypto_ed25519_key_pair(secret.data(), public_key.data(), seed.data());
  }
  Signature Sign(std::span<const std::uint8_t> message) const {
    Signature sig{};
    crypto_ed25519_sign(sig.data(), secret.data(), message.data(), message.size());
    return sig;
  }
};

struct Fixture {
  JoltRuntime runtime;
  JPH::JobSystemSingleThreaded jobs{JPH::cMaxPhysicsJobs};
  FixedEntropy entropy;
  Server server;
  TransportBinding binding{};

  explicit Fixture(ServerConfig config = {}) : server(std::move(config), entropy, jobs) {
    binding.fill(0x42);
  }

  void Send(SessionId id, const Message& m) { server.OnReliable(id, Channel::kControl, Encode(m)); }

  std::vector<Message> Replies(SessionId id, bool* closed = nullptr) {
    std::vector<Message> out;
    for (auto& o : server.TakeOutbox()) {
      if (o.session != id) continue;
      if (o.kind == Outgoing::Kind::kClose) {
        if (closed) *closed = true;
        continue;
      }
      auto m = Decode(o.bytes);
      REQUIRE(m.has_value());
      out.push_back(*m);
    }
    return out;
  }

  // Runs the full handshake; returns the Welcome (or fails the test).
  Welcome Join(SessionId id, const Identity& who) {
    server.OnConnected(id, TransportKind::kWebTransport, binding);
    Send(id, ClientHello{kProtocolVersion, "test", who.public_key, "Tester"});
    auto replies = Replies(id);
    REQUIRE(replies.size() == 1);
    const auto nonce = std::get<Challenge>(replies[0]).nonce;
    Send(id, ClientAuth{who.Sign(AuthTranscript(nonce, binding, who.public_key))});
    replies = Replies(id);
    REQUIRE(replies.size() == 1);
    REQUIRE(std::holds_alternative<Welcome>(replies[0]));
    return std::get<Welcome>(replies[0]);
  }
};

}  // namespace

TEST_CASE("server: status query works before joining") {
  Fixture f(ServerConfig{.name = "Test", .motd = "hi", .max_players = 4});
  f.server.OnConnected(1, TransportKind::kWebTransport, f.binding);
  f.Send(1, StatusRequest{});
  const auto replies = f.Replies(1);
  REQUIRE(replies.size() == 1);
  const auto& status = std::get<StatusResponse>(replies[0]);
  CHECK(status.server_name == "Test");
  CHECK(status.max_players == 4);
  CHECK(status.players == 0);
}

TEST_CASE("server: a signed handshake joins and assigns a player id") {
  Fixture f(ServerConfig{.world_seed = 1234});
  const auto welcome = f.Join(7, Identity(1));
  CHECK(welcome.player_id == 1);
  CHECK(welcome.world_seed == 1234);
  CHECK(f.server.joined_players() == 1);
}

TEST_CASE("server: wrong protocol version is rejected and closed") {
  Fixture f;
  f.server.OnConnected(1, TransportKind::kWebTransport, f.binding);
  f.Send(1, ClientHello{static_cast<std::uint16_t>(kProtocolVersion + 1), "t", {}, "x"});
  bool closed = false;
  const auto replies = f.Replies(1, &closed);
  REQUIRE(replies.size() == 1);
  CHECK(std::get<Reject>(replies[0]).reason == RejectReason::kProtocolVersion);
  CHECK(closed);
}

TEST_CASE("server: a bad signature is rejected") {
  Fixture f;
  const Identity who(2), other(3);
  f.server.OnConnected(1, TransportKind::kWebTransport, f.binding);
  f.Send(1, ClientHello{kProtocolVersion, "t", who.public_key, "x"});
  const auto nonce = std::get<Challenge>(f.Replies(1)[0]).nonce;
  // Signed by a different key.
  f.Send(1, ClientAuth{other.Sign(AuthTranscript(nonce, f.binding, who.public_key))});
  bool closed = false;
  const auto replies = f.Replies(1, &closed);
  CHECK(std::get<Reject>(replies[0]).reason == RejectReason::kAuthFailed);
  CHECK(closed);
}

TEST_CASE("server: a signature for another transport binding is rejected") {
  Fixture f;
  const Identity who(4);
  f.server.OnConnected(1, TransportKind::kWebTransport, f.binding);
  f.Send(1, ClientHello{kProtocolVersion, "t", who.public_key, "x"});
  const auto nonce = std::get<Challenge>(f.Replies(1)[0]).nonce;
  TransportBinding elsewhere{};
  elsewhere.fill(0x99);
  f.Send(1, ClientAuth{who.Sign(AuthTranscript(nonce, elsewhere, who.public_key))});
  CHECK(std::get<Reject>(f.Replies(1)[0]).reason == RejectReason::kAuthFailed);
}

TEST_CASE("server: the same key cannot join twice") {
  Fixture f;
  const Identity who(5);
  f.Join(1, who);
  f.server.OnConnected(2, TransportKind::kWebTransport, f.binding);
  f.Send(2, ClientHello{kProtocolVersion, "t", who.public_key, "x"});
  const auto nonce = std::get<Challenge>(f.Replies(2)[0]).nonce;
  f.Send(2, ClientAuth{who.Sign(AuthTranscript(nonce, f.binding, who.public_key))});
  CHECK(std::get<Reject>(f.Replies(2)[0]).reason == RejectReason::kAuthFailed);
}

TEST_CASE("server: rejects joins when full") {
  Fixture f(ServerConfig{.max_players = 1});
  f.Join(1, Identity(6));
  f.server.OnConnected(2, TransportKind::kWebTransport, f.binding);
  f.Send(2, ClientHello{kProtocolVersion, "t", Identity(7).public_key, "x"});
  CHECK(std::get<Reject>(f.Replies(2)[0]).reason == RejectReason::kFull);
}

TEST_CASE("server: malformed control messages close the session") {
  Fixture f;
  f.server.OnConnected(1, TransportKind::kWebTransport, f.binding);
  const std::uint8_t junk[] = {0xEE, 0x01};
  f.server.OnReliable(1, Channel::kControl, junk);
  bool closed = false;
  CHECK(std::get<Reject>(f.Replies(1, &closed)[0]).reason == RejectReason::kMalformed);
  CHECK(closed);
}

TEST_CASE("server: pings report the server tick; datagram pings need a joined session") {
  Fixture f;
  f.server.OnConnected(1, TransportKind::kWebTransport, f.binding);
  for (int i = 0; i < 3; ++i) f.server.Step();

  f.server.OnDatagram(1, Encode(DatagramPing{1, 10.0}));
  CHECK(f.Replies(1).empty());  // not joined yet

  f.Send(1, Ping{2, 20.0});
  const auto pong = std::get<Pong>(f.Replies(1)[0]);
  CHECK(pong.seq == 2);
  CHECK(pong.client_time_ms == 20.0);
  CHECK(pong.server_tick == 3);

  f.Join(1, Identity(8));
  f.server.OnDatagram(1, Encode(DatagramPing{3, 30.0}));
  const auto dpong = std::get<DatagramPong>(f.Replies(1)[0]);
  CHECK(dpong.seq == 3);
  CHECK(dpong.server_tick == 3);
}
