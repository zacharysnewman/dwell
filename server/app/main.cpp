// Dwell dedicated server entry point. Phase 0: prove the C++ core, Jolt, and the Rust network
// crate link together and report their versions.
#include <cstdio>

#include "dwell/core/jolt_runtime.h"
#include "dwell_net.h"

int main() {
  dwell::core::JoltRuntime jolt;
  std::printf("dwell_server (phase 0) | %s | dwell-net %s (C ABI v%u)\n",
              dwell::core::JoltVersionString(), dwell_net_crate_version(), dwell_net_abi_version());
  return 0;
}
