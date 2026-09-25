# Scenario trace for the native↔WASM divergence check (PLAYER_CONTROLLER.md §8.3). The same source
# builds natively and, in the wasm preset, as a Node program (dwell_scenario_trace.js).
add_executable(dwell_scenario_trace scenario_trace.cpp)
target_include_directories(dwell_scenario_trace PRIVATE ${CMAKE_CURRENT_SOURCE_DIR}/../tests/player)
target_link_libraries(dwell_scenario_trace PRIVATE dwell_core dwell_options)
if(EMSCRIPTEN)
  target_link_options(dwell_scenario_trace PRIVATE -sENVIRONMENT=node -sALLOW_MEMORY_GROWTH=1
                      -sINITIAL_MEMORY=64MB -sSTACK_SIZE=1MB)
endif()

if(EMSCRIPTEN)
  # The ported player controller suite under Node (the exit criterion "natively and in WASM"):
  # node build/wasm/tools/dwell_player_tests.js. NODERAWFS reads the golden trace from disk.
  file(GLOB PLAYER_TESTS ${CMAKE_CURRENT_SOURCE_DIR}/../tests/player/*_test.cpp)
  add_executable(dwell_player_tests ${CMAKE_CURRENT_SOURCE_DIR}/../tests/main.cpp ${PLAYER_TESTS})
  target_link_libraries(dwell_player_tests PRIVATE dwell_core dwell_options doctest::doctest monocypher)
  target_compile_definitions(dwell_player_tests PRIVATE
    DWELL_PLAYER_GOLDEN="${CMAKE_CURRENT_SOURCE_DIR}/../tests/player/golden/scenario-trace.txt")
  target_link_options(dwell_player_tests PRIVATE -sENVIRONMENT=node -sNODERAWFS=1
                      -sALLOW_MEMORY_GROWTH=1 -sINITIAL_MEMORY=128MB -sSTACK_SIZE=4MB)

  # The worldgen suite under Node: its golden chunk hashes must match the native build's
  # (ARCHITECTURE.md §6.3, ADR 0010). node build/wasm/tools/dwell_worldgen_tests.js
  file(GLOB WORLDGEN_TESTS ${CMAKE_CURRENT_SOURCE_DIR}/../tests/worldgen/*_test.cpp)
  add_executable(dwell_worldgen_tests ${CMAKE_CURRENT_SOURCE_DIR}/../tests/main.cpp ${WORLDGEN_TESTS})
  target_link_libraries(dwell_worldgen_tests PRIVATE dwell_core dwell_options doctest::doctest)
  target_compile_definitions(dwell_worldgen_tests PRIVATE
    DWELL_WORLDGEN_GOLDEN="${CMAKE_CURRENT_SOURCE_DIR}/../tests/worldgen/golden/chunk-hashes.txt")
  target_link_options(dwell_worldgen_tests PRIVATE -sENVIRONMENT=node -sNODERAWFS=1
                      -sALLOW_MEMORY_GROWTH=1 -sINITIAL_MEMORY=128MB -sSTACK_SIZE=4MB)
endif()

# Worldgen inspection (ASCII map, biome shares, timings, spawn): Phase 3 debug tooling.
add_executable(dwell_worldgen_inspect worldgen_inspect.cpp)
target_link_libraries(dwell_worldgen_inspect PRIVATE dwell_core dwell_options)
if(EMSCRIPTEN)
  target_link_options(dwell_worldgen_inspect PRIVATE -sENVIRONMENT=node -sALLOW_MEMORY_GROWTH=1
                      -sINITIAL_MEMORY=64MB -sSTACK_SIZE=1MB)
endif()

# Worldgen noise numerics prototype: strict float vs 16.16 fixed point (ADR 0010).
add_executable(dwell_noise_numerics noise_numerics.cpp)
target_link_libraries(dwell_noise_numerics PRIVATE dwell_core dwell_options)
if(EMSCRIPTEN)
  target_link_options(dwell_noise_numerics PRIVATE -sENVIRONMENT=node -sALLOW_MEMORY_GROWTH=1)
endif()
