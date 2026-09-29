// World file tool (Phase 3e debug tooling, ARCHITECTURE.md §6.4):
//   dwell_world FILE info                 format, meta, settings, permissions, counts, integrity
//   dwell_world FILE diff [cx cy cz]      regenerate saved chunks from the world's seed and
//                                         generator and diff them: voxels changed per chunk (all
//                                         chunks), or each changed voxel of one chunk
// A saved chunk identical to its generated self is flagged: it need not be stored.
#include <cstdio>
#include <cstdlib>
#include <string>
#include <string_view>

#include "dwell/core/voxel.h"
#include "dwell/storage/world_db.h"

using namespace dwell;

namespace {

int Usage() {
  std::fputs("usage: dwell_world FILE info | diff [cx cy cz]\n", stderr);
  return 2;
}

std::string Hex(const protocol::PublicKey& key) {
  static constexpr char kDigits[] = "0123456789abcdef";
  std::string out;
  for (std::uint8_t b : key) {
    out += kDigits[b >> 4];
    out += kDigits[b & 15];
  }
  return out;
}

core::Chunk FromVoxels(const std::vector<std::uint16_t>& voxels) {
  core::Chunk c;
  std::copy(voxels.begin(), voxels.end(), c.generation_voxels().begin());
  return c;
}

}  // namespace

int main(int argc, char** argv) {
  if (argc < 3) return Usage();
  const std::string path = argv[1];
  const std::string_view command = argv[2];
  std::string error;
  auto db = storage::WorldDb::Open(path, error);
  if (!db) {
    std::fprintf(stderr, "dwell_world: %s: %s\n", path.c_str(), error.c_str());
    return 1;
  }
  const auto meta = db->LoadMeta();
  if (command == "info") {
    std::printf("format %d, integrity %s\n", db->format_version(), db->IntegrityCheck().c_str());
    if (!meta) {
      std::puts("no world saved yet");
      return 0;
    }
    std::printf("seed %llu, generator %u, world tick %u, created %lld, saved %lld\n",
                static_cast<unsigned long long>(meta->world_seed), meta->generator_version,
                meta->world_tick, static_cast<long long>(meta->created_at),
                static_cast<long long>(meta->saved_at));
    if (meta->spawn) {
      std::printf("spawn %.3f %.3f %.3f\n", (*meta->spawn)[0], (*meta->spawn)[1],
                  (*meta->spawn)[2]);
    }
    for (const char* key : {"name", "motd", "max_players", "edits", "autosave_seconds"}) {
      if (auto v = db->Setting(key)) std::printf("setting %s = %s\n", key, v->c_str());
    }
    for (const auto& p : db->Permissions()) {
      const char* kind = p.kind == storage::PermissionKind::kOp    ? "op"
                         : p.kind == storage::PermissionKind::kBan ? "ban"
                                                                   : "allow";
      std::printf("%s %s %s\n", kind, Hex(p.key).c_str(), p.reason.c_str());
    }
    std::printf("%zu modified chunks\n", db->ChunkIndex().size());
    return 0;
  }
  if (command != "diff" || !meta) return meta ? Usage() : (std::puts("no world saved yet"), 0);

  const core::ChunkGenerator generate =
      core::GeneratorFor(meta->generator_version, meta->world_seed);
  auto regenerate = [&](const core::ChunkCoord& c) {
    core::Chunk chunk;
    generate(c, chunk);
    return chunk;
  };
  if (argc >= 6) {
    const core::ChunkCoord c{std::atoi(argv[3]), std::atoi(argv[4]), std::atoi(argv[5])};
    const auto saved = db->LoadChunk(c);
    if (!saved) {
      std::printf("chunk %d %d %d is not saved (it is exactly as generated)\n", c.x, c.y, c.z);
      return 0;
    }
    const auto diff = core::DiffChunk(regenerate(c), FromVoxels(saved->voxels));
    std::printf("chunk %d %d %d, revision %u: %zu voxels differ from generation\n", c.x, c.y, c.z,
                saved->revision, diff.size());
    for (const auto& d : diff) {
      const int x = c.x * core::kChunkSize + (d.index & 31);
      const int y = c.y * core::kChunkSize + ((d.index >> 5) & 31);
      const int z = c.z * core::kChunkSize + ((d.index >> 10) & 31);
      std::printf("  %d %d %d: %s -> %s\n", x, y, z,
                  std::string(core::GetMaterial(d.generated).name).c_str(),
                  std::string(core::GetMaterial(d.current).name).c_str());
    }
    return 0;
  }
  std::size_t total = 0, unchanged = 0;
  for (const auto& [c, revision] : db->ChunkIndex()) {
    const auto saved = db->LoadChunk(c);
    if (!saved) {
      std::printf("chunk %d %d %d: unreadable\n", c.x, c.y, c.z);
      continue;
    }
    const std::size_t n = core::DiffChunk(regenerate(c), FromVoxels(saved->voxels)).size();
    total += n;
    if (n == 0) ++unchanged;
    std::printf("chunk %d %d %d, revision %u: %zu voxels changed%s\n", c.x, c.y, c.z, revision, n,
                n == 0 ? " (identical to generation)" : "");
  }
  std::printf("%zu voxels changed in total; %zu saved chunks identical to generation\n", total,
              unchanged);
  return 0;
}
