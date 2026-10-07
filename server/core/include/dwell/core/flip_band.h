#pragma once

#include <cmath>

// The flip band's approach cushion (BIFACIAL_WORLD.md §3, ADR 0023). The band itself — gravity
// fading to zero and a drag over the last few metres either side of the midplane — only works on a
// body slow enough to spend ticks inside it: a fall of 2 km (the rim's drop) arrives at 280 m/s,
// nearly five metres a tick, and would cross the band's eight in two ticks. So a body approaching
// the midplane may not exceed a limit that falls with its height: constant braking from far away
// (`decel`, a few g) down to `drag · band` at the band's edge, which the drag then stops within the
// band, and `drag · h` inside it (an exponential approach).
namespace dwell::core {

// The fastest a body at height `h` (m above the midplane on its own side, ≥ 0) may move toward it.
inline float ApproachSpeedLimit(float h, float band, float drag, float decel) {
  if (h <= band) return drag * (h > 0.0f ? h : 0.0f);
  const float edge = drag * band;
  return std::sqrt(edge * edge + 2.0f * decel * (h - band));
}

}  // namespace dwell::core
