add_executable(dwell_tests
  main.cpp
  app_version_test.cpp
  jolt_smoke_test.cpp
  net_abi_test.cpp
  protocol_test.cpp
  core_test.cpp
  server_test.cpp
  streaming_test.cpp
  chunk_request_test.cpp
  hosting_test.cpp
  block_edit_test.cpp
  block_registry_test.cpp
  block_shape_test.cpp
  lod_server_test.cpp
  storage/storage_test.cpp
  player/movement_test.cpp
  player/vertical_crouch_test.cpp
  player/platform_climb_swim_test.cpp
  player/flight_test.cpp
  player/voxel_query_test.cpp
  player/slope_test.cpp
  player/scenario_test.cpp
  player/netcode_test.cpp
  worldgen/worldgen_test.cpp
  worldgen/lod_test.cpp
  worldgen/slopes_test.cpp
  worldgen/slope_terrain_test.cpp)
target_link_libraries(dwell_tests PRIVATE dwell_core dwell_net dwell_options doctest::doctest monocypher
                      dwell_sqlite)
add_test(NAME dwell_tests COMMAND dwell_tests)
target_compile_definitions(dwell_tests PRIVATE
  DWELL_VERSION_VECTORS="${CMAKE_CURRENT_SOURCE_DIR}/../../shared/version/vectors.txt"
  DWELL_BLOCK_VECTORS="${CMAKE_CURRENT_SOURCE_DIR}/../../shared/blocks/vectors.txt"
  DWELL_PROTOCOL_VECTORS="${CMAKE_CURRENT_SOURCE_DIR}/../../shared/protocol/vectors.txt"
  DWELL_PLAYER_GOLDEN="${CMAKE_CURRENT_SOURCE_DIR}/player/golden/scenario-trace.txt"
  DWELL_WORLDGEN_GOLDEN="${CMAKE_CURRENT_SOURCE_DIR}/worldgen/golden/chunk-hashes.txt"
  DWELL_LOD_GOLDEN="${CMAKE_CURRENT_SOURCE_DIR}/worldgen/golden/lod-hashes.txt"
  DWELL_STORAGE_GOLDEN="${CMAKE_CURRENT_SOURCE_DIR}/storage/golden")

# The real dwell_server on world files saved by other versions (RELEASES.md §6).
find_package(Python3 COMPONENTS Interpreter)
if(Python3_FOUND)
  add_test(NAME dwell_version_lock
           COMMAND ${Python3_EXECUTABLE} ${CMAKE_CURRENT_SOURCE_DIR}/version_lock_test.py
                   $<TARGET_FILE:dwell_server> $<TARGET_FILE:dwell_world>)
endif()
