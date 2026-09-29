#include "dwell/core/lod_propagation.h"

#include <algorithm>
#include <limits>
#include <utility>

#include "dwell/protocol/messages.h"

namespace dwell::core {

namespace {

constexpr std::size_t kMaxGenerated = 256;  // full-chunk-mode sections kept for answering

double DistanceSquared(const LodCoord& c, const std::vector<std::array<double, 3>>& centers) {
  if (centers.empty()) return 0;
  const LodOrigin o = LodSectionOrigin(c);
  const double half = static_cast<double>(LodSectionSize(c.level)) / 2;
  double best = std::numeric_limits<double>::infinity();
  for (const auto& p : centers) {
    const double dx = static_cast<double>(o.x) + half - p[0];
    const double dy = static_cast<double>(o.y) + half - p[1];
    const double dz = static_cast<double>(o.z) + half - p[2];
    best = std::min(best, dx * dx + dy * dy + dz * dz);
  }
  return best;
}

LodCells Decode(const std::vector<std::uint8_t>& encoded) {
  auto cells = protocol::DecodeLodCells(encoded);
  return cells ? std::move(*cells) : LodCells(kLodVolume, Materials::kAir);
}

}  // namespace

LodPropagation::LodPropagation(LodGenerator generator, int threads)
    : generator_(std::move(generator)) {
  for (int i = 0; i < threads; ++i) workers_.emplace_back([this] { Work(); });
}

LodPropagation::~LodPropagation() {
  {
    std::lock_guard lock(mutex_);
    stop_ = true;
  }
  work_.notify_all();
  for (auto& t : workers_) t.join();
}

void LodPropagation::MarkChunk(const ChunkCoord& c) { MarkDirty(LodParent(LodOfChunk(c))); }

void LodPropagation::MarkDirty(const LodCoord& c) {
  if (LodInWorld(c)) dirty_.insert(c);
}

void LodPropagation::Restore(const LodCoord& c, LodSection section) {
  next_revision_ = std::max(next_revision_, section.revision + 1);
  sections_[c] = std::move(section);
}

LodPropagation::Job LodPropagation::Snapshot(const LodCoord& c,
                                             const ModifiedChunk& modified) const {
  Job job;
  job.coord = c;
  for (int o = 0; o < 8; ++o) {
    const LodCoord child = LodChild(c, o);
    if (child.level == 0) {
      if (const Chunk* chunk = modified ? modified(ChunkOfLod(child)) : nullptr) {
        job.children[o].assign(chunk->voxels().begin(), chunk->voxels().end());
      }
    } else if (const LodSection* s = Find(child)) {
      job.children[o] = Decode(s->encoded);
    }
  }
  return job;
}

LodPropagation::Result LodPropagation::Run(const LodGenerator& generator, const Job& job) {
  LodCells cells;
  generator(job.coord, cells);
  if (!job.generate_only) {
    for (int o = 0; o < 8; ++o) {
      const auto& child = job.children[o];
      if (child.empty()) continue;  // unmodified: the generated octant stays
      if (job.coord.level == 1) {
        DownsampleOctant(
            [&](int x, int y, int z) {
              return child[static_cast<std::size_t>(LocalIndex(x, y, z))];
            },
            o, cells);
      } else {
        DownsampleSectionOctant(child, o, cells);
      }
    }
  }
  return {job.coord, job.generate_only, protocol::EncodeLodCells(cells)};
}

void LodPropagation::Work() {
  std::unique_lock lock(mutex_);
  for (;;) {
    work_.wait(lock, [&] { return stop_ || !queue_.empty(); });
    if (stop_) return;
    Job job = std::move(queue_.front());
    queue_.pop_front();
    ++running_;
    lock.unlock();
    Result r = Run(generator_, job);
    lock.lock();
    --running_;
    done_.push_back(std::move(r));
    idle_.notify_all();
  }
}

void LodPropagation::Collect(std::vector<Result>& out) {
  std::lock_guard lock(mutex_);
  for (auto& r : done_) out.push_back(std::move(r));
  done_.clear();
}

void LodPropagation::Apply(Result& r) {
  if (r.generate_only) {
    generating_.erase(r.coord);
    if (generated_.size() >= kMaxGenerated && !generated_order_.empty()) {
      generated_.erase(generated_order_.front());
      generated_order_.pop_front();
    }
    generated_order_.push_back(r.coord);
    generated_[r.coord] = std::move(r.encoded);
    return;
  }
  in_flight_.erase(r.coord);
  sections_[r.coord] = {next_revision_++, std::move(r.encoded)};
  written_.push_back(r.coord);
  if (r.coord.level < kLodMaxLevel) MarkDirty(LodParent(r.coord));
}

void LodPropagation::Step(const std::vector<std::array<double, 3>>& centers,
                          const ModifiedChunk& modified, int budget,
                          std::chrono::microseconds inline_budget) {
  std::vector<Result> results;
  Collect(results);
  for (Result& r : results) Apply(r);

  // Lowest level first (children before parents), then nearest to a player.
  if (!dirty_.empty() && budget > 0) {
    std::vector<std::pair<std::pair<int, double>, LodCoord>> ready;
    for (const LodCoord& c : dirty_) {
      if (!in_flight_.count(c)) ready.push_back({{c.level, DistanceSquared(c, centers)}, c});
    }
    const std::size_t n = std::min(ready.size(), static_cast<std::size_t>(budget));
    std::partial_sort(ready.begin(), ready.begin() + static_cast<std::ptrdiff_t>(n), ready.end(),
                      [](const auto& a, const auto& b) { return a.first < b.first; });
    std::vector<Job> jobs;
    for (std::size_t i = 0; i < n; ++i) {
      const LodCoord c = ready[i].second;
      dirty_.erase(c);
      in_flight_.insert(c);
      jobs.push_back(Snapshot(c, modified));
    }
    {
      std::lock_guard lock(mutex_);
      for (Job& j : jobs) queue_.push_back(std::move(j));
    }
    work_.notify_all();
  }

  if (workers_.empty()) {
    const auto deadline = std::chrono::steady_clock::now() + inline_budget;
    while (!queue_.empty()) {
      Job job = std::move(queue_.front());
      queue_.pop_front();
      done_.push_back(Run(generator_, job));
      if (std::chrono::steady_clock::now() >= deadline) break;
    }
    results.clear();
    Collect(results);
    for (Result& r : results) Apply(r);
  }
}

void LodPropagation::Drain(const std::vector<std::array<double, 3>>& centers,
                           const ModifiedChunk& modified) {
  while (!dirty_.empty() || !in_flight_.empty() || !generating_.empty()) {
    Step(centers, modified, std::numeric_limits<int>::max(), std::chrono::hours(1));
    if (!workers_.empty()) {
      std::unique_lock lock(mutex_);
      idle_.wait(lock, [&] { return !done_.empty() || (queue_.empty() && running_ == 0); });
    }
  }
}

std::vector<LodCoord> LodPropagation::TakeWritten() { return std::exchange(written_, {}); }

const std::vector<std::uint8_t>* LodPropagation::Generated(const LodCoord& c) {
  if (const auto it = generated_.find(c); it != generated_.end()) return &it->second;
  if (generating_.insert(c).second) {
    Job job;
    job.coord = c;
    job.generate_only = true;
    {
      std::lock_guard lock(mutex_);
      queue_.push_back(std::move(job));
    }
    work_.notify_all();
  }
  return nullptr;
}

std::vector<std::uint16_t> LodPropagation::CellsForClient(const LodCoord& c) const {
  const LodSection* s = Find(c);
  if (!s) return {};
  LodCells cells = Decode(s->encoded);
  constexpr int N = kLodSectionCells;
  // Face neighbours: axis 0 x, 1 y, 2 z; our apron layer and their border layer.
  for (int axis = 0; axis < 3; ++axis) {
    for (const int dir : {-1, 1}) {
      LodCoord n = c;
      (axis == 0 ? n.i : axis == 1 ? n.j : n.k) += dir;
      const LodSection* ns = Find(n);
      if (!ns) continue;
      const LodCells other = Decode(ns->encoded);
      const int ours = dir > 0 ? N : -1, theirs = dir > 0 ? 0 : N - 1;
      for (int a = 0; a < N; ++a)
        for (int b = 0; b < N; ++b) {
          const auto at = [&](int layer) {
            return axis == 0   ? LodCell(layer, a, b)
                   : axis == 1 ? LodCell(a, layer, b)
                               : LodCell(a, b, layer);
          };
          cells[static_cast<std::size_t>(at(ours))] = other[static_cast<std::size_t>(at(theirs))];
        }
    }
  }
  return cells;
}

}  // namespace dwell::core
