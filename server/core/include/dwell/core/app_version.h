#pragma once

#include <cstdint>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

// The app version (RELEASES.md §3, §6): Semantic Versioning 2.0.0 and the compatibility lines that
// lock worlds to builds. Mirrors client/src/version/semver.ts; both are checked against
// shared/version/vectors.txt.
namespace dwell::core {

// The version this build carries, without build metadata: "0.1.0" for a release,
// "0.2.0-dev.42" for a dev build. Set at configure time (server/CMakeLists.txt) from
// client/package.json or DWELL_VERSION, so the native server and the WASM core agree with the
// client they ship with.
extern const char* const kAppVersion;

struct Version {
  std::uint64_t major = 0, minor = 0, patch = 0;
  std::vector<std::string> pre;  // pre-release identifiers; empty for a release
};

// Strict SemVer 2.0.0 (no leading "v", no missing parts); build metadata is accepted and dropped.
std::optional<Version> ParseVersion(std::string_view text);

// Precedence of a against b: -1, 0 or 1 (build metadata ignored).
int CompareVersions(const Version& a, const Version& b);

// The compatibility line: "0.1" before 1.0.0, MAJOR from it, the exact version for a pre-release.
// Empty for an invalid version.
std::string CompatibilityLine(std::string_view version);

// Whether a build may open a world last saved by `world_last`: the same line, and not older.
bool CanOpenWorld(std::string_view build, std::string_view world_last);

// Why `build` may not open a world last saved by `world_last` ("" if it may): a message for the
// player naming the version to use. An empty `world_last` is a world saved before versioned
// releases, which no build opens.
std::string WorldVersionError(std::string_view build, std::string_view world_last);

}  // namespace dwell::core
