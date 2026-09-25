#pragma once

namespace dwell::core {

// Process-wide Jolt setup (allocator, factory, type registration). Construct once before creating
// any physics system; destroy after the last one is gone.
class JoltRuntime {
 public:
  JoltRuntime();
  ~JoltRuntime();

  JoltRuntime(const JoltRuntime&) = delete;
  JoltRuntime& operator=(const JoltRuntime&) = delete;
};

// "Jolt x.y.z" for logs and the build-info banner.
const char* JoltVersionString();

}  // namespace dwell::core
