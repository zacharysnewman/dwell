#include "dwell/protocol/bytes.h"

namespace dwell::protocol {

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
