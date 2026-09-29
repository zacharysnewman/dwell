#pragma once

#include <chrono>
#include <condition_variable>
#include <deque>
#include <memory>
#include <mutex>
#include <thread>
#include <unordered_set>
#include <utility>
#include <vector>

#include "dwell/core/voxel.h"

// Off-tick chunk generation (ARCHITECTURE.md §6.3, ADR 0007). The server says which chunks it
// wants next (nearest first); worker threads generate them and the tick collects the results.
// With zero threads (the browser's single-threaded local mode) Collect() generates on the calling
// thread instead, within a time budget, so a tick never stalls on a burst of new terrain.
namespace dwell::core {

class WorldgenPool {
 public:
  using Result = std::pair<ChunkCoord, std::unique_ptr<Chunk>>;

  WorldgenPool(ChunkGenerator generator, int threads);
  ~WorldgenPool();

  WorldgenPool(const WorldgenPool&) = delete;
  WorldgenPool& operator=(const WorldgenPool&) = delete;

  // Replaces the queue of chunks to generate, in priority order. Chunks already being generated
  // or finished (not yet collected) are skipped.
  void SetWanted(const std::vector<ChunkCoord>& wanted);

  // Moves finished chunks to `out`. Without threads, first generates queued chunks here until
  // `budget` has elapsed (at least one when any is queued).
  void Collect(std::vector<Result>& out, std::chrono::microseconds budget);

  // Blocks until every queued chunk is generated, then collects them (startup pre-generation).
  void Drain(std::vector<Result>& out);

  int threads() const { return static_cast<int>(workers_.size()); }

 private:
  void Work();
  bool Pending(const ChunkCoord& c) const;  // queued, in flight, or done (lock held)

  ChunkGenerator generator_;
  std::vector<std::thread> workers_;
  mutable std::mutex mutex_;
  std::condition_variable work_;  // queue non-empty or stopping
  std::condition_variable idle_;  // a chunk finished
  std::deque<ChunkCoord> queue_;
  std::unordered_set<ChunkCoord, ChunkCoordHash> in_flight_;
  std::vector<Result> done_;
  bool stop_ = false;
};

}  // namespace dwell::core
