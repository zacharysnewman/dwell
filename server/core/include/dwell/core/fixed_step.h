#pragma once

#include <algorithm>

namespace dwell::core {

// Fixed-timestep accumulator (ARCHITECTURE.md §4.2). The simulation always advances in whole
// steps of `1 / hz`; real time that doesn't fill a step carries over to the next call.
class FixedStep {
 public:
  explicit FixedStep(int hz, int max_steps_per_advance = 8)
      : dt_(1.0 / hz), max_steps_(max_steps_per_advance) {}

  // Adds `elapsed_seconds` of real time and returns how many steps to run now. If the host fell
  // far behind (e.g. a stall), at most `max_steps_per_advance` are returned and the rest of the
  // backlog is dropped, so a slow frame can't snowball.
  int Advance(double elapsed_seconds) {
    accumulator_ += std::max(0.0, elapsed_seconds);
    int steps = static_cast<int>(accumulator_ / dt_);
    if (steps > max_steps_) {
      steps = max_steps_;
      accumulator_ = 0.0;
    } else {
      accumulator_ -= steps * dt_;
    }
    return steps;
  }

  double dt() const { return dt_; }
  // Seconds until the next step is due.
  double TimeUntilNextStep() const { return std::max(0.0, dt_ - accumulator_); }

 private:
  double dt_;
  int max_steps_;
  double accumulator_ = 0.0;
};

}  // namespace dwell::core
