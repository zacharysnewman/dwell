#include "dwell/protocol/bytes.h"

namespace dwell::protocol {

// Round-to-nearest-even conversion after F. Giesen's float_to_half_fast3_rtne.
std::uint16_t FloatToHalf(float f) {
  std::uint32_t x = std::bit_cast<std::uint32_t>(f);
  const std::uint32_t sign = x & 0x80000000u;
  x ^= sign;
  std::uint32_t out;
  if (x >= 0x47800000u) {  // ≥ 65536, inf or NaN
    out = x > 0x7F800000u ? 0x7E00u : 0x7C00u;
  } else if (x < 0x38800000u) {  // subnormal half or zero: let float addition round
    const float magic = std::bit_cast<float>(0x3F000000u);  // 0.5
    out = std::bit_cast<std::uint32_t>(std::bit_cast<float>(x) + magic) - 0x3F000000u;
  } else {
    const std::uint32_t mantissa_odd = (x >> 13) & 1u;
    x += 0xC8000FFFu;  // rebias the exponent (15 − 127) and round
    x += mantissa_odd;
    out = x >> 13;
  }
  return static_cast<std::uint16_t>(out | (sign >> 16));
}

float HalfToFloat(std::uint16_t h) {
  const std::uint32_t sign = static_cast<std::uint32_t>(h & 0x8000u) << 16;
  const std::uint32_t exponent = (h >> 10) & 0x1Fu;
  const std::uint32_t mantissa = h & 0x3FFu;
  if (exponent == 0) {
    const float value = static_cast<float>(mantissa) * (1.0f / 16777216.0f);  // 2^-24
    return std::bit_cast<float>(std::bit_cast<std::uint32_t>(value) | sign);
  }
  if (exponent == 31) return std::bit_cast<float>(sign | 0x7F800000u | (mantissa << 13));
  return std::bit_cast<float>(sign | ((exponent + 112) << 23) | (mantissa << 13));
}

bool IsValidUtf8(std::string_view s) {
  std::size_t i = 0;
  const auto n = s.size();
  while (i < n) {
    const auto c = static_cast<unsigned char>(s[i]);
    std::size_t len;
    std::uint32_t cp;
    if (c < 0x80) {
      ++i;
      continue;
    } else if ((c & 0xE0) == 0xC0) {
      len = 2;
      cp = c & 0x1F;
    } else if ((c & 0xF0) == 0xE0) {
      len = 3;
      cp = c & 0x0F;
    } else if ((c & 0xF8) == 0xF0) {
      len = 4;
      cp = c & 0x07;
    } else {
      return false;
    }
    if (n - i < len) return false;
    for (std::size_t k = 1; k < len; ++k) {
      const auto cc = static_cast<unsigned char>(s[i + k]);
      if ((cc & 0xC0) != 0x80) return false;
      cp = (cp << 6) | (cc & 0x3F);
    }
    // Reject overlong encodings, surrogates, and out-of-range code points.
    if ((len == 2 && cp < 0x80) || (len == 3 && cp < 0x800) || (len == 4 && cp < 0x10000) ||
        (cp >= 0xD800 && cp <= 0xDFFF) || cp > 0x10FFFF) {
      return false;
    }
    i += len;
  }
  return true;
}

std::string ByteReader::Str(std::size_t max_bytes) {
  const std::uint16_t len = U16();
  if (!ok_ || len > max_bytes || !Need(len)) {
    ok_ = false;
    return {};
  }
  std::string s(reinterpret_cast<const char*>(in_.data() + pos_), len);
  pos_ += len;
  if (!IsValidUtf8(s)) {
    ok_ = false;
    return {};
  }
  return s;
}

}  // namespace dwell::protocol
