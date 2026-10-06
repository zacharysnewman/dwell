// The app version (RELEASES.md §3, §6): SemVer precedence and the compatibility lines that lock
// worlds to builds, checked against the vectors the TypeScript implementation (client/src/version)
// is tested with, plus the world file recording the version and the handshake naming it.
#include <doctest/doctest.h>

#include <filesystem>
#include <fstream>
#include <sstream>
#include <string>
#include <vector>

#include "dwell/core/app_version.h"
#include "dwell/storage/world_db.h"

using namespace dwell::core;

namespace {

std::vector<std::vector<std::string>> Vectors(const std::string& kind) {
  std::ifstream in(DWELL_VERSION_VECTORS);
  REQUIRE(in.good());
  std::vector<std::vector<std::string>> out;
  std::string line;
  while (std::getline(in, line)) {
    if (line.empty() || line[0] == '#') continue;
    std::istringstream ls(line);
    std::string word;
    std::vector<std::string> words;
    while (ls >> word) words.push_back(word);
    if (!words.empty() && words[0] == kind) out.emplace_back(words.begin() + 1, words.end());
  }
  return out;
}

}  // namespace

TEST_CASE("version: precedence follows SemVer 2.0.0 (shared vectors)") {
  const auto vectors = Vectors("compare");
  REQUIRE(!vectors.empty());
  for (const auto& v : vectors) {
    INFO(v[0], " vs ", v[1]);
    const auto a = ParseVersion(v[0]);
    const auto b = ParseVersion(v[1]);
    REQUIRE(a);
    REQUIRE(b);
    CHECK(CompareVersions(*a, *b) == std::stoi(v[2]));
  }
}

TEST_CASE("version: compatibility lines (shared vectors)") {
  const auto vectors = Vectors("line");
  REQUIRE(!vectors.empty());
  for (const auto& v : vectors) {
    INFO(v[0]);
    CHECK(CompatibilityLine(v[0]) == (v[1] == "-" ? "" : v[1]));
  }
}

TEST_CASE("version: which builds open which worlds (shared vectors)") {
  const auto vectors = Vectors("open");
  REQUIRE(!vectors.empty());
  for (const auto& v : vectors) {
    INFO("build ", v[0], " world ", v[1]);
    CHECK(CanOpenWorld(v[0], v[1]) == (v[2] == "YES"));
  }
}

TEST_CASE("version: invalid versions are rejected (shared vectors)") {
  std::ifstream in(DWELL_VERSION_VECTORS);
  REQUIRE(in.good());
  std::string line;
  int checked = 0;
  while (std::getline(in, line)) {
    if (line.rfind("invalid", 0) != 0) continue;
    const std::string text = line.size() > 8 ? line.substr(8) : "";
    INFO("'", text, "'");
    CHECK_FALSE(ParseVersion(text));
    ++checked;
  }
  CHECK(checked > 0);
}

TEST_CASE("version: this build's version is valid and has no build metadata") {
  REQUIRE(ParseVersion(kAppVersion));
  CHECK(std::string(kAppVersion).find('+') == std::string::npos);
  CHECK(CanOpenWorld(kAppVersion, kAppVersion));
}

TEST_CASE("version: the error names the version to use") {
  CHECK(WorldVersionError("0.1.1", "0.1.0").empty());
  const std::string newer = WorldVersionError("0.1.0", "0.1.1");
  CHECK(newer.find("0.1.1") != std::string::npos);
  CHECK(newer.find("newer") != std::string::npos);
  const std::string other_line = WorldVersionError("0.2.0", "0.1.1");
  CHECK(other_line.find("0.1.1") != std::string::npos);
  CHECK(other_line.find("line 0.1") != std::string::npos);
  CHECK(WorldVersionError("0.1.0", "").find("before versioned releases") != std::string::npos);
  CHECK_FALSE(WorldVersionError("0.2.0-dev.1", "0.2.0-dev.2").empty());
}

TEST_CASE("version: a save records the build's version, and keeps the creating one") {
  namespace fs = std::filesystem;
  const fs::path path = fs::temp_directory_path() / "dwell-version-tests.dwellworld";
  for (const char* suffix : {"", "-journal", "-wal", "-shm"}) fs::remove(path.string() + suffix);
  std::string error;
  {
    auto db = dwell::storage::WorldDb::Open(path.string(), error, {nullptr, false});
    REQUIRE(db);
    CHECK_FALSE(db->LoadMeta());
    dwell::storage::SaveBatch batch;
    batch.meta = dwell::storage::WorldMeta{5, 4, std::nullopt, 1};
    batch.meta->app_version_created = batch.meta->app_version_last = "0.1.0";
    REQUIRE(db->Save(batch, error));
    // A later compatible build saves: the last version moves, the first stays.
    batch.meta->app_version_created = batch.meta->app_version_last = "0.1.3";
    REQUIRE(db->Save(batch, error));
  }
  auto db = dwell::storage::WorldDb::Open(path.string(), error, {nullptr, false});
  REQUIRE(db);
  const auto meta = db->LoadMeta();
  REQUIRE(meta);
  CHECK(meta->app_version_created == "0.1.0");
  CHECK(meta->app_version_last == "0.1.3");
}

TEST_CASE("version: a world saved without a version reads as unversioned") {
  namespace fs = std::filesystem;
  const fs::path path = fs::temp_directory_path() / "dwell-version-legacy.dwellworld";
  for (const char* suffix : {"", "-journal", "-wal", "-shm"}) fs::remove(path.string() + suffix);
  std::string error;
  auto db = dwell::storage::WorldDb::Open(path.string(), error, {nullptr, false});
  REQUIRE(db);
  dwell::storage::SaveBatch batch;
  batch.meta =
      dwell::storage::WorldMeta{5, 4, std::nullopt, 1};  // as a pre-baseline build wrote it
  REQUIRE(db->Save(batch, error));
  const auto meta = db->LoadMeta();
  REQUIRE(meta);
  CHECK(meta->app_version_last.empty());
  CHECK_FALSE(WorldVersionError(kAppVersion, meta->app_version_last).empty());
}
