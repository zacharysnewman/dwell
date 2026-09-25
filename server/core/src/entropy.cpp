#include "dwell/core/entropy.h"

#include <random>

namespace dwell::core {

void SystemEntropy::Fill(std::span<std::uint8_t> out) {
  std::random_device rd;
  for (std::size_t i = 0; i < out.size(); i += 4) {
    const auto v = rd();
    for (std::size_t k = 0; k < 4 && i + k < out.size(); ++k) {
      out[i + k] = static_cast<std::uint8_t>(v >> (8 * k));
    }
  }
}

}  // namespace dwell::core
