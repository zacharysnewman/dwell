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

#include "dwell/core/fixed_step.h"
#include "dwell/core/jolt_runtime.h"
#include "dwell/core/server.h"
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
};

void Usage() {
  std::puts(
      "usage: dwell_server [--port N] [--rtc-port N] [--advertise IP] [--name NAME] [--motd TEXT]\n"
      "                    [--max-players N] [--seed N] [--generator N] [--client-url URL]\n"
      "  --advertise IP  address players use to reach this server (invite links, WebRTC)\n"
      "  --generator N   world generator: 2 = procedural terrain (default), 1 = movement\n"
      "                  playground, 0 = flat");
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
    } else if (arg == "--name") {
      o.server.name = v;
    } else if (arg == "--motd") {
      o.server.motd = v;
    } else if (arg == "--max-players") {
      o.server.max_players = static_cast<std::uint16_t>(std::strtoul(v, nullptr, 10));
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

  dwell::core::JoltRuntime jolt;
  const int workers = std::max(1, static_cast<int>(std::thread::hardware_concurrency()) - 1);
  JPH::JobSystemThreadPool jobs(JPH::cMaxPhysicsJobs, JPH::cMaxPhysicsBarriers, workers);
  dwell::core::SystemEntropy entropy;
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
  dwell_net_stop(net);
  return 0;
}
