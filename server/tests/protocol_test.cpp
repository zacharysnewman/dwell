// Protocol codec tests against the shared golden vectors (shared/protocol/vectors.txt). The field
// values below mirror shared/protocol/make_vectors.py and client/src/protocol/messages.test.ts.
#include <doctest/doctest.h>

#include <fstream>
#include <map>
#include <sstream>
#include <string>
#include <vector>

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

std::map<std::string, Message> Expected() {
  return {
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
  CHECK(count >= 7);
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
