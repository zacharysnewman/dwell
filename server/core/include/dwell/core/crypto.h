#pragma once

#include <array>
#include <cstdint>
#include <span>

namespace dwell::core {

// Verifies an RFC 8032 Ed25519 signature (the format WebCrypto produces). ADR 0004.
bool VerifyEd25519(const std::array<std::uint8_t, 64>& signature,
                   const std::array<std::uint8_t, 32>& public_key,
                   std::span<const std::uint8_t> message);

}  // namespace dwell::core
