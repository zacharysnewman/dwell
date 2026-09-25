# Simulation core: platform-free; compiled natively and (from Phase 1) to WASM. ARCHITECTURE.md §4.3.
add_library(dwell_core
  src/crypto.cpp
  src/entropy.cpp
  src/jolt_runtime.cpp
  src/physics_world.cpp
  src/server.cpp
  src/terrain_collision.cpp
  src/voxel.cpp
  src/protocol/bytes.cpp
  src/protocol/messages.cpp
  src/player/controller.cpp
  src/player/net.cpp
  src/player/predictor.cpp
  src/player/voxel_query.cpp)
target_include_directories(dwell_core PUBLIC include)
target_link_libraries(dwell_core PUBLIC Jolt PRIVATE dwell_options monocypher)
