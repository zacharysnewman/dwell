#pragma once

#include <condition_variable>
#include <cstdint>
#include <deque>
#include <memory>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

#include "dwell/storage/world_db.h"

// The server's handle on its world file (ARCHITECTURE.md §6.4): loads on the tick thread, saves
// prepared on the tick and committed off it. Natively a second connection commits on an I/O thread
// (WAL: the tick's reads never wait for it); in the browser, with no threads, saves commit on the
// caller's thread.
namespace dwell::storage {

class WorldStore {
 public:
  // `writer`: a second connection to the same file for the I/O thread; null commits inline.
  explicit WorldStore(std::unique_ptr<WorldDb> db, std::unique_ptr<WorldDb> writer = nullptr);
  ~WorldStore();  // commits what is queued, then stops the thread
  WorldStore(const WorldStore&) = delete;
  WorldStore& operator=(const WorldStore&) = delete;

  // Opens `path` natively: the tick's connection plus the I/O thread's. Null and `error` on
  // failure.
  static std::unique_ptr<WorldStore> OpenFile(const std::string& path, std::string& error);

  // The tick thread's connection (loads, settings).
  WorldDb& db() { return *db_; }
  bool threaded() const { return writer_ != nullptr; }

  // Queues (threaded) or commits the batch; returns its id.
  std::uint64_t Save(SaveBatch batch);

  struct Result {
    std::uint64_t id = 0;
    bool ok = false;
    std::string error;
  };
  // Saves finished since the last call, in order.
  std::vector<Result> TakeCompleted();
  // Blocks until every queued save has been committed.
  void Flush();

 private:
  void Run();

  std::unique_ptr<WorldDb> db_;
  std::unique_ptr<WorldDb> writer_;
  std::mutex mutex_;
  std::condition_variable wake_, idle_;
  std::deque<std::pair<std::uint64_t, SaveBatch>> queue_;
  std::vector<Result> completed_;
  bool busy_ = false;
  bool stop_ = false;
  std::uint64_t next_id_ = 1;
  std::thread thread_;
};

}  // namespace dwell::storage
