// Image output for the debug tools: PNG (stored deflate blocks, no compression library) or PPM,
// by the file's extension. PNGs open anywhere and can be attached to a PR as they are.
#pragma once

#include <algorithm>
#include <cstdint>
#include <cstdio>
#include <string>
#include <vector>

namespace dwell::tools {

namespace detail {
inline std::uint32_t Crc32(const std::uint8_t* d, std::size_t n, std::uint32_t crc = 0) {
  static const auto table = [] {
    std::vector<std::uint32_t> t(256);
    for (std::uint32_t i = 0; i < 256; ++i) {
      std::uint32_t c = i;
      for (int k = 0; k < 8; ++k) c = (c & 1u) ? 0xEDB88320u ^ (c >> 1) : c >> 1;
      t[i] = c;
    }
    return t;
  }();
  crc = ~crc;
  for (std::size_t i = 0; i < n; ++i) crc = table[(crc ^ d[i]) & 0xFFu] ^ (crc >> 8);
  return ~crc;
}
inline void Put32(std::vector<std::uint8_t>& v, std::uint32_t x) {
  for (int s = 24; s >= 0; s -= 8) v.push_back(static_cast<std::uint8_t>(x >> s));
}
inline void Chunk(std::FILE* f, const char* type, const std::vector<std::uint8_t>& data) {
  std::vector<std::uint8_t> out;
  Put32(out, static_cast<std::uint32_t>(data.size()));
  out.insert(out.end(), type, type + 4);
  out.insert(out.end(), data.begin(), data.end());
  Put32(out, Crc32(out.data() + 4, out.size() - 4));
  std::fwrite(out.data(), 1, out.size(), f);
}
}  // namespace detail

// Writes `rgb` (w × h × 3, row-major, top row first). False if the file cannot be written.
inline bool WriteImage(const std::string& path, int w, int h, const std::vector<std::uint8_t>& rgb) {
  std::FILE* f = std::fopen(path.c_str(), "wb");
  if (!f) return false;
  const bool png = path.size() > 4 && path.compare(path.size() - 4, 4, ".png") == 0;
  if (!png) {
    std::fprintf(f, "P6\n%d %d\n255\n", w, h);
    std::fwrite(rgb.data(), 1, rgb.size(), f);
    std::fclose(f);
    return true;
  }
  static const std::uint8_t kSig[8] = {0x89, 'P', 'N', 'G', 0x0D, 0x0A, 0x1A, 0x0A};
  std::fwrite(kSig, 1, 8, f);
  std::vector<std::uint8_t> ihdr;
  detail::Put32(ihdr, static_cast<std::uint32_t>(w));
  detail::Put32(ihdr, static_cast<std::uint32_t>(h));
  ihdr.insert(ihdr.end(), {8, 2, 0, 0, 0});  // 8-bit RGB
  detail::Chunk(f, "IHDR", ihdr);
  // Scanlines with filter byte 0, in a zlib stream of stored blocks.
  std::vector<std::uint8_t> raw;
  raw.reserve(static_cast<std::size_t>(h) * (static_cast<std::size_t>(w) * 3 + 1));
  for (int y = 0; y < h; ++y) {
    raw.push_back(0);
    raw.insert(raw.end(), rgb.begin() + static_cast<std::ptrdiff_t>(y) * w * 3,
               rgb.begin() + static_cast<std::ptrdiff_t>(y + 1) * w * 3);
  }
  std::vector<std::uint8_t> z{0x78, 0x01};
  std::uint32_t a = 1, b = 0;
  for (const std::uint8_t c : raw) {
    a = (a + c) % 65521u;
    b = (b + a) % 65521u;
  }
  for (std::size_t pos = 0; pos < raw.size() || pos == 0;) {
    const std::size_t n = std::min<std::size_t>(65535, raw.size() - pos);
    const bool last = pos + n >= raw.size();
    z.push_back(last ? 1 : 0);
    z.push_back(static_cast<std::uint8_t>(n));
    z.push_back(static_cast<std::uint8_t>(n >> 8));
    z.push_back(static_cast<std::uint8_t>(~n));
    z.push_back(static_cast<std::uint8_t>((~n) >> 8));
    z.insert(z.end(), raw.begin() + static_cast<std::ptrdiff_t>(pos),
             raw.begin() + static_cast<std::ptrdiff_t>(pos + n));
    pos += n;
    if (last) break;
  }
  detail::Put32(z, (b << 16) | a);
  detail::Chunk(f, "IDAT", z);
  detail::Chunk(f, "IEND", {});
  std::fclose(f);
  return true;
}

}  // namespace dwell::tools
