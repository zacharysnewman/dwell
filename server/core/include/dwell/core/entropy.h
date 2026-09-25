#pragma once

#include <cstdint>
#include <span>

namespace dwell::core {

// Source of unpredictable bytes (e.g. handshake nonces), provided by the host so the core makes no
// OS calls itself (ARCHITECTURE.md §4.3).
class Entropy {
 public:
  virtual ~Entropy() = default;
  virtual void Fill(std::span<std::uint8_t> out) = 0;
};

// std::random_device-backed entropy: getrandom()/urandom natively, crypto.getRandomValues() under
// Emscripten.
class SystemEntropy final : public Entropy {
 public:
  void Fill(std::span<std::uint8_t> out) override;
};

}  // namespace dwell::core
