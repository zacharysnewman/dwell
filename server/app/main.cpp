// Dwell dedicated server (ARCHITECTURE.md §4, §10.1). Hosts the simulation core behind the Rust
// network front-end: transport events are drained into the core, the core steps at SIM_HZ, and its
// outbox is sent back out.
#include <Jolt/Jolt.h>

#include <Jolt/Core/JobSystemThreadPool.h>

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

#include "dwell/core/fixed_step.h"
#include "dwell/core/jolt_runtime.h"
#include "dwell/core/server.h"
#include "dwell/storage/world_store.h"
#include "dwell_net.h"

namespace {

std::atomic<bool> g_running{true};

void OnSignal(int) { g_running = false; }

struct Options {
  std::uint16_t port = 4433;
  std::uint16_t rtc_port = 0;  // 0 = port + 1
  std::string advertise = "127.0.0.1";
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
      "                    [--client-url URL]\n"
      "  --world FILE    the world file (default world.dwellworld; created if missing; \"\" keeps\n"
      "                  the world in memory only)\n"
      "  --advertise IP  address players use to reach this server (invite links, WebRTC)\n"
      "  --seed N, --generator N  a new world's seed and generator: 4 = procedural terrain\n"
      "                  (default), 1 = movement playground, 0 = flat (a saved world keeps its "
      "own)\n"
      "  --name, --motd, --max-players, --edits everyone|ops|nobody, --flight everyone|ops|nobody\n"
      "                  saved in the world's settings (--flight: who may use creative flight)\n"
      "  --op KEY, --ban KEY  grant op or ban a player (hex device public key), saved in the "
      "world");
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

  std::printf("dwell_server | %s | dwell-net %s | protocol v%u\n", dwell::core::JoltVersionString(),
              dwell_net_crate_version(), dwell::protocol::kProtocolVersion);
  const std::uint16_t rtc_port = dwell_net_rtc_port(net);
  std::printf("listening on UDP %u (WebTransport) and UDP %u (WebRTC)\n", port, rtc_port);
  std::printf("certificate sha-256: %s\n", hash_hex.c_str());
  // IPv6 literals need brackets in host:port.
  const std::string host = options.advertise.find(':') != std::string::npos
                               ? "[" + options.advertise + "]"
                               : options.advertise;
  std::printf("invite link: %s?join=%s:%u&cert=%s&rtc=%u&ice=%s:%s\n", options.client_url.c_str(),
              host.c_str(), port, hash_hex.c_str(), rtc_port, dwell_net_ice_ufrag(net),
              dwell_net_ice_pwd(net));
  std::fflush(stdout);

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

    // Wake for the next step, and at least every 2 ms to keep request latency low.
    const double sleep_s = std::min(fixed.TimeUntilNextStep(), 0.002);
    std::this_thread::sleep_for(std::chrono::duration<double>(sleep_s));
  }

  std::puts("dwell_server: shutting down");
  // Tell joined players why (Reject(ServerClosing)); best effort before the transport stops.
  server.CloseSessions("The server is shutting down.", /*keep_loopback=*/false);
  flush_outbox();
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
