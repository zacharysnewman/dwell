# Simulation core: platform-free; compiled natively and (from Phase 1) to WASM. ARCHITECTURE.md §4.3.
add_library(dwell_core
  src/jolt_runtime.cpp)
target_include_directories(dwell_core PUBLIC include)
target_link_libraries(dwell_core PUBLIC Jolt PRIVATE dwell_options)
