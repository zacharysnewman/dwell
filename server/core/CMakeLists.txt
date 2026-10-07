# Simulation core: platform-free; compiled natively and (from Phase 1) to WASM. ARCHITECTURE.md §4.3.
configure_file(src/app_version_build.cpp.in "${CMAKE_CURRENT_BINARY_DIR}/app_version_build.cpp" @ONLY)
add_library(dwell_core
  "${CMAKE_CURRENT_BINARY_DIR}/app_version_build.cpp"
  src/app_version.cpp
  src/block_edit.cpp
  src/crypto.cpp
  src/entropy.cpp
  src/jolt_runtime.cpp
  src/lod.cpp
  src/lod_propagation.cpp
  src/physics_world.cpp
  src/server.cpp
  src/terrain_collision.cpp
  src/block_registry.cpp
  src/block_shape.cpp
  src/voxel.cpp
  src/worldgen_pool.cpp
  src/protocol/bytes.cpp
  src/protocol/messages.cpp
  src/player/controller.cpp
  src/player/net.cpp
  src/player/predictor.cpp
  src/player/voxel_query.cpp
  src/worldgen/noise.cpp
  src/worldgen/continents.cpp
  src/worldgen/rivers.cpp
  src/worldgen/biomes.cpp
  src/worldgen/terrain.cpp
  src/worldgen/bifacial.cpp
  src/worldgen/slopes.cpp
  src/storage/world_db.cpp
  src/storage/world_store.cpp)
if(EMSCRIPTEN)
  # SQLite's OS layer in the browser: files are OPFS sync access handles (§6.4).
  target_sources(dwell_core PRIVATE src/storage/opfs_vfs.cpp)
endif()
target_include_directories(dwell_core PUBLIC include)
find_package(Threads REQUIRED)
target_link_libraries(dwell_core PUBLIC Jolt Threads::Threads PRIVATE dwell_options monocypher dwell_sqlite dwell_zstd)
