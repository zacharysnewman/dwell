#pragma once

#include <array>
#include <chrono>
#include <condition_variable>
#include <cstdint>
#include <deque>
#include <functional>
#include <mutex>
#include <optional>
#include <thread>
#include <unordered_map>
#include <unordered_set>
#include <vector>

#include "dwell/core/lod.h"
#include "dwell/core/voxel.h"

// The server's LOD sections (ARCHITECTURE.md §6.6): which sections are modified (any modified
// chunk below them), their content and lodRevision, and the propagation that keeps them current.
//
// A chunk change marks its level-1 section dirty. Each tick, up to
// LOD_PROPAGATION_SECTIONS_PER_TICK dirty sections — lowest level first, then nearest to a player —
// are snapshotted (their modified children's content) and computed off the tick: GenerateLod of the
// section with the octants of modified children replaced by their downsample. Writing a section
// gives it the next lodRevision and marks its parent dirty, up to the root. With no threads (the
// browser's local mode) the jobs run on the calling thread within a time budget. The same pool
// generates unmodified sections for full-chunk-mode clients.
namespace dwell::core {

struct LodSection {
  std::uint32_t revision = 0;
  std::vector<std::uint8_t> encoded;  // palette + RLE of the 34³ cells (protocol::EncodeLodCells)
};

class LodPropagation {
 public:
  // The content of a modified chunk (revision > 0 or saved), or null for an unmodified one.
  using ModifiedChunk = std::function<const Chunk*(const ChunkCoord&)>;

  LodPropagation(LodGenerator generator, int threads);
  ~LodPropagation();
  LodPropagation(const LodPropagation&) = delete;
  LodPropagation& operator=(const LodPropagation&) = delete;

  // A chunk's voxels changed (any VoxelModification source).
  void MarkChunk(const ChunkCoord& c);
  // Loading a saved world: a section's cached content, or a section still to (re)compute.
  void Restore(const LodCoord& c, LodSection section);
  void MarkDirty(const LodCoord& c);

  // Once per tick: collects finished jobs (writing sections, marking parents), then dispatches
  // dirty sections (at most `budget` per tick) nearest to `centers` (world metres), and without
  // threads runs queued jobs here for up to `inline_budget`.
  void Step(const std::vector<std::array<double, 3>>& centers, const ModifiedChunk& modified,
            int budget, std::chrono::microseconds inline_budget);
  // Blocks until nothing is dirty or in flight (tests, tools, shutdown saves).
  void Drain(const std::vector<std::array<double, 3>>& centers, const ModifiedChunk& modified);

  // Sections written since the last call, in order.
  std::vector<LodCoord> TakeWritten();

  // Generated content of an unmodified section (full-chunk-mode clients): encoded 34³ cells once
  // ready, else null (and queued). Kept in a small cache.
  const std::vector<std::uint8_t>* Generated(const LodCoord& c);

  // Modified: written, or dirty / being computed.
  bool Modified(const LodCoord& c) const {
    return sections_.count(c) != 0 || dirty_.count(c) != 0 || in_flight_.count(c) != 0;
  }
  const LodSection* Find(const LodCoord& c) const {
    const auto it = sections_.find(c);
    return it == sections_.end() ? nullptr : &it->second;
  }
  const std::unordered_map<LodCoord, LodSection, LodCoordHash>& sections() const {
    return sections_;
  }
  const std::unordered_set<LodCoord, LodCoordHash>& dirty() const { return dirty_; }
  const std::unordered_set<LodCoord, LodCoordHash>& in_flight() const { return in_flight_; }
  std::size_t pending() const { return dirty_.size() + in_flight_.size(); }
  std::uint32_t last_revision() const { return next_revision_ - 1; }
  void set_next_revision(std::uint32_t r) { next_revision_ = std::max(next_revision_, r); }
  int threads() const { return static_cast<int>(workers_.size()); }

  // A section's 34³ cells with its apron taken from the same-level neighbours that are modified
  // (the stored content's apron is generated): what LodData Explicit carries.
  std::vector<std::uint16_t> CellsForClient(const LodCoord& c) const;

 private:
  struct Job {
    LodCoord coord;
    bool generate_only = false;
    // Modified children (octant order): chunk voxels (level 1) or section cells (level ≥ 2).
    std::array<std::vector<MaterialId>, 8> children;
  };
  struct Result {
    LodCoord coord;
    bool generate_only = false;
    std::vector<std::uint8_t> encoded;
  };

  static Result Run(const LodGenerator& generator, const Job& job);
  void Work();
  void Collect(std::vector<Result>& out);
  void Apply(Result& r);
  Job Snapshot(const LodCoord& c, const ModifiedChunk& modified) const;

  LodGenerator generator_;
  std::vector<std::thread> workers_;
  std::mutex mutex_;
  std::condition_variable work_;
  std::condition_variable idle_;
  std::deque<Job> queue_;  // guarded by mutex_
  std::vector<Result> done_;
  int running_ = 0;
  bool stop_ = false;

  // Tick thread only.
  std::unordered_map<LodCoord, LodSection, LodCoordHash> sections_;
  std::unordered_set<LodCoord, LodCoordHash> dirty_;
  std::unordered_set<LodCoord, LodCoordHash> in_flight_;
  std::vector<LodCoord> written_;
  std::uint32_t next_revision_ = 1;
  // Full-chunk mode: generated sections (bounded), and those queued.
  std::unordered_map<LodCoord, std::vector<std::uint8_t>, LodCoordHash> generated_;
  std::deque<LodCoord> generated_order_;
  std::unordered_set<LodCoord, LodCoordHash> generating_;
};

}  // namespace dwell::core
