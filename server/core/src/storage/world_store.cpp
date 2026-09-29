#include "dwell/storage/world_store.h"

namespace dwell::storage {

WorldStore::WorldStore(std::unique_ptr<WorldDb> db, std::unique_ptr<WorldDb> writer)
    : db_(std::move(db)), writer_(std::move(writer)) {
  if (writer_) thread_ = std::thread([this] { Run(); });
}

WorldStore::~WorldStore() {
  if (!thread_.joinable()) return;
  {
    std::lock_guard lock(mutex_);
    stop_ = true;
  }
  wake_.notify_all();
  thread_.join();
}

std::unique_ptr<WorldStore> WorldStore::OpenFile(const std::string& path, std::string& error) {
  auto db = WorldDb::Open(path, error);
  if (!db) return nullptr;
  auto writer = WorldDb::Open(path, error);
  if (!writer) return nullptr;
  return std::make_unique<WorldStore>(std::move(db), std::move(writer));
}

std::uint64_t WorldStore::Save(SaveBatch batch) {
  std::unique_lock lock(mutex_);
  const std::uint64_t id = next_id_++;
  if (!writer_) {
    lock.unlock();
    Result r{id, false, {}};
    r.ok = db_->Save(batch, r.error);
    lock.lock();
    completed_.push_back(std::move(r));
    return id;
  }
  queue_.emplace_back(id, std::move(batch));
  lock.unlock();
  wake_.notify_one();
  return id;
}

std::vector<WorldStore::Result> WorldStore::TakeCompleted() {
  std::lock_guard lock(mutex_);
  return std::exchange(completed_, {});
}

void WorldStore::Flush() {
  std::unique_lock lock(mutex_);
  idle_.wait(lock, [&] { return queue_.empty() && !busy_; });
}

void WorldStore::Run() {
  std::unique_lock lock(mutex_);
  for (;;) {
    wake_.wait(lock, [&] { return stop_ || !queue_.empty(); });
    if (queue_.empty()) break;  // stopping, nothing left
    auto [id, batch] = std::move(queue_.front());
    queue_.pop_front();
    busy_ = true;
    lock.unlock();
    Result r{id, false, {}};
    r.ok = writer_->Save(batch, r.error);
    lock.lock();
    busy_ = false;
    completed_.push_back(std::move(r));
    idle_.notify_all();
  }
  idle_.notify_all();
}

}  // namespace dwell::storage
