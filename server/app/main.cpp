// Dwell dedicated server (ARCHITECTURE.md §4, §10.1). Hosts the simulation core behind the Rust
// network front-end: transport events are drained into the core, the core steps at SIM_HZ, and its
// outbox is sent back out.
#include <Jolt/Jolt.h>

#include <Jolt/Core/JobSystemThreadPool.h>

#if defined(__unix__) || defined(__APPLE__)
#include <arpa/inet.h>
#include <ifaddrs.h>
#include <net/if.h>
#include <netinet/in.h>
#endif

#include <algorithm>
#include <array>
#include <atomic>
#include <chrono>
#include <csignal>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <thread>
#include <utility>
#include <vector>

#include "dwell/core/app_version.h"
#include "dwell/core/entropy.h"
#include "dwell/core/fixed_step.h"
#include "dwell/core/jolt_runtime.h"
#include "dwell/core/server.h"
#include "dwell/storage/world_store.h"
#include "dwell_net.h"

namespace {

std::atomic<bool> g_running{true};

void OnSignal(int) { g_running = false; }

// The deployed master server (ARCHITECTURE.md §10.3); --master overrides it.
constexpr const char* kDefaultMaster = "https://dwell-master.dropkick.workers.dev";

struct Options {
  std::uint16_t port = 4433;
  std::uint16_t rtc_port = 0;  // 0 = port + 1
  std::string advertise = "127.0.0.1";
  bool advertise_given = false;  // --advertise: also the address the master hands out
  // Master registration (§10.1, Phase 5d): unlisted (by code, address and on its network), public
  // (also in the lobby list, Phase 5e) or none (never contacts the master).
  std::string master = kDefaultMaster;
  std::string visibility = "unlisted";
  int heartbeat_s = 30;
  std::vector<std::string> tags;              // --tags: lobby-list search tags (Phase 5e)
  std::array<std::uint8_t, 32> server_key{};  // Ed25519 seed, kept in the world's settings
  dwell::core::ServerConfig server;
  std::string client_url = "http://localhost:5173/dwell/";
  std::string world = "world.dwellworld";  // empty: nothing is saved
  // Launch options that also change the world's settings table (§6.4, §10.1).
  std::vector<std::pair<std::string, std::string>> settings;
  std::vector<std::pair<dwell::storage::PermissionKind, std::string>> grants;  // key hex
};

void Usage() {
  std::puts(
      "usage: dwell_server [--world FILE] [--port N] [--rtc-port N] [--advertise IP]\n"
      "                    [--name NAME] [--motd TEXT] [--max-players N] [--edits POLICY]\n"
      "                    [--flight POLICY] [--seed N] [--generator N] [--op KEY] [--ban KEY]\n"
      "                    [--client-url URL] [--master URL] [--visibility public|unlisted|none]\n"
      "                    [--tags A,B]\n"
      "  --world FILE    the world file (default world.dwellworld; created if missing; \"\" keeps\n"
      "                  the world in memory only)\n"
      "  --advertise IP  address players use to reach this server (invite links, WebRTC)\n"
      "  --seed N, --generator N  a new world's seed and generator: 4 = procedural terrain\n"
      "                  (default), 1 = movement playground, 0 = flat (a saved world keeps its "
      "own)\n"
      "  --name, --motd, --max-players, --edits everyone|ops|nobody, --flight everyone|ops|nobody\n"
      "                  saved in the world's settings (--flight: who may use creative flight)\n"
      "  --op KEY, --ban KEY  grant op or ban a player (hex device public key), saved in the "
      "world\n"
      "  --master URL    the master server (default https://dwell-master.dropkick.workers.dev)\n"
      "  --visibility V  unlisted (default): players join by code or address and see it on its\n"
      "                  own network; public: also in the lobby list; none: never contacts the\n"
      "                  master\n"
      "  --tags A,B      tags players can search the lobby list by (e.g. pve,creative)\n"
      "  --heartbeat S   seconds between master heartbeats (default 30; testing)");
}

bool ParseOptions(int argc, char** argv, Options& o) {
  for (int i = 1; i < argc; ++i) {
    const std::string arg = argv[i];
    auto value = [&]() -> const char* { return i + 1 < argc ? argv[++i] : nullptr; };
    const char* v = nullptr;
    if (arg == "--help" || arg == "-h") return false;
    if (!(v = value())) return false;
    if (arg == "--port") {
      o.port = static_cast<std::uint16_t>(std::strtoul(v, nullptr, 10));
    } else if (arg == "--rtc-port") {
      o.rtc_port = static_cast<std::uint16_t>(std::strtoul(v, nullptr, 10));
    } else if (arg == "--advertise") {
      o.advertise = v;
      o.advertise_given = true;
    } else if (arg == "--master") {
      o.master = v;
    } else if (arg == "--visibility") {
      o.visibility = v;
      if (o.visibility != "public" && o.visibility != "unlisted" && o.visibility != "none") {
        return false;
      }
    } else if (arg == "--tags") {
      o.tags.clear();
      std::string tag;
      for (const char* p = v;; ++p) {
        if (*p == ',' || *p == '\0') {
          if (!tag.empty()) o.tags.push_back(tag);
          tag.clear();
          if (*p == '\0') break;
        } else if (*p != ' ') {
          tag += *p;
        }
      }
    } else if (arg == "--heartbeat") {
      o.heartbeat_s = std::clamp(std::atoi(v), 2, 60);
    } else if (arg == "--world") {
      o.world = v;
    } else if (arg == "--name") {
      o.settings.emplace_back("name", v);
    } else if (arg == "--motd") {
      o.settings.emplace_back("motd", v);
    } else if (arg == "--max-players") {
      o.settings.emplace_back("max_players", v);
    } else if (arg == "--edits") {
      if (std::string(v) != "everyone" && std::string(v) != "ops" && std::string(v) != "nobody") {
        return false;
      }
      o.settings.emplace_back("edits", v);
    } else if (arg == "--flight") {
      if (std::string(v) != "everyone" && std::string(v) != "ops" && std::string(v) != "nobody") {
        return false;
      }
      o.settings.emplace_back("flight", v);
    } else if (arg == "--op") {
      o.grants.emplace_back(dwell::storage::PermissionKind::kOp, v);
    } else if (arg == "--ban") {
      o.grants.emplace_back(dwell::storage::PermissionKind::kBan, v);
    } else if (arg == "--seed") {
      o.server.world_seed = std::strtoull(v, nullptr, 10);
    } else if (arg == "--generator") {
      o.server.generator_version = static_cast<std::uint32_t>(std::strtoul(v, nullptr, 10));
    } else if (arg == "--client-url") {
      o.client_url = v;
    } else {
      return false;
    }
  }
  return true;
}

std::string Hex(const std::uint8_t* bytes, std::size_t n) {
  static constexpr char kDigits[] = "0123456789abcdef";
  std::string out;
  for (std::size_t i = 0; i < n; ++i) {
    out += kDigits[bytes[i] >> 4];
    out += kDigits[bytes[i] & 0xF];
  }
  return out;
}

template <std::size_t N>
bool ParseHex(const std::string& hex, std::array<std::uint8_t, N>& out) {
  if (hex.size() != 2 * N) return false;
  for (std::size_t i = 0; i < N; ++i) {
    const std::string byte = hex.substr(2 * i, 2);
    char* end = nullptr;
    out[i] = static_cast<std::uint8_t>(std::strtoul(byte.c_str(), &end, 16));
    if (end != byte.c_str() + 2) return false;
  }
  return true;
}

bool ParseKey(const std::string& hex, dwell::protocol::PublicKey& key) {
  if (hex.size() != 64) return false;
  for (std::size_t i = 0; i < key.size(); ++i) {
    const std::string byte = hex.substr(2 * i, 2);
    char* end = nullptr;
    key[i] = static_cast<std::uint8_t>(std::strtoul(byte.c_str(), &end, 16));
    if (end != byte.c_str() + 2) return false;
  }
  return true;
}

// Applies a setting to the server config (§10.1): stored settings first, launch options after.
void ApplySetting(dwell::core::ServerConfig& c, const std::string& key, const std::string& value) {
  if (key == "name") {
    c.name = value;
  } else if (key == "motd") {
    c.motd = value;
  } else if (key == "max_players") {
    c.max_players = static_cast<std::uint16_t>(std::strtoul(value.c_str(), nullptr, 10));
  } else if (key == "edits") {
    c.edits = value == "ops"      ? dwell::core::EditPolicy::kOps
              : value == "nobody" ? dwell::core::EditPolicy::kNobody
                                  : dwell::core::EditPolicy::kEveryone;
  } else if (key == "flight") {
    c.flight = value == "ops"      ? dwell::core::EditPolicy::kOps
               : value == "nobody" ? dwell::core::EditPolicy::kNobody
                                   : dwell::core::EditPolicy::kEveryone;
  } else if (key == "autosave_seconds") {
    c.autosave_seconds = std::max(1, std::atoi(value.c_str()));
  } else if (key == "allow_list") {
    if (value == "1") c.allow_list.emplace();
  }
}

// Opens the world file and folds its settings and permissions (plus the launch options, which are
// saved into it) into the config. False on failure.
bool OpenWorld(Options& o) {
  auto& c = o.server;
  for (const auto& [key, value] : o.settings) ApplySetting(c, key, value);
  // The server's key for the master (§10.1): new each run unless the world keeps it.
  dwell::core::SystemEntropy entropy;
  entropy.Fill(o.server_key);
  if (o.world.empty()) {
    std::puts("world: in memory only (--world \"\"): nothing is saved");
    return true;
  }
  std::string error;
  auto store = dwell::storage::WorldStore::OpenFile(o.world, error);
  if (!store) {
    std::fprintf(stderr, "dwell_server: cannot open world file %s: %s\n", o.world.c_str(),
                 error.c_str());
    return false;
  }
  auto& db = store->db();
  if (auto saved = db.Setting("server_key"); !saved || !ParseHex(*saved, o.server_key)) {
    db.SetSetting("server_key", Hex(o.server_key.data(), o.server_key.size()));
  }
  for (const char* key :
       {"name", "motd", "max_players", "edits", "flight", "autosave_seconds", "allow_list"}) {
    if (auto value = db.Setting(key)) ApplySetting(c, key, *value);
  }
  for (const auto& [key, value] : o.settings) {
    ApplySetting(c, key, value);
    db.SetSetting(key, value);
  }
  for (const auto& [kind, hex] : o.grants) {
    dwell::storage::PermissionEntry e;
    if (!ParseKey(hex, e.key)) {
      std::fprintf(stderr, "dwell_server: not a device public key: %s\n", hex.c_str());
      return false;
    }
    e.kind = kind;
    db.SetPermission(e);
  }
  for (const auto& e : db.Permissions()) {
    switch (e.kind) {
      case dwell::storage::PermissionKind::kOp:
        c.ops.push_back(e.key);
        break;
      case dwell::storage::PermissionKind::kBan:
        c.banned.push_back(e.key);
        break;
      case dwell::storage::PermissionKind::kAllow:
        if (c.allow_list) c.allow_list->push_back(e.key);
        break;
    }
  }
  const auto meta = db.LoadMeta();
  // Worlds are locked to their compatibility line (RELEASES.md §6): refuse one this build may not
  // open, naming the version to run. Nothing was written to it.
  if (meta) {
    const std::string locked =
        dwell::core::WorldVersionError(dwell::core::kAppVersion, meta->app_version_last);
    if (!locked.empty()) {
      std::fprintf(stderr, "dwell_server %s: cannot open world file %s: %s\n",
                   dwell::core::kAppVersion, o.world.c_str(), locked.c_str());
      return false;
    }
  }
  if (meta &&
      (meta->world_seed != c.world_seed || meta->generator_version != c.generator_version)) {
    std::printf("world: %s keeps its own seed %llu and generator %u\n", o.world.c_str(),
                static_cast<unsigned long long>(meta->world_seed), meta->generator_version);
  }
  std::printf("world: %s (%s, %zu modified chunks)\n", o.world.c_str(), meta ? "loaded" : "new",
              db.ChunkIndex().size());
  c.store = std::move(store);
  return true;
}

// This machine's LAN addresses (IPv4 and non-link-local IPv6, no loopback), reported to the master
// so players on the same network can type them (§10.3).
std::vector<std::string> LanAddresses() {
  std::vector<std::string> out;
#if defined(__unix__) || defined(__APPLE__)
  ifaddrs* list = nullptr;
  if (getifaddrs(&list) != 0) return out;
  for (ifaddrs* a = list; a && out.size() < 8; a = a->ifa_next) {
    if (!a->ifa_addr || (a->ifa_flags & IFF_LOOPBACK) || !(a->ifa_flags & IFF_UP)) continue;
    char text[INET6_ADDRSTRLEN] = {};
    if (a->ifa_addr->sa_family == AF_INET) {
      const auto* in = reinterpret_cast<const sockaddr_in*>(a->ifa_addr);
      inet_ntop(AF_INET, &in->sin_addr, text, sizeof text);
    } else if (a->ifa_addr->sa_family == AF_INET6) {
      const auto* in6 = reinterpret_cast<const sockaddr_in6*>(a->ifa_addr);
      if (IN6_IS_ADDR_LINKLOCAL(&in6->sin6_addr)) continue;
      inet_ntop(AF_INET6, &in6->sin6_addr, text, sizeof text);
    } else {
      continue;
    }
    if (text[0] != 0 && std::find(out.begin(), out.end(), text) == out.end())
      out.emplace_back(text);
  }
  freeifaddrs(list);
#endif
  return out;
}

std::string JsonString(const std::string& s) {
  std::string out = "\"";
  for (const char ch : s) {
    const auto c = static_cast<unsigned char>(ch);
    if (c == '"' || c == '\\') {
      out += '\\';
      out += ch;
    } else if (c < 0x20) {
      char buf[8];
      std::snprintf(buf, sizeof buf, "\\u%04x", c);
      out += buf;
    } else {
      out += ch;
    }
  }
  return out + "\"";
}

dwell::protocol::TransportKind ToTransportKind(std::uint8_t v) {
  return v == 2 ? dwell::protocol::TransportKind::kWebRtc
                : dwell::protocol::TransportKind::kWebTransport;
}

}  // namespace

int main(int argc, char** argv) {
  Options options;
  if (!ParseOptions(argc, argv, options)) {
    Usage();
    return 2;
  }

  if (!OpenWorld(options)) return 1;
  dwell::core::JoltRuntime jolt;
  const int workers = std::max(1, static_cast<int>(std::thread::hardware_concurrency()) - 1);
  JPH::JobSystemThreadPool jobs(JPH::cMaxPhysicsJobs, JPH::cMaxPhysicsBarriers, workers);
  dwell::core::SystemEntropy entropy;
  // Terrain generation off the tick (§6.3): about cores − 2 threads beside the physics pool.
  options.server.worldgen_threads =
      std::max(1, static_cast<int>(std::thread::hardware_concurrency()) - 2);
  dwell::core::Server server(options.server, entropy, jobs);

  const DwellNetConfig net_config{
      options.port, options.rtc_port, options.advertise.c_str(),
      static_cast<std::uint32_t>(dwell::protocol::kMaxReliableMessageBytes),
      static_cast<std::uint32_t>(dwell::protocol::kMaxDatagramBytes)};
  DwellNet* net = dwell_net_start(&net_config);
  if (!net) {
    std::fprintf(stderr, "dwell_server: failed to start networking: %s\n", dwell_net_last_error());
    return 1;
  }
  std::uint8_t cert_hash[32];
  dwell_net_cert_hash(net, cert_hash);
  const std::uint16_t port = dwell_net_port(net);
  const std::string hash_hex = Hex(cert_hash, sizeof cert_hash);

  std::printf("dwell_server %s | %s | dwell-net %s | protocol v%u\n", dwell::core::kAppVersion,
              dwell::core::JoltVersionString(), dwell_net_crate_version(),
              dwell::protocol::kProtocolVersion);
  const std::uint16_t rtc_port = dwell_net_rtc_port(net);
  std::printf("listening on UDP %u (WebTransport) and UDP %u (WebRTC)\n", port, rtc_port);
  std::printf("certificate sha-256: %s\n", hash_hex.c_str());
  // IPv6 literals need brackets in host:port.
  const std::string host = options.advertise.find(':') != std::string::npos
                               ? "[" + options.advertise + "]"
                               : options.advertise;
  // `v` is this server's app version: the launcher opens a client build on its compatibility line
  // (RELEASES.md §5).
  std::printf("invite link: %s?join=%s:%u&cert=%s&rtc=%u&ice=%s:%s&v=%s\n",
              options.client_url.c_str(), host.c_str(), port, hash_hex.c_str(), rtc_port,
              dwell_net_ice_ufrag(net), dwell_net_ice_pwd(net), dwell::core::kAppVersion);
  std::fflush(stdout);

  // Registration with the master (§10.1, Phase 5d): a heartbeat now and every heartbeat_s.
  DwellMaster* master = nullptr;
  const std::vector<std::string> lan = LanAddresses();
  if (options.visibility == "none") {
    std::puts("master: not registering (--visibility none)");
  } else if (!(master = dwell_master_start(options.master.c_str(), options.server_key.data()))) {
    std::fprintf(stderr, "dwell_server: master registration unavailable: %s\n",
                 dwell_net_last_error());
  } else {
    std::printf("master: registering with %s as %s\n", options.master.c_str(),
                options.visibility.c_str());
  }
  const auto heartbeat = [&] {
    if (!master) return;
    std::string lan_json;
    for (const auto& a : lan) lan_json += (lan_json.empty() ? "" : ",") + JsonString(a);
    std::string tags_json;
    for (const auto& t : options.tags) tags_json += (tags_json.empty() ? "" : ",") + JsonString(t);
    const auto& c = options.server;
    const std::string body =
        "{\"port\":" + std::to_string(port) + ",\"rtcPort\":" + std::to_string(rtc_port) +
        ",\"ice\":" +
        JsonString(std::string(dwell_net_ice_ufrag(net)) + ":" + dwell_net_ice_pwd(net)) +
        ",\"cert\":" + JsonString(hash_hex) +
        (options.advertise_given ? ",\"advertise\":" + JsonString(options.advertise) : "") +
        ",\"lan\":[" + lan_json + "],\"name\":" + JsonString(c.name) +
        ",\"motd\":" + JsonString(c.motd) +
        ",\"players\":" + std::to_string(server.joined_players()) +
        ",\"maxPlayers\":" + std::to_string(c.max_players) +
        ",\"protocol\":" + std::to_string(dwell::protocol::kProtocolVersion) +
        ",\"appVersion\":" + JsonString(dwell::core::kAppVersion) +
        ",\"visibility\":" + JsonString(options.visibility) + ",\"tags\":[" + tags_json + "]" +
        ",\"heartbeatS\":" + std::to_string(options.heartbeat_s) + "}";
    dwell_master_heartbeat(master, body.c_str());
  };
  heartbeat();
  auto next_heartbeat =
      std::chrono::steady_clock::now() + std::chrono::seconds(options.heartbeat_s);
  std::string shown_code, shown_error;

  std::signal(SIGINT, OnSignal);
  std::signal(SIGTERM, OnSignal);

  auto flush_outbox = [&] {
    for (const auto& out : server.TakeOutbox()) {
      switch (out.kind) {
        case dwell::core::Outgoing::Kind::kReliable:
          dwell_net_send_reliable(net, out.session, static_cast<std::uint8_t>(out.channel),
                                  out.bytes.data(), out.bytes.size());
          break;
        case dwell::core::Outgoing::Kind::kDatagram:
          dwell_net_send_datagram(net, out.session, out.bytes.data(), out.bytes.size());
          break;
        case dwell::core::Outgoing::Kind::kClose:
          dwell_net_close(net, out.session);
          break;
      }
    }
  };

  dwell::core::FixedStep fixed(dwell::protocol::kSimHz);
  auto last = std::chrono::steady_clock::now();
  while (g_running) {
    DwellNetEvent ev;
    while (dwell_net_poll(net, &ev)) {
      switch (ev.kind) {
        case DwellNetEventKind::Connected: {
          dwell::core::TransportBinding binding;
          std::memcpy(binding.data(), ev.binding, binding.size());
          server.OnConnected(ev.session, ToTransportKind(ev.transport), binding);
          break;
        }
        case DwellNetEventKind::Disconnected:
          server.OnDisconnected(ev.session);
          break;
        case DwellNetEventKind::Reliable:
          server.OnReliable(ev.session, static_cast<dwell::protocol::Channel>(ev.channel),
                            {ev.data, ev.len});
          break;
        case DwellNetEventKind::Datagram:
          server.OnDatagram(ev.session, {ev.data, ev.len});
          break;
        case DwellNetEventKind::None:
          break;
      }
    }
    // Replies to requests go out immediately rather than waiting for the next tick.
    flush_outbox();

    const auto now = std::chrono::steady_clock::now();
    const int steps = fixed.Advance(std::chrono::duration<double>(now - last).count());
    last = now;
    for (int i = 0; i < steps; ++i) server.Step();
    flush_outbox();

    if (master) {
      if (now >= next_heartbeat) {
        heartbeat();
        next_heartbeat = now + std::chrono::seconds(options.heartbeat_s);
      }
      // Say once when the join code is known, and whenever registration starts or stops failing.
      if (const std::string code = dwell_master_code(master); code != shown_code && !code.empty()) {
        shown_code = code;
        std::string plain = code;
        plain.erase(std::remove(plain.begin(), plain.end(), '-'), plain.end());
        std::printf("join code: %s (link: %s?code=%s)\n", code.c_str(), options.client_url.c_str(),
                    plain.c_str());
        std::fflush(stdout);
      }
      if (const std::string error = dwell_master_error(master); error != shown_error) {
        shown_error = error;
        if (!error.empty()) std::fprintf(stderr, "master: %s\n", error.c_str());
      }
    }

    // Wake for the next step, and at least every 2 ms to keep request latency low.
    const double sleep_s = std::min(fixed.TimeUntilNextStep(), 0.002);
    std::this_thread::sleep_for(std::chrono::duration<double>(sleep_s));
  }

  std::puts("dwell_server: shutting down");
  // Tell joined players why (Reject(ServerClosing)); best effort before the transport stops.
  server.CloseSessions("The server is shutting down.", /*keep_loopback=*/false);
  flush_outbox();
  // Leave the master's listings at once rather than after two missed heartbeats.
  if (master) dwell_master_stop(master, /*leave=*/true);
  dwell_net_stop(net);
  if (options.server.store) {
    server.SaveNow();
    options.server.store->Flush();
    server.Step();  // collects the save's result
    const auto stats = server.save_stats();
    if (stats.failed > 0) {
      std::fprintf(stderr, "dwell_server: saving failed: %s\n", stats.last_error.c_str());
      return 1;
    }
    std::puts("world saved");
  }
  return 0;
}
