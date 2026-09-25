#include "dwell/core/server.h"

#include <algorithm>
#include <variant>

#include "dwell/core/crypto.h"

namespace dwell::core {

using namespace protocol;

Server::Server(ServerConfig config, Entropy& entropy, JPH::JobSystem& jobs)
    : config_(std::move(config)), entropy_(entropy), physics_(jobs) {}

Server::~Server() = default;

void Server::OnConnected(SessionId id, TransportKind kind, const TransportBinding& binding) {
  Session session;
  session.kind = kind;
  session.binding = binding;
  sessions_[id] = std::move(session);
}

void Server::OnDisconnected(SessionId id) { sessions_.erase(id); }

void Server::OnReliable(SessionId id, Channel channel, std::span<const std::uint8_t> bytes) {
  const auto it = sessions_.find(id);
  if (it == sessions_.end()) return;
  // Clients may only send on the control channel; the world channel is server → client.
  const auto message = channel == Channel::kControl ? Decode(bytes) : std::nullopt;
  if (!message) {
    Reject(id, RejectReason::kMalformed, "Malformed message.");
    return;
  }
  HandleControl(id, it->second, *message);
}

void Server::OnDatagram(SessionId id, std::span<const std::uint8_t> bytes) {
  const auto it = sessions_.find(id);
  if (it == sessions_.end() || it->second.phase != Phase::kJoined) return;
  // Datagrams are unreliable: malformed ones are dropped, not fatal.
  const auto message = Decode(bytes);
  if (!message) return;
  if (const auto* ping = std::get_if<DatagramPing>(&*message)) {
    SendDatagram(id, DatagramPong{ping->seq, ping->client_time_ms, tick_});
  }
}

void Server::HandleControl(SessionId id, Session& s, const Message& m) {
  if (std::holds_alternative<StatusRequest>(m)) {
    SendReliable(id, StatusResponse{kProtocolVersion, config_.name, config_.motd,
                                    static_cast<std::uint16_t>(joined_players()),
                                    config_.max_players, /*flags=*/0});
    return;
  }
  if (const auto* ping = std::get_if<Ping>(&m)) {
    SendReliable(id, Pong{ping->seq, ping->client_time_ms, tick_, time_ms()});
    return;
  }

  switch (s.phase) {
    case Phase::kAwaitingHello: {
      const auto* hello = std::get_if<ClientHello>(&m);
      if (!hello) break;
      if (hello->protocol_version != kProtocolVersion) {
        Reject(id, RejectReason::kProtocolVersion,
               "Server runs protocol " + std::to_string(kProtocolVersion) + ", client runs " +
                   std::to_string(hello->protocol_version) + ".");
        return;
      }
      if (joined_players() >= config_.max_players) {
        Reject(id, RejectReason::kFull, "Server is full.");
        return;
      }
      s.public_key = hello->public_key;
      s.display_name = hello->display_name.empty() ? "Player" : hello->display_name;
      entropy_.Fill(s.nonce);
      s.phase = Phase::kAwaitingAuth;
      SendReliable(id, Challenge{s.nonce});
      return;
    }
    case Phase::kAwaitingAuth: {
      const auto* auth = std::get_if<ClientAuth>(&m);
      if (!auth) break;
      const auto transcript = AuthTranscript(s.nonce, s.binding, s.public_key);
      if (!VerifyEd25519(auth->signature, s.public_key, transcript)) {
        Reject(id, RejectReason::kAuthFailed, "Identity check failed.");
        return;
      }
      // The signature proves key ownership, so a new login replaces any older session for the
      // same player (e.g. one whose connection dropped but hasn't timed out yet).
      std::vector<SessionId> replaced;
      for (const auto& [other_id, other] : sessions_) {
        if (other_id != id && other.phase == Phase::kJoined && other.public_key == s.public_key) {
          replaced.push_back(other_id);
        }
      }
      for (const auto other_id : replaced) {
        Reject(other_id, RejectReason::kReplaced, "Signed in from another connection.");
      }
      s.player_id = AllocatePlayerId();
      s.phase = Phase::kJoined;
      SendReliable(id, Welcome{s.player_id, config_.world_seed, config_.generator_version, tick_});
      return;
    }
    case Phase::kJoined:
      // Phase 1 has no further client control messages beyond ping/status.
      return;
  }
  Reject(id, RejectReason::kMalformed, "Unexpected message.");
}

void Server::Step() {
  physics_.Step(1.0f / kSimHz);
  ++tick_;
}

std::vector<Outgoing> Server::TakeOutbox() { return std::exchange(outbox_, {}); }

std::size_t Server::joined_players() const {
  return static_cast<std::size_t>(std::count_if(sessions_.begin(), sessions_.end(), [](auto& kv) {
    return kv.second.phase == Phase::kJoined;
  }));
}

void Server::SendReliable(SessionId id, const Message& m) {
  outbox_.push_back({id, Outgoing::Kind::kReliable, Channel::kControl, Encode(m)});
}

void Server::SendDatagram(SessionId id, const Message& m) {
  outbox_.push_back({id, Outgoing::Kind::kDatagram, Channel::kControl, Encode(m)});
}

void Server::Reject(SessionId id, RejectReason reason, std::string message) {
  SendReliable(id, protocol::Reject{reason, std::move(message)});
  outbox_.push_back({id, Outgoing::Kind::kClose, Channel::kControl, {}});
  sessions_.erase(id);
}

std::uint16_t Server::AllocatePlayerId() {
  // Player ids are unique among joined players; skip 0 and ids in use.
  for (;;) {
    const std::uint16_t candidate = next_player_id_++;
    if (candidate == 0) continue;
    const bool used = std::any_of(sessions_.begin(), sessions_.end(), [&](auto& kv) {
      return kv.second.phase == Phase::kJoined && kv.second.player_id == candidate;
    });
    if (!used) return candidate;
  }
}

}  // namespace dwell::core
