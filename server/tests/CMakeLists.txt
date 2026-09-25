add_executable(dwell_tests
  main.cpp
  jolt_smoke_test.cpp
  net_abi_test.cpp
  protocol_test.cpp
  core_test.cpp
  server_test.cpp
  player/movement_test.cpp
  player/vertical_crouch_test.cpp
  player/platform_climb_swim_test.cpp
  player/scenario_test.cpp
  player/netcode_test.cpp
  worldgen/worldgen_test.cpp)
target_link_libraries(dwell_tests PRIVATE dwell_core dwell_net dwell_options doctest::doctest monocypher)
add_test(NAME dwell_tests COMMAND dwell_tests)
target_compile_definitions(dwell_tests PRIVATE
  DWELL_PROTOCOL_VECTORS="${CMAKE_CURRENT_SOURCE_DIR}/../../shared/protocol/vectors.txt"
  DWELL_PLAYER_GOLDEN="${CMAKE_CURRENT_SOURCE_DIR}/player/golden/scenario-trace.txt"
  DWELL_WORLDGEN_GOLDEN="${CMAKE_CURRENT_SOURCE_DIR}/worldgen/golden/chunk-hashes.txt")
