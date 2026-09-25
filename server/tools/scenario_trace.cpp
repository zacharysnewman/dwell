// Prints the four-player scenario's per-tick trace (position and velocity at full float precision)
// for the native↔WASM divergence check (PLAYER_CONTROLLER.md §8.3, tools/divergence.mjs). Built
// natively and with Emscripten (run under Node).
#include <cstdio>

#include "scenario.h"

int main() {
  for (const auto& r : dwell::test::RecordFullTrace()) {
    std::printf("%d %u %.9g %.9g %.9g %.9g %.9g %.9g %d\n", r.tick, r.player, r.position[0],
                r.position[1], r.position[2], r.velocity[0], r.velocity[1], r.velocity[2], r.state);
  }
  return 0;
}
