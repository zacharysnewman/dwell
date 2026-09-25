#include "dwell/core/crypto.h"

#include <monocypher-ed25519.h>

namespace dwell::core {

bool VerifyEd25519(const std::array<std::uint8_t, 64>& signature,
                   const std::array<std::uint8_t, 32>& public_key,
                   std::span<const std::uint8_t> message) {
  return crypto_ed25519_check(signature.data(), public_key.data(), message.data(),
                              message.size()) == 0;
}

}  // namespace dwell::core
