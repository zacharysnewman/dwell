// Friend-world hosting in the integrated server (ARCHITECTURE.md §10.2, Phase 5c): a local world
// admits guests once its player starts hosting, with the host's limits and policies; guests hear
// when the host pauses; stopping hosting (or shutting down) ends sessions with a reason.
#include <doctest/doctest.h>

#include <algorithm>

#include "server_fixture.h"

using namespace dwell::test;

namespace {

// A local world before hosting: one player, like dwell_local_create.
ServerConfig LocalWorld() {
  ServerConfig c = Flat();
  c.max_players = 1;
  return c;
}

// Connects over `kind` and runs the handshake; the server's last reply (Welcome or Reject).
Message Handshake(Fixture& f, SessionId id, const Client& who, TransportKind kind) {
  f.server.OnConnected(id, kind, f.binding);
  f.Send(id, ClientHello{kProtocolVersion, "test", who.public_key, "Tester"});
  auto reply = f.Take(id).at(0).first;
  if (!std::holds_alternative<Challenge>(reply)) return reply;
  const auto transcript =
      AuthTranscript(std::get<Challenge>(reply).nonce, f.binding, who.public_key);
  Signature sig{};
  crypto_ed25519_sign(sig.data(), who.secret.data(), transcript.data(), transcript.size());
  f.Send(id, ClientAuth{sig});
  return f.Take(id).at(0).first;
}

RejectReason ReasonOf(const Message& m) {
  REQUIRE(std::holds_alternative<Reject>(m));
  return std::get<Reject>(m).reason;
}

bool MayFly(const Message& welcome) {
  REQUIRE(std::holds_alternative<Welcome>(welcome));
  return (std::get<Welcome>(welcome).flags & WelcomeFlags::kFlight) != 0;
}

}  // namespace

TEST_CASE("hosting: a local world admits guests only once its host starts hosting, up to its cap") {
  Fixture f(LocalWorld());
  const Client host(1), guest(2), third(3);
  REQUIRE(std::holds_alternative<Welcome>(Handshake(f, 1, host, TransportKind::kLoopback)));
  CHECK(ReasonOf(Handshake(f, 2, guest, TransportKind::kWebRtc)) == RejectReason::kFull);

  f.server.SetHosting(2, EditPolicy::kEveryone, EditPolicy::kEveryone, host.public_key);
  CHECK(std::holds_alternative<Welcome>(Handshake(f, 3, guest, TransportKind::kWebRtc)));
  CHECK(f.server.joined_players() == 2);
  CHECK(ReasonOf(Handshake(f, 4, third, TransportKind::kWebRtc)) == RejectReason::kFull);
}

TEST_CASE("hosting: with ops-only policies the host (an op) may fly, guests may not") {
  Fixture f(LocalWorld());
  const Client host(1), guest(2);
  REQUIRE(std::holds_alternative<Welcome>(Handshake(f, 1, host, TransportKind::kLoopback)));
  f.server.SetHosting(4, EditPolicy::kOps, EditPolicy::kOps, host.public_key);
  CHECK_FALSE(MayFly(Handshake(f, 2, guest, TransportKind::kWebRtc)));
  // The host rejoining (e.g. after a reload) is still an op.
  CHECK(MayFly(Handshake(f, 3, host, TransportKind::kLoopback)));
}

TEST_CASE("hosting: every joined client hears that the host paused and resumed") {
  Fixture f(LocalWorld());
  const Client host(1), guest(2);
  REQUIRE(std::holds_alternative<Welcome>(Handshake(f, 1, host, TransportKind::kLoopback)));
  f.server.SetHosting(4, EditPolicy::kEveryone, EditPolicy::kEveryone, host.public_key);
  REQUIRE(std::holds_alternative<Welcome>(Handshake(f, 2, guest, TransportKind::kWebRtc)));
  f.TakeAll();

  f.server.BroadcastHostStatus(HostState::kPaused);
  auto sent = f.TakeAll();
  for (const SessionId id : {1u, 2u}) {
    REQUIRE(sent[id].size() == 1);
    CHECK(std::get<HostStatus>(sent[id][0]).state == HostState::kPaused);
  }
  f.server.BroadcastHostStatus(HostState::kResumed);
  sent = f.TakeAll();
  CHECK(std::get<HostStatus>(sent[2].at(0)).state == HostState::kResumed);
}

TEST_CASE("hosting: stopping ends the guests' sessions with a reason and keeps the host playing") {
  Fixture f(LocalWorld());
  const Client host(1), a(2), b(3);
  REQUIRE(std::holds_alternative<Welcome>(Handshake(f, 1, host, TransportKind::kLoopback)));
  f.server.SetHosting(4, EditPolicy::kEveryone, EditPolicy::kEveryone, host.public_key);
  REQUIRE(std::holds_alternative<Welcome>(Handshake(f, 2, a, TransportKind::kWebRtc)));
  REQUIRE(std::holds_alternative<Welcome>(Handshake(f, 3, b, TransportKind::kWebRtc)));
  f.TakeAll();

  f.server.CloseSessions("The host stopped hosting.", /*keep_loopback=*/true);
  const auto out = f.server.TakeOutbox();
  for (const SessionId guest : {2u, 3u}) {
    const auto reject = std::find_if(out.begin(), out.end(), [&](const Outgoing& o) {
      return o.session == guest && o.kind == Outgoing::Kind::kReliable;
    });
    REQUIRE(reject != out.end());
    const auto m = Decode(reject->bytes);
    REQUIRE(m.has_value());
    CHECK(std::get<Reject>(*m).reason == RejectReason::kServerClosing);
    CHECK(std::get<Reject>(*m).message == "The host stopped hosting.");
    CHECK(std::any_of(out.begin(), out.end(), [&](const Outgoing& o) {
      return o.session == guest && o.kind == Outgoing::Kind::kClose;
    }));
  }
  CHECK(std::none_of(out.begin(), out.end(), [](const Outgoing& o) { return o.session == 1; }));
  CHECK(f.server.joined_players() == 1);

  // A dedicated server shutting down closes everyone.
  f.server.CloseSessions("The server is shutting down.", /*keep_loopback=*/false);
  CHECK(f.server.joined_players() == 0);
}
