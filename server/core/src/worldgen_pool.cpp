#include "dwell/core/worldgen_pool.h"

#include <algorithm>

namespace dwell::core {
namespace {

std::unique_ptr<Chunk> Generate(const ChunkGenerator& generator, const ChunkCoord& coord) {
  auto chunk = std::make_unique<Chunk>();
  generator(coord, *chunk);
  chunk->ResetRevision();
  return chunk;
}

}  // namespace

WorldgenPool::WorldgenPool(ChunkGenerator generator, int threads)
    : generator_(std::move(generator)) {
  for (int i = 0; i < threads; ++i) workers_.emplace_back([this] { Work(); });
}

WorldgenPool::~WorldgenPool() {
  {
    std::lock_guard lock(mutex_);
    stop_ = true;
  }
  work_.notify_all();
  for (auto& t : workers_) t.join();
}

bool WorldgenPool::Pending(const ChunkCoord& c) const {
  return in_flight_.count(c) != 0 ||
         std::any_of(done_.begin(), done_.end(), [&](const Result& r) { return r.first == c; });
}

void WorldgenPool::SetWanted(const std::vector<ChunkCoord>& wanted) {
  {
    std::lock_guard lock(mutex_);
    queue_.clear();
    std::unordered_set<ChunkCoord, ChunkCoordHash> seen;
    for (const ChunkCoord& c : wanted) {
      if (seen.insert(c).second && !Pending(c)) queue_.push_back(c);
    }
  }
  work_.notify_all();
}

void WorldgenPool::Collect(std::vector<Result>& out, std::chrono::microseconds budget) {
  if (workers_.empty()) {
    const auto deadline = std::chrono::steady_clock::now() + budget;
    while (!queue_.empty()) {
      const ChunkCoord c = queue_.front();
      queue_.pop_front();
      out.emplace_back(c, Generate(generator_, c));
      if (std::chrono::steady_clock::now() >= deadline) break;
    }
    return;
  }
  std::lock_guard lock(mutex_);
  for (auto& r : done_) out.push_back(std::move(r));
  done_.clear();
}

void WorldgenPool::Drain(std::vector<Result>& out) {
  if (workers_.empty()) {
    while (!queue_.empty()) Collect(out, std::chrono::hours(1));
    return;
  }
  std::unique_lock lock(mutex_);
  idle_.wait(lock, [&] { return queue_.empty() && in_flight_.empty(); });
  for (auto& r : done_) out.push_back(std::move(r));
  done_.clear();
}

void WorldgenPool::Work() {
  std::unique_lock lock(mutex_);
  for (;;) {
    work_.wait(lock, [&] { return stop_ || !queue_.empty(); });
    if (stop_) return;
    const ChunkCoord c = queue_.front();
    queue_.pop_front();
    in_flight_.insert(c);
    lock.unlock();
    auto chunk = Generate(generator_, c);
    lock.lock();
    in_flight_.erase(c);
    done_.emplace_back(c, std::move(chunk));
    idle_.notify_all();
  }
}

}  // namespace dwell::core
