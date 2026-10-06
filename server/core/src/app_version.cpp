#include "dwell/core/app_version.h"

#include <algorithm>
#include <cctype>
#include <charconv>

namespace dwell::core {
namespace {

bool AllDigits(std::string_view s) {
  return !s.empty() && std::all_of(s.begin(), s.end(), [](char c) { return c >= '0' && c <= '9'; });
}

bool IsIdentChar(char c) { return std::isalnum(static_cast<unsigned char>(c)) || c == '-'; }

bool ParseNumber(std::string_view s, std::uint64_t& out) {
  if (!AllDigits(s) || (s.size() > 1 && s[0] == '0')) return false;
  const auto [end, ec] = std::from_chars(s.data(), s.data() + s.size(), out);
  return ec == std::errc{} && end == s.data() + s.size();
}

std::vector<std::string_view> Split(std::string_view s, char sep) {
  std::vector<std::string_view> out;
  std::size_t from = 0;
  for (;;) {
    const std::size_t at = s.find(sep, from);
    out.push_back(s.substr(from, at == std::string_view::npos ? at : at - from));
    if (at == std::string_view::npos) return out;
    from = at + 1;
  }
}

int ComparePre(const std::vector<std::string>& a, const std::vector<std::string>& b) {
  // A release outranks its pre-releases (SemVer §11.3).
  if (a.empty() || b.empty()) return a.empty() ? (b.empty() ? 0 : 1) : -1;
  for (std::size_t i = 0; i < std::max(a.size(), b.size()); ++i) {
    if (i >= a.size()) return -1;
    if (i >= b.size()) return 1;
    const bool an = AllDigits(a[i]), bn = AllDigits(b[i]);
    if (an && bn) {
      // Numeric identifiers: by value (no leading zeros, so by length, then text).
      if (a[i].size() != b[i].size()) return a[i].size() < b[i].size() ? -1 : 1;
      if (a[i] != b[i]) return a[i] < b[i] ? -1 : 1;
    } else if (an != bn) {
      return an ? -1 : 1;  // numeric identifiers rank below alphanumeric ones
    } else if (a[i] != b[i]) {
      return a[i] < b[i] ? -1 : 1;
    }
  }
  return 0;
}

}  // namespace

std::optional<Version> ParseVersion(std::string_view text) {
  std::string_view build;
  if (const std::size_t plus = text.find('+'); plus != std::string_view::npos) {
    build = text.substr(plus + 1);
    text = text.substr(0, plus);
    for (const auto id : Split(build, '.')) {
      if (id.empty() || !std::all_of(id.begin(), id.end(), IsIdentChar)) return std::nullopt;
    }
  }
  std::string_view pre;
  bool has_pre = false;
  if (const std::size_t dash = text.find('-'); dash != std::string_view::npos) {
    pre = text.substr(dash + 1);
    text = text.substr(0, dash);
    has_pre = true;
  }
  const auto parts = Split(text, '.');
  Version v;
  if (parts.size() != 3 || !ParseNumber(parts[0], v.major) || !ParseNumber(parts[1], v.minor) ||
      !ParseNumber(parts[2], v.patch)) {
    return std::nullopt;
  }
  if (has_pre) {
    for (const auto id : Split(pre, '.')) {
      if (id.empty() || !std::all_of(id.begin(), id.end(), IsIdentChar)) return std::nullopt;
      if (AllDigits(id) && id.size() > 1 && id[0] == '0') return std::nullopt;
      v.pre.emplace_back(id);
    }
  }
  return v;
}

int CompareVersions(const Version& a, const Version& b) {
  if (a.major != b.major) return a.major < b.major ? -1 : 1;
  if (a.minor != b.minor) return a.minor < b.minor ? -1 : 1;
  if (a.patch != b.patch) return a.patch < b.patch ? -1 : 1;
  return ComparePre(a.pre, b.pre);
}

std::string CompatibilityLine(std::string_view version) {
  const auto v = ParseVersion(version);
  if (!v) return {};
  if (!v->pre.empty()) {
    std::string line = std::to_string(v->major) + "." + std::to_string(v->minor) + "." +
                       std::to_string(v->patch) + "-";
    for (std::size_t i = 0; i < v->pre.size(); ++i) line += (i ? "." : "") + v->pre[i];
    return line;
  }
  return v->major == 0 ? "0." + std::to_string(v->minor) : std::to_string(v->major);
}

bool CanOpenWorld(std::string_view build, std::string_view world_last) {
  const auto b = ParseVersion(build);
  const auto w = ParseVersion(world_last);
  if (!b || !w) return false;
  return CompatibilityLine(build) == CompatibilityLine(world_last) && CompareVersions(*b, *w) >= 0;
}

std::string WorldVersionError(std::string_view build, std::string_view world_last) {
  if (world_last.empty()) {
    return "this world was saved before versioned releases and can't be opened by any version";
  }
  if (CanOpenWorld(build, world_last)) return {};
  const std::string line = CompatibilityLine(world_last);
  if (line.empty())
    return "this world records an unreadable version (" + std::string(world_last) + ")";
  if (line == CompatibilityLine(build)) {
    return "this world was last saved by Dwell " + std::string(world_last) +
           ", newer than this build (" + std::string(build) + "); run Dwell " +
           std::string(world_last) + " or later on the " + line + " line";
  }
  return "this world was saved by Dwell " + std::string(world_last) + " (line " + line +
         "), which this build (" + std::string(build) +
         ") can't open; run a Dwell build on that line";
}

}  // namespace dwell::core
