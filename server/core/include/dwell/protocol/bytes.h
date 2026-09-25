#pragma once

#include <array>
#include <bit>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <span>
#include <string>
#include <string_view>
#include <vector>

namespace dwell::protocol {

// IEEE 754 binary16 conversions (round to nearest even; overflow → ±inf; NaN kept).
std::uint16_t FloatToHalf(float f);
float HalfToFloat(std::uint16_t h);

// Little-endian byte writer for protocol messages (ARCHITECTURE.md §8.2: all fields little-endian).
class ByteWriter {
 public:
  explicit ByteWriter(std::vector<std::uint8_t>& out) : out_(out) {}

  void U8(std::uint8_t v) { out_.push_back(v); }
  void U16(std::uint16_t v) { Le(v); }
  void U32(std::uint32_t v) { Le(v); }
  void U64(std::uint64_t v) { Le(v); }
  void F64(double v) { Le(std::bit_cast<std::uint64_t>(v)); }
  void I8(std::int8_t v) { U8(static_cast<std::uint8_t>(v)); }
  void I16(std::int16_t v) { U16(static_cast<std::uint16_t>(v)); }
  void I32(std::int32_t v) { U32(static_cast<std::uint32_t>(v)); }
  void F32(float v) { Le(std::bit_cast<std::uint32_t>(v)); }
  void F16(float v) { U16(FloatToHalf(v)); }
  void Bytes(std::span<const std::uint8_t> b) { out_.insert(out_.end(), b.begin(), b.end()); }
  // UTF-8 string: u16 byte length, then bytes. Callers enforce per-field limits before encoding.
  void Str(std::string_view s) {
    U16(static_cast<std::uint16_t>(s.size()));
    out_.insert(out_.end(), s.begin(), s.end());
  }

 private:
  template <typename T>
  void Le(T v) {
    for (std::size_t i = 0; i < sizeof(T); ++i)
      out_.push_back(static_cast<std::uint8_t>(v >> (8 * i)));
  }
  std::vector<std::uint8_t>& out_;
};

// Bounds-checked little-endian reader. Any failure latches `ok() == false`; reads after a failure
// return zero values, so decoders can read all fields and check once at the end.
class ByteReader {
 public:
  explicit ByteReader(std::span<const std::uint8_t> in) : in_(in) {}

  std::uint8_t U8() { return Le<std::uint8_t>(); }
  std::uint16_t U16() { return Le<std::uint16_t>(); }
  std::uint32_t U32() { return Le<std::uint32_t>(); }
  std::uint64_t U64() { return Le<std::uint64_t>(); }
  double F64() { return std::bit_cast<double>(Le<std::uint64_t>()); }
  std::int8_t I8() { return static_cast<std::int8_t>(U8()); }
  std::int16_t I16() { return static_cast<std::int16_t>(U16()); }
  std::int32_t I32() { return static_cast<std::int32_t>(U32()); }
  float F32() { return std::bit_cast<float>(Le<std::uint32_t>()); }
  float F16() { return HalfToFloat(U16()); }
  // Fails the read (like truncation) when `condition` is false; for range checks in decoders.
  void Check(bool condition) {
    if (!condition) ok_ = false;
  }

  template <std::size_t N>
  std::array<std::uint8_t, N> Fixed() {
    std::array<std::uint8_t, N> out{};
    if (Need(N)) {
      std::memcpy(out.data(), in_.data() + pos_, N);
      pos_ += N;
    }
    return out;
  }

  // Reads a u16-length-prefixed UTF-8 string of at most `max_bytes`; invalid UTF-8 fails.
  std::string Str(std::size_t max_bytes);

  bool ok() const { return ok_; }
  bool AtEnd() const { return ok_ && pos_ == in_.size(); }

 private:
  bool Need(std::size_t n) {
    if (!ok_ || in_.size() - pos_ < n) {
      ok_ = false;
      return false;
    }
    return true;
  }
  template <typename T>
  T Le() {
    if (!Need(sizeof(T))) return T{};
    T v = 0;
    for (std::size_t i = 0; i < sizeof(T); ++i) v |= static_cast<T>(T(in_[pos_ + i]) << (8 * i));
    pos_ += sizeof(T);
    return v;
  }

  std::span<const std::uint8_t> in_;
  std::size_t pos_ = 0;
  bool ok_ = true;
};

bool IsValidUtf8(std::string_view s);

}  // namespace dwell::protocol
