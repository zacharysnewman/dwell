cmake_minimum_required(VERSION 3.24)
project(dwell_server LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

option(DWELL_BUILD_TESTS "Build Dwell server unit tests" ON)

include(cmake/Dependencies.cmake)

# Flags for Dwell's own targets. No FMA contraction, to keep native and WASM builds as close as
# possible (PLAYER_CONTROLLER.md §8.3); Jolt applies the same via CROSS_PLATFORM_DETERMINISTIC.
add_library(dwell_options INTERFACE)
if(MSVC)
  target_compile_options(dwell_options INTERFACE /W4 /permissive- /fp:precise)
else()
  target_compile_options(dwell_options INTERFACE -Wall -Wextra -Wpedantic -ffp-contract=off)
endif()

add_subdirectory(core)
add_subdirectory(tools)

if(EMSCRIPTEN)
  # Browser build: the simulation core only (no networking, no native app). ADR 0007.
  add_subdirectory(wasm)
else()
  add_subdirectory(net)
  add_subdirectory(app)
  if(DWELL_BUILD_TESTS)
    enable_testing()
    add_subdirectory(tests)
  endif()
endif()
